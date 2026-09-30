//! The single definition of "which worktrees exist": every child directory of
//! the project root holding a `.git`, except `main/` and dotdirs.

use std::path::PathBuf;

use crate::config::AppConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub name: String,
    pub path: PathBuf,
}

/// Every worktree currently on disk, sorted by name.
pub fn enumerate(config: &AppConfig) -> Vec<WorktreeEntry> {
    let mut entries = scan_project_root(config);

    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

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
    fn enumerate_is_empty_for_a_missing_project_root() {
        let config = config_for(std::path::Path::new("/nonexistent/bork-project"));

        assert!(enumerate(&config).is_empty());
    }
}
