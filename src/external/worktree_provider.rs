//! Delegation of worktree creation, removal and discovery to external
//! commands.
//!
//! A provider owns its own directory layout, so bork stores the absolute path
//! it reports rather than deriving one from the project root. Providers that
//! allocate scarce per-worktree resources (ports, databases) also cap how many
//! worktrees can exist at once; `worktree_pool_size` carries that ceiling.
//!
//! Every invocation carries `BORK_PROVIDER_API` (currently `1`) and
//! `BORK_PROJECT_ROOT` alongside the call-specific variables, so a provider
//! can detect a future contract change and never has to infer the project
//! from its working directory alone.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use serde::Deserialize;

use crate::config::AppConfig;
use crate::worktrees::WorktreeEntry;

pub const PROVIDER_API_VERSION: &str = "1";

/// Provisioning a worktree (databases, containers) legitimately takes minutes;
/// enumeration feeds the poll loop and must never wedge it.
const CREATE_TIMEOUT: Duration = Duration::from_secs(1800);
const REMOVE_TIMEOUT: Duration = Duration::from_secs(600);
const LIST_TIMEOUT: Duration = Duration::from_secs(30);

/// Which machinery owns worktree creation, removal and discovery — resolved
/// once from config, so call sites dispatch on a value instead of re-testing
/// individual command fields.
pub enum WorktreeBackend<'a> {
    /// bork's own layout: `git worktree add` under the project root.
    Native,
    /// External commands own the layout and the provisioning.
    Provider(ProviderCommands<'a>),
}

impl<'a> WorktreeBackend<'a> {
    /// A provider exists exactly when `worktree_create_cmd` is set; the other
    /// commands refine it.
    pub fn resolve(config: &'a AppConfig) -> Self {
        match config.worktree_create_cmd.as_deref() {
            Some(create) => WorktreeBackend::Provider(ProviderCommands {
                config,
                create,
                remove: config.worktree_remove_cmd.as_deref(),
                list: config.worktree_list_cmd.as_deref(),
            }),
            None => WorktreeBackend::Native,
        }
    }
}

/// The provider's shell commands, run with the main checkout as their working
/// directory.
///
/// Both streams are captured, never inherited: these commands run while the
/// TUI owns the alternate screen, and anything a child writes to the terminal
/// scribbles over the board. Stderr is surfaced through the error instead.
pub struct ProviderCommands<'a> {
    config: &'a AppConfig,
    create: &'a str,
    remove: Option<&'a str>,
    list: Option<&'a str>,
}

impl ProviderCommands<'_> {
    pub fn config(&self) -> &AppConfig {
        self.config
    }

    /// Create a worktree via `worktree_create_cmd`.
    ///
    /// The command receives the issue's identity in the environment and must
    /// print the worktree as a JSON object on the last line of stdout.
    /// `BORK_BRANCH`, when set, is the exact branch to check out — used when a
    /// card already has one, such as a pull request's head branch — and
    /// overrides composing a name from the issue id and slug.
    pub fn create(
        &self,
        issue_id: &str,
        slug: Option<&str>,
        base_branch: Option<&str>,
        branch: Option<&str>,
    ) -> anyhow::Result<WorktreeEntry> {
        let stdout = run_provider_command(
            self.config,
            "worktree_create_cmd",
            self.create,
            &[
                ("BORK_ISSUE_ID", issue_id),
                ("BORK_SLUG", slug.unwrap_or_default()),
                ("BORK_BASE", base_branch.unwrap_or_default()),
                ("BORK_BRANCH", branch.unwrap_or_default()),
            ],
            CREATE_TIMEOUT,
        )?;

        let entry = parse_created(&stdout)?;

        if !entry.path.is_dir() {
            bail!(
                "worktree_create_cmd reported '{}', which is not a directory",
                entry.path.display()
            );
        }

        Ok(entry)
    }

    /// Remove a worktree via `worktree_remove_cmd`.
    ///
    /// `force` is passed as `BORK_FORCE=1`; the provider decides what that
    /// means for uncommitted work. A provider without a remove command fails
    /// here rather than falling back to `git worktree remove`, which would
    /// strand whatever else it provisioned alongside the directory.
    pub fn remove(&self, name: &str, path: &Path, force: bool) -> anyhow::Result<()> {
        let Some(script) = self.remove else {
            bail!("worktree_remove_cmd is not configured");
        };

        run_provider_command(
            self.config,
            "worktree_remove_cmd",
            script,
            &[
                ("BORK_WORKTREE_NAME", name),
                ("BORK_WORKTREE_PATH", &path.to_string_lossy()),
                ("BORK_FORCE", if force { "1" } else { "0" }),
            ],
            REMOVE_TIMEOUT,
        )
        .map(|_| ())
        // One flat line, not a context layer: callers surface the first line
        // of the message, and a wrapping hint would hide the actual failure.
        .map_err(|err| anyhow::anyhow!("{err:#}. Re-run with force to discard uncommitted work."))
    }

    /// Enumerate the provider's worktrees, treating any failure as "none".
    ///
    /// For display and polling only. A failing list command must never take
    /// the board down, but it must also never be mistaken for an empty pool —
    /// see [`Self::list_checked`] for anything that decides on the count.
    pub fn list(&self) -> Vec<WorktreeEntry> {
        self.list_checked().unwrap_or_default()
    }

    /// Enumerate the provider's worktrees, surfacing failure.
    ///
    /// A provider without a list command cannot be discovered by scanning the
    /// project root, so an empty list is the only honest answer there —
    /// returning the project root's children instead would silently mix two
    /// layouts.
    pub fn list_checked(&self) -> anyhow::Result<Vec<WorktreeEntry>> {
        let Some(script) = self.list else {
            return Ok(Vec::new());
        };

        let stdout =
            run_provider_command(self.config, "worktree_list_cmd", script, &[], LIST_TIMEOUT)?;

        Ok(parse_list(&stdout))
    }
}

/// Run one provider command to completion: shared environment, captured
/// streams, a deadline, and a uniform error carrying the last stderr line.
///
/// The deadline exists because `worktree_list_cmd` runs on every poll cycle
/// and create/remove hold a background thread: a hung script would otherwise
/// wedge them forever with nothing on screen but a spinner.
fn run_provider_command(
    config: &AppConfig,
    label: &str,
    script: &str,
    envs: &[(&str, &str)],
    timeout: Duration,
) -> anyhow::Result<String> {
    let mut command = Command::new("sh");
    command
        .args(["-c", script])
        .current_dir(config.main_dir())
        .env("BORK_PROVIDER_API", PROVIDER_API_VERSION)
        .env("BORK_PROJECT_ROOT", &config.project_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in envs {
        command.env(key, value);
    }

    let mut child = command
        .spawn()
        .with_context(|| format!("Failed to run {label}"))?;

    // Readers drain the pipes on their own threads so a chatty child never
    // blocks on a full pipe while this thread waits on it.
    let stdout_reader = drain_on_thread(child.stdout.take());
    let stderr_reader = drain_on_thread(child.stderr.take());

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(err).with_context(|| format!("{label} failed"));
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "{label} timed out after {}s and was killed",
                timeout.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    };

    // The child has exited, but the pipes close only when every holder does: a
    // script that backgrounded something with inherited stdio would otherwise
    // park this thread on a blocking read forever. Collect what arrives within
    // a grace window and stop waiting.
    let stdout = collect_stream(stdout_reader);
    let stderr = collect_stream(stderr_reader);

    if !status.success() {
        bail!(
            "{label} failed (exit {}): {}",
            status.code().unwrap_or(-1),
            last_line(&stderr)
        );
    }

    Ok(stdout)
}

/// How long to keep reading a pipe after its child exited. Closed pipes
/// resolve immediately; only an orphan still holding the write end runs this
/// out.
const STREAM_GRACE: Duration = Duration::from_secs(2);

fn drain_on_thread<R: Read + Send + 'static>(
    stream: Option<R>,
) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let Some(mut stream) = stream else { return };
        let mut chunk = [0u8; 8192];
        while let Ok(read) = stream.read(&mut chunk) {
            if read == 0 || tx.send(chunk[..read].to_vec()).is_err() {
                break;
            }
        }
    });
    rx
}

fn collect_stream(chunks: std::sync::mpsc::Receiver<Vec<u8>>) -> String {
    let mut buffer = Vec::new();
    let deadline = Instant::now() + STREAM_GRACE;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match chunks.recv_timeout(remaining) {
            Ok(chunk) => buffer.extend_from_slice(&chunk),
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&buffer).into_owned()
}

/// One worktree, as a provider reports it.
///
/// `name` defaults to the path's basename, so a provider with nothing
/// interesting to say can emit `{"path": "..."}` alone.
#[derive(Debug, Deserialize)]
struct WorktreeJson {
    path: String,
    #[serde(default)]
    name: Option<String>,
}

impl WorktreeJson {
    fn into_entry(self) -> anyhow::Result<WorktreeEntry> {
        let path = PathBuf::from(self.path);
        if !path.is_absolute() {
            bail!("worktree path '{}' is not absolute", path.display());
        }

        let name = match self.name {
            Some(name) if !name.trim().is_empty() => name.trim().to_string(),
            _ => worktree_name(&path)?,
        };
        // The name becomes a state key and later feeds removal; a separator or
        // dot-dot smuggled through here would aim those at the wrong place.
        if name == "." || name == ".." || name.contains(['/', '\\']) || name.contains('\0') {
            bail!("'{name}' is not a valid worktree name");
        }

        Ok(WorktreeEntry { name, path })
    }
}

/// Last non-empty line of a captured stream, for error messages. Providers
/// log progress before the failure, so the tail is the informative part.
fn last_line(stream: &str) -> String {
    stream
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no output")
        .to_string()
}

/// The created worktree, from the last non-empty line of stdout. Providers
/// commonly log progress first, so only the final line is authoritative.
fn parse_created(stdout: &str) -> anyhow::Result<WorktreeEntry> {
    let line = stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .with_context(|| format!("worktree_create_cmd printed nothing (stdout: {stdout:?})"))?;

    serde_json::from_str::<WorktreeJson>(line)
        .with_context(|| format!("worktree_create_cmd printed {line:?}, expected a JSON object"))?
        .into_entry()
}

/// Parse one JSON object per line.
///
/// A malformed line is skipped rather than failing the whole enumeration: this
/// feeds the poll loop, and one bad line must not blank the board.
fn parse_list(stdout: &str) -> Vec<WorktreeEntry> {
    parse_json_lines::<WorktreeJson>(stdout)
        .into_iter()
        .filter_map(|entry| entry.into_entry().ok())
        .collect()
}

/// Deserialize one `T` per non-empty line, dropping lines that will not parse.
fn parse_json_lines<T: for<'de> Deserialize<'de>>(stdout: &str) -> Vec<T> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_str::<T>(line).ok())
        .collect()
}

fn worktree_name(path: &Path) -> anyhow::Result<String> {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .with_context(|| format!("Cannot derive a worktree name from '{}'", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Provider config whose `main/` exists, since that is every provider
    /// command's working directory and the spawn fails without it.
    fn provider_config(list_cmd: &str) -> AppConfig {
        let root = std::env::temp_dir().join(format!("bork-provider-{}", std::process::id()));
        std::fs::create_dir_all(root.join("main")).expect("fixture main/");

        AppConfig {
            project_root: root,
            worktree_create_cmd: Some("true".into()),
            worktree_list_cmd: Some(list_cmd.into()),
            ..AppConfig::for_tests()
        }
    }

    fn provider(config: &AppConfig) -> ProviderCommands<'_> {
        match WorktreeBackend::resolve(config) {
            WorktreeBackend::Provider(provider) => provider,
            WorktreeBackend::Native => panic!("fixture config must resolve to a provider"),
        }
    }

    #[test]
    fn resolves_to_native_without_a_create_command() {
        let config = AppConfig {
            worktree_create_cmd: None,
            ..provider_config("unused")
        };

        assert!(matches!(
            WorktreeBackend::resolve(&config),
            WorktreeBackend::Native
        ));
    }

    #[test]
    fn remove_surfaces_stderr_on_failure() {
        let config = AppConfig {
            worktree_remove_cmd: Some("echo 'wt exploded' >&2; exit 4".into()),
            ..provider_config("unused")
        };

        let err = provider(&config)
            .remove("wt-1", Path::new("/tmp/wt-1"), false)
            .expect_err("a failing remove must not look like success");

        let chain = format!("{err:#}");
        assert!(chain.contains("exit 4"), "got: {chain}");
        assert!(chain.contains("wt exploded"), "got: {chain}");
    }

    #[test]
    fn remove_without_a_command_fails_instead_of_falling_back() {
        // `git worktree remove` on a provider worktree would strand whatever
        // else the provider provisioned alongside it.
        let config = AppConfig {
            worktree_remove_cmd: None,
            ..provider_config("unused")
        };

        let err = provider(&config)
            .remove("wt-1", Path::new("/tmp/wt-1"), false)
            .expect_err("no remove command must not look like success");

        assert!(
            err.to_string().contains("worktree_remove_cmd"),
            "got: {err}"
        );
    }

    #[test]
    fn commands_receive_the_shared_environment() {
        let config = AppConfig {
            worktree_remove_cmd: Some(
                r#"[ "$BORK_PROVIDER_API" = "1" ] && [ -n "$BORK_PROJECT_ROOT" ]"#.into(),
            ),
            ..provider_config("unused")
        };

        provider(&config)
            .remove("wt-1", Path::new("/tmp/wt-1"), false)
            .expect("API version and project root must be in the environment");
    }

    #[test]
    fn a_hung_command_is_killed_at_the_deadline() {
        let config = provider_config("unused");

        let err = run_provider_command(
            &config,
            "worktree_list_cmd",
            "sleep 30",
            &[],
            Duration::from_millis(150),
        )
        .expect_err("a hung provider must not wedge the caller");

        assert!(err.to_string().contains("timed out"), "got: {err}");
    }

    #[test]
    fn an_orphaned_pipe_holder_does_not_stall_a_finished_command() {
        // A script may background something that inherits stdio; the pipe then
        // stays open after the script exits, and a plain blocking drain would
        // wait on it forever.
        let config = provider_config("unused");
        let started = Instant::now();

        let stdout = run_provider_command(
            &config,
            "worktree_create_cmd",
            "echo done; sleep 30 >/dev/null &",
            &[],
            Duration::from_secs(20),
        )
        .expect("the command itself succeeded");

        assert!(stdout.contains("done"), "got: {stdout:?}");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "took {:?}, still waiting on the orphan's pipe",
            started.elapsed()
        );
    }

    #[test]
    fn last_line_falls_back_when_a_stream_is_silent() {
        assert_eq!(last_line("\n  \n"), "no output");
    }

    #[test]
    fn list_checked_reports_a_failing_command() {
        // Silently returning an empty list here would read as an empty pool
        // and disable the caller's ceiling.
        let config = provider_config("echo boom >&2; exit 3");
        let err = provider(&config)
            .list_checked()
            .expect_err("a failing list command must not look like an empty pool");

        assert!(err.to_string().contains("exit 3"), "got: {err}");
        assert!(err.to_string().contains("boom"), "got: {err}");
    }

    #[test]
    fn list_swallows_failure_for_display() {
        let config = provider_config("exit 3");
        assert!(provider(&config).list().is_empty());
    }

    #[test]
    fn list_checked_is_empty_without_a_list_command() {
        let config = AppConfig {
            worktree_list_cmd: None,
            ..provider_config("unused")
        };

        assert!(provider(&config)
            .list_checked()
            .expect("no command is not a failure")
            .is_empty());
    }

    #[test]
    fn parse_created_takes_the_last_line() {
        // Providers log progress before the result.
        let stdout = concat!(
            "Allocating slot 3\n",
            "Setting up databases\n",
            "{\"path\":\"/dev/wt--pool-a\",\"name\":\"wt--pool-a\"}\n",
        );

        let entry = parse_created(stdout).expect("should parse");

        assert_eq!(entry.name, "wt--pool-a");
        assert_eq!(entry.path, PathBuf::from("/dev/wt--pool-a"));
    }

    #[test]
    fn parse_created_defaults_the_name_to_the_basename() {
        let entry = parse_created("{\"path\":\"/dev/wt--pool-a\"}").expect("should parse");

        assert_eq!(entry.name, "wt--pool-a");
    }

    #[test]
    fn parse_created_reports_empty_output() {
        let err = parse_created("\n  \n").expect_err("nothing to parse");

        assert!(err.to_string().contains("printed nothing"), "got: {err}");
    }

    #[test]
    fn parse_created_reports_output_that_is_not_json() {
        // A provider emitting a bare path must fail loudly rather than have
        // its output misread as a name.
        let err = parse_created("/dev/wt--pool-a\n").expect_err("not an object");

        assert!(
            err.to_string().contains("expected a JSON object"),
            "got: {err}"
        );
    }

    #[test]
    fn parse_created_rejects_a_relative_path() {
        let err = parse_created("{\"path\":\"wt--pool-a\"}").expect_err("relative path");

        assert!(err.to_string().contains("not absolute"), "got: {err}");
    }

    #[test]
    fn parse_created_rejects_a_name_with_a_separator() {
        // The name becomes a state key and feeds removal.
        let err = parse_created("{\"path\":\"/dev/wt-a\",\"name\":\"../escape\"}")
            .expect_err("separator in name");

        assert!(
            err.to_string().contains("not a valid worktree name"),
            "got: {err}"
        );
    }

    #[test]
    fn parse_list_reads_objects_with_and_without_names() {
        let stdout = concat!(
            "{\"name\":\"wt-a\",\"path\":\"/dev/wt-a\"}\n",
            "{\"path\":\"/dev/wt-b\"}\n",
        );

        assert_eq!(
            parse_list(stdout),
            vec![
                WorktreeEntry {
                    name: "wt-a".into(),
                    path: PathBuf::from("/dev/wt-a"),
                },
                WorktreeEntry {
                    name: "wt-b".into(),
                    path: PathBuf::from("/dev/wt-b"),
                },
            ]
        );
    }

    #[test]
    fn parse_list_skips_malformed_lines() {
        // A half-written line must not become a worktree the board then tries
        // to poll or prune, and must not fail the whole enumeration either.
        let stdout = concat!(
            "{\"name\":\"wt-a\"}\n",
            "{oops\n",
            "\n",
            "{\"name\":\"wt-c\",\"path\":\"/dev/wt-c\"}\n",
        );

        assert_eq!(
            parse_list(stdout),
            vec![WorktreeEntry {
                name: "wt-c".into(),
                path: PathBuf::from("/dev/wt-c"),
            }]
        );
    }
}
