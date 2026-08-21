use std::path::Path;
use std::process::Command;

use crate::config::{Config, OpenMode};
use crate::project::Repo;

#[derive(Debug, Clone)]
struct ResolvedCommand {
    program: String,
    args: String,
}

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
            #[cfg(target_os = "windows")]
            {
                // Windows: a configured solution file is handed to its system
                // association (Visual Studio et al.) rather than to the
                // configured editor.
                if let Some(sln) = repo
                    .sln
                    .as_deref()
                    .filter(|sln| !sln.as_os_str().is_empty() && sln.exists())
                {
                    win::open_with_association(&sln.to_string_lossy())?;
                    return Ok(format!(
                        "opened `{}` via its default app ({name})",
                        sln.display()
                    ));
                }
            }

            // What gets handed to the editor: on Windows the project directory,
            // elsewhere the solution file when configured (editors like
            // devenv / VS Code open a `.sln` directly).
            let target: &std::path::Path = {
                #[cfg(target_os = "windows")]
                {
                    dir
                }
                #[cfg(not(target_os = "windows"))]
                {
                    repo.sln
                        .as_deref()
                        .filter(|sln| !sln.as_os_str().is_empty() && sln.exists())
                        .unwrap_or(dir)
                }
            };
            if let Some(editor_cmd) = editor_command(editor) {
                if spawn_editor(&editor_cmd, target).is_ok() {
                    return Ok(format!(
                        "opened `{}` in {editor} ({name})",
                        target.display()
                    ));
                }
                return Err(format!(
                    "failed to launch editor `{editor}` for `{}` ({name})",
                    target.display()
                ));
            }
            Err(format!(
                "no editor found for `{}` — install one or set `editor` in the config ({name})",
                target.display()
            ))
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
    let Some(editor_cmd) = editor_command(&editor) else {
        return Err("no editor found: install VS Code or set `editor` in the config".to_string());
    };
    spawn_editor(&editor_cmd, &path)?;
    Ok(format!("opened config in {editor}"))
}

/// Launches the resolved editor command with `arg` as the target path.
fn spawn_editor(editor: &ResolvedCommand, arg: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        // ShellExecuteW launches the editor the way Explorer would: it resolves
        // real executables and `.cmd`/`.bat` shims (VS Code's `code.cmd`) alike,
        // detaches, and shows no console window.
        let args = build_editor_args_windows(&editor.args, arg);
        let args = if args.trim().is_empty() {
            None
        } else {
            Some(args)
        };
        win::run(&editor.program, args.as_deref())?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let args = build_editor_args_unix(&editor.args, arg);
        let mut command = shell_quote(&editor.program);
        if !args.trim().is_empty() {
            command.push(' ');
            command.push_str(&args);
        }
        Command::new("sh")
            .arg("-c")
            .arg(command)
            .spawn()
            .map_err(|err| format!("failed to launch {}: {err}", editor.program))?;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn build_editor_args_windows(configured_args: &str, target: &Path) -> String {
    let quoted = windows_quote_arg(target);
    let configured = configured_args.trim();
    if configured.is_empty() {
        return quoted;
    }
    let expanded = configured
        .replace("{target}", &quoted)
        .replace("{file}", &quoted)
        .replace("{dir}", &quoted);
    if expanded == configured {
        format!("{configured} {quoted}")
    } else {
        expanded
    }
}

#[cfg(not(target_os = "windows"))]
fn build_editor_args_unix(configured_args: &str, target: &Path) -> String {
    let quoted = shell_quote(&target.display().to_string());
    let configured = configured_args.trim();
    if configured.is_empty() {
        return quoted;
    }
    let expanded = configured
        .replace("{target}", &quoted)
        .replace("{file}", &quoted)
        .replace("{dir}", &quoted);
    if expanded == configured {
        format!("{configured} {quoted}")
    } else {
        expanded
    }
}

/// Splits a command line into the first token and the raw argument tail.
///
/// Supports single- and double-quoted first tokens so paths like
/// `"C:\\Program Files\\Foo\\foo.exe" --flag` are handled correctly.
fn split_command(command: &str) -> Option<(String, String)> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }

    let chars: Vec<char> = command.chars().collect();
    let mut idx = 0;
    let mut first = String::new();

    if matches!(chars.first(), Some('"' | '\'')) {
        let quote = chars[0];
        idx = 1;
        while idx < chars.len() && chars[idx] != quote {
            first.push(chars[idx]);
            idx += 1;
        }
        if idx >= chars.len() || chars[idx] != quote {
            return None;
        }
        idx += 1;
    } else {
        while idx < chars.len() && !chars[idx].is_whitespace() {
            first.push(chars[idx]);
            idx += 1;
        }
    }

    if first.is_empty() {
        return None;
    }

    while idx < chars.len() && chars[idx].is_whitespace() {
        idx += 1;
    }

    let args = chars[idx..].iter().collect::<String>();
    Some((first, args))
}

/// Resolves the configured editor command to an executable path plus the
/// original argument tail. The executable is validated up front so callers can
/// emit a clear error instead of silently doing nothing.
fn editor_command(configured: &str) -> Option<ResolvedCommand> {
    let (first, args) = split_command(configured)?;

    let program = resolve_executable(&first).or_else(|| {
        #[cfg(target_os = "windows")]
        {
            vscode_command(&first)
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    })?;

    Some(ResolvedCommand {
        program: program.to_string_lossy().into_owned(),
        args,
    })
}

/// Common VS Code install locations, most likely first.
#[cfg(target_os = "windows")]
fn vscode_locations(insiders: bool) -> Vec<std::path::PathBuf> {
    let mut locations = Vec::new();
    for env in ["LOCALAPPDATA", "ProgramFiles"] {
        let Some(base) = std::env::var_os(env) else {
            continue;
        };
        let base = std::path::PathBuf::from(base);
        if insiders {
            locations.push(base.join("Programs/Microsoft VS Code Insiders/Code - Insiders.exe"));
        } else {
            locations.push(base.join("Programs/Microsoft VS Code/Code.exe"));
        }
    }
    locations
}

#[cfg(target_os = "windows")]
fn vscode_command(name: &str) -> Option<std::path::PathBuf> {
    let stem = std::path::Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())?;
    let insiders = if stem.eq_ignore_ascii_case("code-insiders") {
        true
    } else if stem.eq_ignore_ascii_case("code") {
        false
    } else {
        return None;
    };

    vscode_locations(insiders)
        .into_iter()
        .find(|location| location.is_file())
}

/// Resolves `bin` (a bare program name or a path) to an existing file on disk:
/// as-is when a path was given, otherwise a PATH search, then a few common
/// non-PATH install locations (flatpak/snap exports, `~/.local/bin`, …).
/// Returns `None` when the program isn't installed anywhere we'd look, so
/// callers can fail with a clear message instead of silently doing nothing.
fn resolve_executable(bin: &str) -> Option<std::path::PathBuf> {
    let bin = bin.trim();
    if bin.is_empty() {
        return None;
    }
    let candidate = Path::new(bin);
    if candidate.components().count() > 1 || candidate.is_absolute() {
        let expanded = crate::project::expand_tilde(candidate);
        return expanded.is_file().then_some(expanded);
    }
    executable_on_path(bin).or_else(|| {
        common_dirs()
            .iter()
            .map(|dir| dir.join(bin))
            .find(|path| path.is_file())
    })
}

/// Returns the absolute path of `bin` if it resolves on PATH. On Windows the
/// PATHEXT extensions (`.cmd`, `.exe`, …) are tried too.
fn executable_on_path(bin: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    #[cfg(target_os = "windows")]
    let extensions: Vec<String> = std::env::var("PATHEXT")
        .map(|p| p.split(';').map(|e| e.to_lowercase()).collect())
        .unwrap_or_else(|_| {
            vec![".com".into(), ".exe".into(), ".bat".into(), ".cmd".into()]
        });
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(target_os = "windows")]
        for ext in &extensions {
            let candidate = dir.join(format!("{bin}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Directories probed after PATH for programs that install outside it: flatpak
/// and snap exports, `~/.local/bin`, and the classic `/usr/local/bin`. Windows
/// needs nothing extra — PATH covers the system tools and VS Code has its own
/// dedicated probe.
fn common_dirs() -> Vec<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        Vec::new()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut dirs = Vec::new();
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".local/bin"));
            dirs.push(home.join("bin"));
        }
        dirs.push(std::path::PathBuf::from("/usr/local/bin"));
        dirs.push(std::path::PathBuf::from("/snap/bin"));
        dirs.push(std::path::PathBuf::from("/var/lib/flatpak/exports/bin"));
        dirs
    }
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
        if let Some(wt) = resolve_executable("wt")
            && Command::new(wt).args(["-d", &dir]).spawn().is_ok()
        {
            return Ok("wt".to_string());
        }
        // Prefer modern PowerShell when available.
        let ps = format!("Set-Location -LiteralPath '{}'", dir.replace('\'', "''"));
        if let Some(pwsh) = resolve_executable("pwsh")
            && Command::new(pwsh)
                .args(["-NoExit", "-Command", &ps])
                .spawn()
                .is_ok()
        {
            return Ok("pwsh".to_string());
        }
        // Then PowerShell, using its own quoting so the path is safe.
        if let Some(powershell) = resolve_executable("powershell")
            && Command::new(powershell)
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
        let cmd = resolve_executable("cmd")
            .ok_or_else(|| "failed to launch terminal: cmd not found".to_string())?;
        Command::new(cmd)
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
            // Resolve first so terminals installed outside PATH (flatpak,
            // snap, …) are found too.
            if let Some(exe) = resolve_executable(bin)
                && Command::new(&exe).args(&args).spawn().is_ok()
            {
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
/// on success; `None` when the executable can't be found (so the caller falls
/// back to auto-detection).
fn spawn_configured_terminal(template: &str, dir: &Path) -> Option<String> {
    let (first, rest) = split_command(template)?;

    #[cfg(not(target_os = "windows"))]
    {
        // Resolve the executable first: `sh -c` would happily "succeed"
        // while the configured terminal silently fails when it isn't on PATH.
        let resolved = resolve_executable(&first)?;
        let mut cmd = shell_quote(&resolved.to_string_lossy());
        let rest = rest.replace("{dir}", &shell_quote(&dir.display().to_string()));
        if !rest.trim().is_empty() {
            cmd.push(' ');
            cmd.push_str(rest.trim());
        }
        Command::new("sh").arg("-c").arg(&cmd).spawn().ok()?;
        Some(template.to_string())
    }
    #[cfg(target_os = "windows")]
    {
        // Launch the terminal the same way as the editor: resolve the
        // template's first token to a real executable (extension included) and
        // run it through ShellExecuteExW — the mechanism Explorer uses, so
        // console wrapper apps like `wezterm` start reliably with no hidden
        // console. `{dir}` is quoted for the command line.
        win_launch(template, dir).ok()?;
        Some(template.to_string())
    }
}

/// Quotes a path for use inside a cmd command line, doubling embedded quotes
/// so it can be safely embedded as a single argument.
#[cfg(target_os = "windows")]
fn windows_quote_arg(path: &Path) -> String {
    format!("\"{}\"", path.to_string_lossy().replace('"', "\"\""))
}

/// Launches a configured command line on Windows, e.g.
/// `wezterm start --cwd {dir}`. The first whitespace token is resolved to a
/// real executable with [`resolve_executable`] (extension included), `{dir}`
/// is replaced with the repo path, and the whole thing runs through
/// ShellExecuteExW — matching how the editor is launched and avoiding cmd's
/// quote mangling and CreateProcessW's inability to start scripts.
#[cfg(target_os = "windows")]
fn win_launch(template: &str, dir: &Path) -> Result<(), String> {
    let (first, rest) = split_command(template).ok_or_else(|| "empty command".to_string())?;
    let exe = resolve_executable(&first)
        .ok_or_else(|| format!("`{first}` not found on PATH"))?
        .to_string_lossy()
        .into_owned();
    let rest = rest.replace("{dir}", &windows_quote_arg(dir));
    if rest.is_empty() {
        win::run(&exe, None)
    } else {
        win::run(&exe, Some(&rest))
    }
}

#[cfg(not(target_os = "windows"))]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_command_handles_quoted_program() {
        let parsed = split_command("\"C:/Program Files/Code/Code.exe\" --reuse-window");
        let (program, args) = parsed.expect("quoted command should parse");
        assert_eq!(program, "C:/Program Files/Code/Code.exe");
        assert_eq!(args, "--reuse-window");
    }

    #[test]
    fn split_command_handles_unquoted_program() {
        let parsed = split_command("code --reuse-window");
        let (program, args) = parsed.expect("command should parse");
        assert_eq!(program, "code");
        assert_eq!(args, "--reuse-window");
    }

    #[test]
    fn split_command_rejects_unclosed_quote() {
        assert!(split_command("\"C:/Program Files/Code/Code.exe --reuse-window").is_none());
    }
}

/// Windows-native process launching. `std::process::Command` (and a raw
/// CreateProcessW command line) mangling cmd scripts is avoided entirely by
/// going through ShellExecuteW, which resolves executables and `.cmd`/`.bat`
/// scripts exactly the way Explorer does.
#[cfg(target_os = "windows")]
mod win {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsString::from(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// Opens a file through its system association (e.g. a `.sln` in Visual
    /// Studio), exactly as if it had been double-clicked in Explorer. Detached,
    /// no console window.
    pub fn open_with_association(path: &str) -> Result<(), String> {
        let path_w = wide(path);
        let verb = wide("open");
        // Safe: all pointers reference valid null-terminated wide strings.
        let result = unsafe {
            ShellExecuteW(
                0,
                verb.as_ptr(),
                path_w.as_ptr(),
                ptr::null(),
                ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if result <= 32 {
            return Err(format!(
                "failed to open `{path}`: ShellExecute error {result}"
            ));
        }
        Ok(())
    }

    /// Launches `program` (an executable, `.cmd`/`.bat` script or `.lnk`
    /// shortcut) with optional arguments the way Explorer does. Detached, no
    /// console window. Uses the default verb ("open").
    pub fn run(program: &str, args: Option<&str>) -> Result<(), String> {
        let program_w = wide(program);
        let args_w = args.map(wide);

        // Safe: all pointers reference valid null-terminated wide strings.
        let result = unsafe {
            ShellExecuteW(
                0,
                ptr::null(), // default verb ("open")
                program_w.as_ptr(),
                args_w
                    .as_ref()
                    .map_or(ptr::null(), |args| args.as_ptr()),
                ptr::null(), // working directory
                SW_SHOWNORMAL,
            )
        };

        // ShellExecuteW returns a value > 32 on success.
        if result <= 32 {
            return Err(format!(
                "failed to launch `{program}`: ShellExecute error {result}"
            ));
        }
        Ok(())
    }
}
