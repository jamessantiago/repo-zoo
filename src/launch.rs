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
            // The repo's solution file, when configured and present, is what
            // gets opened (e.g. devenv / VS Code open a `.sln`); otherwise the
            // project directory is passed. On Windows `code` is not always on
            // PATH even when VS Code is installed, so probe known install
            // locations first.
            let target = repo
                .sln
                .as_deref()
                .filter(|sln| !sln.as_os_str().is_empty() && sln.exists())
                .unwrap_or(dir);
            if let Some(editor) = editor_command(editor)
                && spawn_editor(&editor, target).is_ok()
            {
                let what = if target == dir {
                    dir.display().to_string()
                } else {
                    target.display().to_string()
                };
                return Ok(format!("opened `{what}` in {editor} ({name})"));
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
    let Some(editor) = editor_command(&editor) else {
        return Err("no editor found: install VS Code or set `editor` in the config".to_string());
    };
    spawn_editor(&editor, &path)?;
    Ok(format!("opened config in {editor}"))
}

/// Launches `editor` with `arg` as a detached process so it keeps running even
/// after repo-zoo exits. On Windows this shells out to `cmd /C start`, which
/// starts the program without inheriting repo-zoo's console or lifetime —
/// launching the editor this way is more reliable than a plain `spawn()` for
/// GUI editors (VS Code's `code.cmd`, devenv, …).
fn spawn_editor(editor: &str, arg: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        // Pass the paths unquoted: Rust quotes arguments containing spaces when
        // it builds the command line, whereas pre-quoting here makes it escape
        // the quotes as `\"`, which cmd reads as a backslash + token boundary —
        // `code` turned into `\code\` and `start` could not find it.
        //
        // `start` takes the window title as its first argument, hence the
        // empty string, which Rust serializes as `""`.
        Command::new("cmd")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg(editor)
            .arg(arg.to_string_lossy().into_owned())
            .spawn()
            .map_err(|err| format!("failed to launch {editor}: {err}"))?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new(editor)
            .arg(arg)
            .spawn()
            .map_err(|err| format!("failed to launch {editor}: {err}"))?;
        Ok(())
    }
}

/// Returns the editor command to spawn, or `None` when none is usable.
///
/// On Linux the configured command is used as-is. On Windows known VS Code
/// install locations are probed first (the `code`/`code.cmd` launcher is often
/// missing from PATH), then a PATH lookup; `None` means the caller should fall
/// back to the terminal.
fn editor_command(configured: &str) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        for location in vscode_locations() {
            if location.is_file() {
                return Some(location.to_string_lossy().into_owned());
            }
        }
        if configured.trim().is_empty() {
            return None;
        }
        executable_on_path(configured).then(|| configured.to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Some(configured.to_string())
    }
}

/// Common VS Code install locations, most likely first.
#[cfg(target_os = "windows")]
fn vscode_locations() -> Vec<std::path::PathBuf> {
    let mut locations = Vec::new();
    for env in ["LOCALAPPDATA", "ProgramFiles"] {
        let Some(base) = std::env::var_os(env) else {
            continue;
        };
        let base = std::path::PathBuf::from(base);
        locations.push(base.join("Programs/Microsoft VS Code/Code.exe"));
        locations.push(base.join("Programs/Microsoft VS Code Insiders/Code - Insiders.exe"));
    }
    locations
}

/// True when `bin` (possibly with a PATHEXT extension like `.cmd`) resolves on
/// PATH.
#[cfg(target_os = "windows")]
fn executable_on_path(bin: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let exts: Vec<String> = std::env::var("PATHEXT")
        .map(|p| p.split(';').map(|e| e.to_lowercase()).collect())
        .unwrap_or_default();
    let bin_lower = bin.to_lowercase();
    for dir in std::env::split_paths(&path) {
        if dir.join(bin).is_file() {
            return true;
        }
        for ext in &exts {
            if dir.join(format!("{bin_lower}{ext}")).is_file() {
                return true;
            }
        }
    }
    false
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

        // Prefer the system default terminal (Windows Terminal) when present.
        if Command::new("wt").args(["-d", &dir]).spawn().is_ok() {
            return Ok("wt".to_string());
        }
        // Then PowerShell, using its own quoting so the path is safe.
        let ps = format!("Set-Location -LiteralPath '{}'", dir.replace('\'', "''"));
        if Command::new("powershell")
            .args(["-NoExit", "-Command", &ps])
            .spawn()
            .is_ok()
        {
            return Ok("powershell".to_string());
        }
        // Classic cmd as a last resort; `/K` keeps the window open. The tokens
        // are passed separately so Rust quotes the directory when it needs to;
        // embedding `cd /d "..."` in one argument would let Rust escape the
        // quotes as `\"` and cmd would mangle them.
        Command::new("cmd")
            .args(["/K", "cd", "/d", &dir])
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
