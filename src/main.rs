// This is a GUI app: on Windows, don't flash a console window behind it.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

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

    // Only one window at a time: a second launch toggles the running instance
    // and exits instead of opening another window.
    if !single_instance() {
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

/// Prevents more than one repo-zoo instance (and therefore more than one
/// window) from running at a time.
///
/// On Windows a named mutex is created at startup; if it already exists a
/// previous instance is running, so we ask it to toggle (the same effect as
/// `repo-zoo --toggle`) and exit. The handle is intentionally leaked so the
/// mutex stays held for the process lifetime and is released by the OS on
/// exit. Other platforms get no single-instance guard.
#[cfg(target_os = "windows")]
fn single_instance() -> bool {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = OsString::from("repo-zoo-single-instance")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // Safe: `name` is a valid null-terminated wide string and the attributes
    // pointer is null (default security).
    let handle = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
    if handle == 0 {
        // The mutex couldn't be created (permissions, exotic session) — let
        // the instance run rather than blocking the user.
        return true;
    }
    // Safe: GetLastError takes no arguments.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        let _ = crate::hotkey::toggle_active_instance();
        return false;
    }
    // Keep the handle alive: it is never closed, so the mutex stays held for
    // the process lifetime and is released by the OS on exit.
    let _ = handle;
    true
}

#[cfg(not(target_os = "windows"))]
fn single_instance() -> bool {
    true
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
