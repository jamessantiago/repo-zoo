mod app;
mod config;
mod geometry;
mod graph;
mod graph_view;
mod hotkey;
mod kde;
mod launch;
mod project;
mod tray;

use iced::Theme;

fn main() -> iced::Result {
    // `repo-zoo --toggle` asks the running instance to flip window visibility,
    // then exits. On Plasma that is a D-Bus call to the KWin bridge (see
    // `kde`); on Windows it posts a message to the running instance's hotkey
    // window (see `hotkey::win`).
    if std::env::args().any(|arg| arg == "--toggle") {
        let toggle = {
            #[cfg(target_os = "windows")]
            {
                crate::hotkey::toggle_active_instance()
            }
            #[cfg(not(target_os = "windows"))]
            {
                crate::kde::toggle_active_instance()
            }
        };
        if let Err(err) = toggle {
            eprintln!("repo-zoo: toggle failed: {err}");
        }
        return Ok(());
    }

    prefer_x11_positioning();

    // When a tray icon is enabled the window hides to the tray on close
    // instead of quitting, so iced must not close it behind our back.
    let config = config::Config::reload();
    let mut window = geometry::window_settings();
    window.exit_on_close_request = !config.tray;

    // On Plasma the global hotkey is owned by KWin (a native compositor grab),
    // installed as a small script that calls back over D-Bus.
    if crate::kde::is_plasma() {
        if config.hotkey.trim().is_empty() {
            if let Err(err) = crate::kde::uninstall() {
                eprintln!("repo-zoo: KDE hotkey uninstall failed: {err}");
            }
        } else if let Err(err) = crate::kde::install(&config) {
            eprintln!("repo-zoo: KDE hotkey install failed: {err}");
        }
    }

    iced::application(app::App::boot, app::update, app::view)
        .title("repo-zoo")
        .window(window)
        .theme(Theme::KanagawaDragon)
        .subscription(app::subscription)
        .run()
}

/// On Wayland sessions that also have an X server (e.g. KDE's XWayland), video
/// over X11 so the compositor honors the requested window geometry. Native
/// Wayland compositors always place toplevel windows themselves and ignore
/// client positions, which would leave the launcher centered instead of docked
/// above the bottom toolbar.
fn prefer_x11_positioning() {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var_os("REPO_ZOO_NATIVE_WAYLAND").is_none();
    let x_available = std::env::var_os("DISPLAY").is_some();
    if wayland && x_available {
        // Safe: main thread, before iced spawns any threads or the event loop.
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }
}
