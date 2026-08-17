use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::project::{default_roots, expand_tilde, scan};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenMode {
    #[default]
    Editor,
    Terminal,
    Manager,
}

/// Which view the launcher starts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DefaultView {
    #[default]
    Graph,
    List,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepoSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_roots")]
    pub roots: Vec<PathBuf>,
    #[serde(default = "default_depth")]
    pub depth: u32,
    #[serde(default)]
    pub open_mode: OpenMode,
    #[serde(default = "default_editor")]
    pub editor: String,
    #[serde(default)]
    pub terminal: String,
    #[serde(default)]
    pub default_view: DefaultView,
    #[serde(default)]
    pub max_row_width: u32,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    #[serde(default)]
    pub tray: bool,
    #[serde(default)]
    pub repos: BTreeMap<String, RepoSettings>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            roots: default_roots(),
            depth: default_depth(),
            open_mode: OpenMode::default(),
            editor: default_editor(),
            terminal: String::new(),
            default_view: DefaultView::default(),
            max_row_width: 0,
            hotkey: default_hotkey(),
            tray: false,
            repos: BTreeMap::new(),
        }
    }
}

fn default_hotkey() -> String {
    "super+f".to_string()
}

fn default_depth() -> u32 {
    1
}

fn default_editor() -> String {
    "code".to_string()
}

impl Config {
    /// Path to the config file on disk.
    pub fn path() -> PathBuf {
        config_path()
    }

    /// Loads the config from disk, or seeds one from a filesystem scan on the
    /// first run. After the first run, the config file is the only source of
    /// truth for repos: no scanning happens when it already lists repos.
    pub fn load() -> Self {
        let path = config_path();
        let (mut config, parse_failed) = match fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str(&contents) {
                Ok(config) => (config, false),
                Err(err) => {
                    eprintln!("repo-zoo: invalid config at {}: {err}", path.display());
                    (Config::default(), true)
                }
            },
            Err(_) => (Config::default(), false),
        };

        // Seed on the first run: when the file is missing, or when it exists
        // but lists no repos yet. Never reseed after that — and never touch a
        // config we failed to parse, so a hand-edit typo isn't clobbered.
        if !parse_failed && config.repos.is_empty() {
            config.seed_from_scan();
            config.save();
        }

        config
    }

    /// Reloads the config from disk. Unlike [`Config::load`], it never writes
    /// or reseeds the file.
    pub fn reload() -> Self {
        let path = config_path();
        fs::read_to_string(&path)
            .ok()
            .and_then(|contents| toml::from_str(&contents).ok())
            .unwrap_or_default()
    }

    /// Populates `repos` from a one-time scan of the configured roots. Each
    /// discovered repo is recorded with its absolute path and, when
    /// discoverable, its git remote URL.
    fn seed_from_scan(&mut self) {
        let roots = self.resolved_roots();
        let depth = self.depth;
        for repo in scan(roots, depth) {
            let settings = RepoSettings {
                path: Some(repo.path.to_string_lossy().into_owned()),
                remote: repo.remote.clone(),
                depends_on: Vec::new(),
                ..Default::default()
            };
            self.repos.insert(repo.name, settings);
        }
    }

    fn save(&self) {
        let path = config_path();
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(contents) = toml::to_string(self) {
            let _ = fs::write(&path, contents);
        }
    }

    pub fn resolved_roots(&self) -> Vec<PathBuf> {
        self.roots.iter().map(|r| expand_tilde(r)).collect()
    }
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("repo-zoo")
        .join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn homedir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("repo-zoo-config-test-{name}"))
    }

    #[test]
    fn seeds_repos_from_scan_when_empty() {
        let root = homedir("seed");
        let code = root.join("code");
        fs::create_dir_all(code.join("alpha/.git")).unwrap();
        fs::create_dir_all(code.join("beta")).unwrap();

        let mut config = Config {
            roots: vec![code],
            depth: 1,
            ..Default::default()
        };
        config.seed_from_scan();

        assert!(config.repos.contains_key("alpha"));
        assert!(config.repos.contains_key("beta"));
        assert_eq!(
            config.repos["alpha"].path.as_deref(),
            Some(root.join("code").join("alpha").to_str().unwrap()),
            "seeded path must be the absolute path of the scanned repo"
        );
        assert!(config.repos["beta"].remote.is_none());
    }

    #[test]
    fn defaults_are_graph_view_super_z_hotkey() {
        let config = Config::default();
        assert_eq!(config.hotkey, "super+f");
        assert_eq!(config.default_view, DefaultView::Graph);
        assert!(config.terminal.is_empty());
    }

    #[test]
    fn parses_default_view_from_toml() {
        let parsed: Config = toml::from_str("default_view = \"list\"").unwrap();
        assert_eq!(parsed.default_view, DefaultView::List);

        let parsed: Config = toml::from_str("default_view = \"graph\"").unwrap();
        assert_eq!(parsed.default_view, DefaultView::Graph);
    }

    #[test]
    fn parses_per_repo_editor_and_terminal_overrides() {
        let parsed: Config = toml::from_str(
            r#"
[repos.web]
path = "~/code/web"
editor = "nvim"
terminal = "kitty --directory {dir}"

[repos.cli]
path = "~/code/cli"
"#,
        )
        .unwrap();

        assert_eq!(parsed.repos["web"].editor.as_deref(), Some("nvim"));
        assert_eq!(
            parsed.repos["web"].terminal.as_deref(),
            Some("kitty --directory {dir}")
        );
        assert!(parsed.repos["cli"].editor.is_none());
        assert!(parsed.repos["cli"].terminal.is_none());
    }
}
