# repo-zoo

A fast, keyboard-driven launcher for your code projects. It scans your code
directories once, seeds a TOML config, and lets you open any project in an
editor, a terminal, or a file manager — from a searchable list or an
interactive dependency graph. Runs on Linux (KDE Plasma first-class) and
Windows.

## Features

- **Graph and list views** — browse projects as a dependency graph or a flat,
  filterable list.
- **Instant search** — `Ctrl+F` focuses the search box; type to filter.
- **Open anywhere** — per-project editor/terminal/file-manager actions, either
  from the node/list icons or the configurable default mode.
- **Git-aware** — projects with a remote but no local checkout are shown as
  clone candidates; one click runs `git clone` and re-scans.
- **Global hotkey** — toggle the window from anywhere: native KWin grab on
  Plasma, X11 grab elsewhere, `RegisterHotKey` on Windows.
- **System tray** — optional; when enabled, closing the window hides it to the
  tray instead of quitting.
- **Icon everywhere** — the window, tray, and (on Windows) the executable and
  installer all use the bundled repo-zoo icon.
- **Config is just TOML** — one file, hand-editable, re-read on demand.

## Install

### Linux

```sh
make install            # builds release, installs to ~/.local
make uninstall
```

`./scripts/install.sh --system` installs to `/usr/local` instead. A `.desktop`
entry and an icon are installed so repo-zoo shows up in your application menu.
Launch it with `repo-zoo` or from the menu.

### Windows

```powershell
powershell -ExecutionPolicy Bypass -File scripts\install.ps1
powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Uninstall
```

The script builds a release binary and installs it to
`%LOCALAPPDATA%\Programs\repo-zoo` with Start Menu and desktop shortcuts. For a
proper setup wizard, compile `windows\repo-zoo.iss` with [Inno Setup] (on
Windows) or `windows\repo-zoo.nsi` with [NSIS] — the NSIS script compiles to a
`setup.exe` from Linux too (`make win-setup`, requires `makensis`).

Cross-compiling the Windows binary from Linux is supported too:

```sh
cargo build --release --target x86_64-pc-windows-gnu
```

(The mingw-w64 linker is configured in `.cargo/config.toml`.)

## Configuration

On the first run repo-zoo scans the default root (`~/code`, falling back to the
home directory) one level deep and writes the config:

- Linux: `~/.config/repo-zoo/config.toml`
- Windows: `%APPDATA%\repo-zoo\config.toml`

After that the file is the only source of truth — no re-scanning happens unless
you ask (the `↻` button re-reads it). Click `⚙` in the header to open it in
your editor.

```toml
roots = ["~/code"]            # scan roots (default: ~/code or home)
depth = 1                     # scan depth (default: 1)
open_mode = "editor"          # default action on open: editor|terminal|manager
editor = "code"               # editor command (default: code)
terminal = ""                 # terminal template, e.g. "konsole --workdir {dir}"
default_view = "graph"        # graph|list
max_row_width = 3             # graph width in nodes; 0 = the default, larger wraps wider layers
hotkey = "super+f"            # global toggle hotkey (Windows default: ctrl+alt+z)
tray = false                  # hide to tray on close instead of quitting

[repos.myproject]
path = "~/code/myproject"     # absent path + remote present => clone candidate
remote = "git@github.com:user/myproject.git"
editor = "nvim"               # per-repo overrides (optional)
terminal = "kitty --directory {dir}"
sln = "~/code/myproject/myproject.sln"   # solution file passed to the editor instead of the dir (optional)
```

Notes:

- `{dir}` in a terminal template is replaced with the project path
  (shell-quoted on Unix). A bare command is also fine — an emulator is
  auto-detected otherwise (alacritty, kitty, gnome-terminal, konsole, foot,
  wezterm, xterm, …).
- `sln` is handy on Windows: when set and the file exists, opening the project
  in the editor launches the editor with the solution file instead of the
  directory (e.g. `start "" "C:\...\devenv.exe" "myproject.sln"`).
- A repo with a `remote` but no `path` is an external dependency until you
  clone it; the clone lands in the first configured root (or `path` if you set
  one), then the config is reloaded so it becomes openable.
- The hotkey is a `modifier+key` combo: `ctrl`, `shift`, `alt`, `super`, or
  `mod2`, plus a key like `f`, `space`, `f12`, `escape`, … Windows requires at
  least one modifier. The default is `super+f`, except on Windows where
  `ctrl+alt+z` is used instead because the system reserves most Win-key
  combinations (Win+F opens the Feedback Hub) and `RegisterHotKey` will refuse
  them.

## Usage

| Key            | Action                                  |
|----------------|-----------------------------------------|
| `Ctrl+F`       | Focus search (pre-selects the query)    |
| `↑` / `↓`      | Move selection (list or graph)          |
| `Enter`        | Open the selected project               |
| `Esc`          | Clear the search query                  |
| `↻`            | Reload the config from disk             |
| `⚙`            | Open the config file                    |
| node / row     | Click to open (default mode)            |
| `✎` `▸` `▣` `⬇` | Editor / terminal / file manager / clone |

`repo-zoo --toggle` asks a running instance to show or hide the window, then
exits — handy for binding your own key elsewhere. Environment variables:
`REPO_ZOO_VIEW=graph|list` overrides the default view, and
`REPO_ZOO_NATIVE_WAYLAND=1` keeps native Wayland positioning instead of
preferring XWayland.

## Platform support

|                       | Linux        | Windows |
|-----------------------|--------------|---------|
| Window positioning    | X11/XWayland | yes     |
| Global hotkey         | KWin (Plasma) or X11 grab | RegisterHotKey |
| System tray           | yes (AppIndicator) | yes     |
| Terminal launch       | auto-detect  | Windows Terminal → PowerShell → cmd |
| App icon              | window + tray | window + tray + exe + installer |
| `--toggle`            | D-Bus (KWin) | hotkey window message |
| Single instance       | —            | named mutex; second launch toggles the running window |

On Plasma the hotkey is registered with KWin itself (visible under System
Settings → Shortcuts → KWin), so it also works on Wayland. Other platforms
compile with tray/hotkey disabled. On Windows only one instance runs at a time:
starting repo-zoo again toggles the existing window (like `--toggle`) and
exits, instead of opening a second window.

## Build

Requires a recent Rust toolchain (edition 2024).

```sh
make build        # release build
make run          # run in debug
make test         # run the test suite
make lint         # clippy with -D warnings
make fmt          # format the code
```

## Project layout

```
src/
  app.rs         UI state, messages, keybindings, tray/hotkey wiring
  config.rs      TOML config, first-run scan seeding
  graph.rs       dependency graph + layout
  graph_view.rs  canvas rendering + hit-testing
  project.rs     filesystem scan, git remote detection
  launch.rs      open-in-editor/terminal/manager, git clone
  hotkey.rs      global hotkey (X11 + Windows RegisterHotKey)
  kde.rs         KWin scripting + D-Bus hotkey bridge
  tray.rs        system tray (Linux + Windows)
  geometry.rs    window placement, primary-display work area
scripts/         Linux (sh) and Windows (powershell) installers
packaging/       .desktop entry + SVG/PNG/ICO icons
windows/         Inno Setup + NSIS installer sources
```

[Inno Setup]: https://jrsoftware.org/isinfo.php
[NSIS]: https://nsis.sourceforge.io/