use std::path::Path;
use std::process::Command;

use crate::config::{Config, OpenMode};
use crate::project::Repo;

pub fn open_project_with_mode(
    repo: &Repo,
    config: &Config,
    mode: OpenMode,
) -> Result<String, String> {
    let dir = &repo.path;
    let name = &repo.name;

    // A repo-level override wins over the config default; empty overrides fall
    // back to the global setting.
    let editor = repo
        .editor
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&config.editor);
    let terminal = repo
        .terminal
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&config.terminal);

    match mode {
        OpenMode::Editor => {
            if !editor.trim().is_empty() && Command::new(editor).arg(dir).spawn().is_ok() {
                return Ok(format!("opened `{}` in {} ({name})", dir.display(), editor));
            }
            open_in_terminal(dir, terminal).map(|cmd| format!("opened {name} in terminal ({cmd})"))
        }
        OpenMode::Terminal => {
            open_in_terminal(dir, terminal).map(|cmd| format!("opened {name} in terminal ({cmd})"))
        }
        OpenMode::Manager => opener::open(dir)
            .map_err(|err| format!("no file manager available: {err}"))
            .map(|_| format!("opened {name} in file manager")),
    }
}

/// Clones a remote into `dest` with `git clone`.
pub fn clone_repo(remote: &str, dest: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["clone", remote])
        .arg(dest)
        .output()
        .map_err(|err| format!("failed to run `git clone`: {err}"))?;
    if output.status.success() {
        Ok(format!("cloned {remote} into {}", dest.display()))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        let detail = if stderr.is_empty() {
            "unknown error".to_string()
        } else {
            stderr.to_string()
        };
        Err(format!("`git clone` failed: {detail}"))
    }
}

/// Opens the repo-zoo config file in the configured editor.
pub fn open_config(config: &Config) -> Result<String, String> {
    let path = crate::config::Config::path();
    let editor = if config.editor.trim().is_empty() {
        "code".to_string()
    } else {
        config.editor.clone()
    };
    Command::new(&editor)
        .arg(&path)
        .spawn()
        .map_err(|err| format!("failed to open config in {editor}: {err}"))?;
    Ok(format!("opened config in {editor}"))
}

fn open_in_terminal(dir: &Path, configured: &str) -> Result<String, String> {
    if !configured.trim().is_empty() {
        // The configured `terminal` is a command template; `{dir}` is replaced
        // with the repo path. Falls back to auto-detection if it can't spawn.
        if let Some(cmd) = spawn_configured_terminal(configured, dir) {
            return Ok(cmd);
        }
    }

    #[cfg(target_os = "windows")]
    {
        let dir = dir.display().to_string();
        Command::new("cmd")
            .args(["/C", "start", "", "cmd", "/K", &format!("cd /d \"{dir}\"")])
            .spawn()
            .map_err(|err| format!("failed to launch terminal: {err}"))?;
        Ok("cmd".to_string())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let dir = dir.display().to_string();
        let candidates: Vec<(&str, Vec<String>)> = vec![
            ("alacritty", vec!["--working-directory".into(), dir.clone()]),
            ("kitty", vec!["--directory".into(), dir.clone()]),
            ("gnome-terminal", vec![format!("--working-directory={dir}")]),
            ("kgx", vec!["--working-directory".into(), dir.clone()]),
            ("konsole", vec!["--workdir".into(), dir.clone()]),
            ("foot", vec!["--working-directory".into(), dir.clone()]),
            ("wezterm", vec!["start".into(), "--cwd".into(), dir.clone()]),
            (
                "x-terminal-emulator",
                vec![
                    "-e".into(),
                    "bash".into(),
                    "-c".into(),
                    format!("cd '{dir}' && exec bash"),
                ],
            ),
            (
                "xterm",
                vec![
                    "-e".into(),
                    "bash".into(),
                    "-c".into(),
                    format!("cd '{dir}' && exec bash"),
                ],
            ),
        ];

        for (bin, args) in candidates {
            if Command::new(bin).args(&args).spawn().is_ok() {
                return Ok(bin.to_string());
            }
        }

        Err("no supported terminal emulator found".to_string())
    }
}

/// Spawns the user-configured terminal command. `{dir}` is replaced with the
/// repo path (shell-quoted on Unix), so the template can be anything from a
/// bare binary to a full command, e.g. `konsole --workdir {dir}` or
/// `xterm -e bash -c 'cd {dir} && exec bash'`. Returns the configured command
/// on success.
fn spawn_configured_terminal(template: &str, dir: &Path) -> Option<String> {
    #[cfg(not(target_os = "windows"))]
    {
        let quoted = format!("'{}'", dir.display().to_string().replace('\'', "'\\''"));
        let cmd = template.replace("{dir}", &quoted);
        Command::new("sh").arg("-c").arg(&cmd).spawn().ok()?;
        Some(template.to_string())
    }
    #[cfg(target_os = "windows")]
    {
        let cmd = template.replace("{dir}", &format!("\"{}\"", dir.display()));
        Command::new("cmd").args(["/C", &cmd]).spawn().ok()?;
        Some(template.to_string())
    }
}
