use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub name: String,
    pub path: PathBuf,
    pub path_known: bool,
    pub remote: Option<String>,
    pub kind: Kind,
    pub editor: Option<String>,
    pub terminal: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Vcs,
    Dir,
}

const VCS_MARKERS: [&str; 3] = [".git", ".hg", ".svn"];
const SKIPPED_DIRS: [&str; 6] = [
    "node_modules",
    "target",
    "dist",
    "build",
    ".idea",
    "__pycache__",
];

pub fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let (Some(rest), Some(home)) = (s.strip_prefix("~/"), dirs::home_dir()) {
        return home.join(rest);
    }
    path.to_path_buf()
}

pub fn default_roots() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let code = home.join("code");
    if code.is_dir() {
        vec![code]
    } else {
        vec![home]
    }
}

pub fn scan(roots: Vec<PathBuf>, max_depth: u32) -> Vec<Repo> {
    let mut repos: Vec<Repo> = Vec::new();

    for root in roots {
        let root = expand_tilde(&root);
        if !root.is_dir() {
            continue;
        }
        scan_dir(&root, 1, max_depth, &mut repos);
    }

    repos.sort_by_key(|p| p.name.to_lowercase());
    repos
}

fn scan_dir(dir: &Path, level: u32, max_depth: u32, out: &mut Vec<Repo>) {
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };

    let mut subdirs: Vec<PathBuf> = Vec::new();
    let mut direct_children: Vec<PathBuf> = Vec::new();

    for entry in read_dir.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().into_owned();
        if is_skippable(&name) {
            continue;
        }

        let path = entry.path();
        if has_vcs_marker(&path) {
            out.push(repo_from_path(&name, &path, Kind::Vcs));
        }

        subdirs.push(path.clone());
        if level == 1 {
            direct_children.push(path);
        }
    }

    if level == 1 {
        for path in direct_children {
            if !has_vcs_marker(&path) {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                out.push(repo_from_path(&name, &path, Kind::Dir));
            }
        }
    }

    if level < max_depth {
        for sub in subdirs {
            scan_dir(&sub, level + 1, max_depth, out);
        }
    }
}

fn repo_from_path(name: &str, path: &Path, kind: Kind) -> Repo {
    let remote = detect_remote(path);
    Repo {
        name: name.to_string(),
        path: path.to_path_buf(),
        path_known: true,
        remote,
        kind,
        editor: None,
        terminal: None,
    }
}

pub fn detect_remote(dir: &Path) -> Option<String> {
    if !dir.join(".git").exists() {
        return None;
    }
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let remote = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if remote.is_empty() {
        None
    } else {
        Some(remote)
    }
}

fn has_vcs_marker(dir: &Path) -> bool {
    VCS_MARKERS.iter().any(|m| dir.join(m).exists())
}

fn is_skippable(name: &str) -> bool {
    name.starts_with('.') || SKIPPED_DIRS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("repo-zoo-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lists_direct_children_and_vcs_markers() {
        let root = fixture("scan-basic");
        fs::create_dir_all(root.join("proj-a")).unwrap();
        fs::create_dir_all(root.join("proj-b/.git")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::create_dir_all(root.join("node_modules")).unwrap();

        let repos = scan(vec![root.clone()], 1);

        let mut names: Vec<_> = repos.iter().map(|p| p.name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["proj-a", "proj-b"]);

        let b = repos.iter().find(|p| p.name == "proj-b").unwrap();
        assert_eq!(b.kind, Kind::Vcs);
        assert!(b.path_known);
        let a = repos.iter().find(|p| p.name == "proj-a").unwrap();
        assert_eq!(a.kind, Kind::Dir);
    }

    #[test]
    fn respects_max_depth_for_nested_vcs() {
        let root = fixture("scan-depth");
        fs::create_dir_all(root.join("outer/nested/.git")).unwrap();

        let repos = scan(vec![root.clone()], 1);
        assert!(repos.iter().all(|p| p.name != "nested"));

        let repos = scan(vec![root], 2);
        assert!(repos.iter().any(|p| p.name == "nested"));
    }

    #[test]
    fn skips_missing_roots() {
        let root = std::env::temp_dir().join("repo-zoo-does-not-exist-xyz");
        let repos = scan(vec![root], 1);
        assert!(repos.is_empty());
    }

    #[test]
    fn expands_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_tilde(Path::new("~/code")), home.join("code"));
        assert_eq!(
            expand_tilde(Path::new("/abs/path")),
            PathBuf::from("/abs/path")
        );
    }

    #[test]
    fn detects_remote_from_git() {
        let root = fixture("git-remote");
        std::process::Command::new("git")
            .args(["init", "-q", "."])
            .current_dir(&root)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/example/demo.git",
            ])
            .current_dir(&root)
            .status()
            .unwrap();

        assert_eq!(
            detect_remote(&root).as_deref(),
            Some("https://github.com/example/demo.git")
        );
    }
}
