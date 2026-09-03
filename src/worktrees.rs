//! The single definition of "which worktrees exist".
//!
//! By default a worktree is a child directory of the project root. An external
//! provider places them wherever it likes, so it has to enumerate them itself;
//! scanning the project root would find nothing.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::AppConfig;
use crate::external::worktree_provider::WorktreeBackend;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub name: String,
    pub path: PathBuf,
}

/// Every worktree currently on disk, sorted by name.
pub fn enumerate(config: &AppConfig) -> Vec<WorktreeEntry> {
    let mut entries = match WorktreeBackend::resolve(config) {
        WorktreeBackend::Provider(provider) => provider.list(),
        WorktreeBackend::Native => scan_project_root(config),
    };

    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

/// Name-to-path lookup, surfacing a failing provider rather than reporting an
/// empty layout: an empty map reads as "every worktree is already gone", which
/// is the wrong answer for anything deciding whether one still exists.
pub fn path_index(config: &AppConfig) -> anyhow::Result<HashMap<String, PathBuf>> {
    let entries = match WorktreeBackend::resolve(config) {
        WorktreeBackend::Provider(provider) => provider.list_checked()?,
        WorktreeBackend::Native => scan_project_root(config),
    };

    Ok(entries
        .into_iter()
        .map(|entry| (entry.name, entry.path))
        .collect())
}

/// Every child directory of the project root holding a `.git`, except `main/`
/// and dotdirs.
fn scan_project_root(config: &AppConfig) -> Vec<WorktreeEntry> {
    let Ok(dir_entries) = std::fs::read_dir(&config.project_root) else {
        return Vec::new();
    };

    dir_entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name == "main" || name.starts_with('.') {
                return None;
            }
            let path = entry.path();
            if !path.join(".git").exists() {
                return None;
            }
            Some(WorktreeEntry { name, path })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_project() -> PathBuf {
        let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("bork-worktrees-test-{}-{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn config_for(project_root: &std::path::Path) -> AppConfig {
        AppConfig {
            project_root: project_root.to_path_buf(),
            ..AppConfig::for_tests()
        }
    }

    fn worktree_dir(project_root: &std::path::Path, name: &str) {
        let dir = project_root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(".git"), "gitdir: /somewhere").unwrap();
    }

    #[test]
    fn enumerate_finds_project_root_children() {
        let project = temp_project();
        worktree_dir(&project, "bork-2-second");
        worktree_dir(&project, "bork-1-first");

        let names: Vec<String> = enumerate(&config_for(&project))
            .into_iter()
            .map(|e| e.name)
            .collect();

        assert_eq!(names, vec!["bork-1-first", "bork-2-second"]);
        let _ = fs::remove_dir_all(&project);
    }

    #[test]
    fn enumerate_skips_main_dotdirs_and_plain_directories() {
        let project = temp_project();
        worktree_dir(&project, "main");
        worktree_dir(&project, ".bork");
        fs::create_dir_all(project.join("notes")).unwrap();
        worktree_dir(&project, "bork-1-first");

        let names: Vec<String> = enumerate(&config_for(&project))
            .into_iter()
            .map(|e| e.name)
            .collect();

        assert_eq!(names, vec!["bork-1-first"]);
        let _ = fs::remove_dir_all(&project);
    }

    #[test]
    fn enumerate_accepts_git_as_a_directory() {
        let project = temp_project();
        // A real worktree has a `.git` file; a plain clone dropped in the
        // container has a `.git` directory. Both count.
        worktree_dir(&project, "wt-file");
        fs::create_dir_all(project.join("wt-dir").join(".git")).unwrap();

        let names: Vec<String> = enumerate(&config_for(&project))
            .into_iter()
            .map(|e| e.name)
            .collect();

        assert_eq!(names, vec!["wt-dir", "wt-file"]);
        let _ = fs::remove_dir_all(&project);
    }

    #[test]
    fn enumerate_uses_the_provider_and_ignores_the_project_root() {
        let project = temp_project();
        // A project-root child must not leak into an external provider's list:
        // mixing layouts would make the board poll a worktree the provider
        // does not manage.
        worktree_dir(&project, "bork-9-stale");
        // The provider command runs from the main checkout.
        fs::create_dir_all(project.join("main")).unwrap();

        let config = AppConfig {
            worktree_create_cmd: Some("true".into()),
            worktree_list_cmd: Some(
                r#"printf '{"name":"wt--pool-a","path":"/elsewhere/wt--pool-a"}\n'"#.into(),
            ),
            ..config_for(&project)
        };

        assert_eq!(
            enumerate(&config),
            vec![WorktreeEntry {
                name: "wt--pool-a".into(),
                path: PathBuf::from("/elsewhere/wt--pool-a"),
            }]
        );
        let _ = fs::remove_dir_all(&project);
    }

    #[test]
    fn path_index_maps_names_to_paths() {
        let project = temp_project();
        worktree_dir(&project, "bork-1-first");

        let index = path_index(&config_for(&project)).expect("a plain scan cannot fail");

        assert_eq!(
            index.get("bork-1-first"),
            Some(&project.join("bork-1-first"))
        );
        assert_eq!(index.len(), 1);
        let _ = fs::remove_dir_all(&project);
    }

    #[test]
    fn enumerate_is_empty_for_a_missing_project_root() {
        let config = config_for(std::path::Path::new("/nonexistent/bork-project"));

        assert!(enumerate(&config).is_empty());
    }
}
