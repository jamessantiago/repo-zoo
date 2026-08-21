//! System tray icon with a small context menu.
//!
//! iced has no tray support, so the icon is created from a dedicated thread
//! with [`tray-icon`](https://crates.io/crates/tray-icon). On Linux that means
//! owning a GTK main loop on that thread (sunken under the StatusNotifier
//! protocol); menu selections are forwarded to the application through an iced
//! [`Subscription`]. Tray failures are non-fatal: the window simply keeps
//! working without an icon.

use std::time::Duration;

use iced::Subscription;
use iced::futures::Stream;
use iced::futures::channel::mpsc;

use crate::app::View;
use crate::config::Config;

/// Events produced by the tray icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Show or hide the launcher window.
    Toggle,
    /// Switch the launcher to a specific view (graph or list).
    View(View),
    /// Exit the application.
    Quit,
}

const TOGGLE_ID: &str = "repo-zoo-toggle";
const VIEW_ID: &str = "repo-zoo-view";
const VIEW_GRAPH_ID: &str = "repo-zoo-view-graph";
const VIEW_LIST_ID: &str = "repo-zoo-view-list";
const QUIT_ID: &str = "repo-zoo-quit";

/// Returns a subscription that forwards tray menu selections. Empty when the
/// tray is disabled in the config.
pub fn subscription(config: &Config) -> Subscription<Event> {
    if !config.tray {
        return Subscription::none();
    }
    Subscription::run_with(config.tray, |_| stream())
}

fn stream() -> impl Stream<Item = Event> {
    let (tx, rx) = mpsc::unbounded();
    std::thread::spawn(move || {
        if let Err(err) = run(tx) {
            eprintln!("repo-zoo: system tray unavailable: {err}");
        }
    });
    rx
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn run(tx: mpsc::UnboundedSender<Event>) -> Result<(), Box<dyn std::error::Error>> {
    use tray_icon::TrayIconBuilder;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, Submenu};

    // The tray window's messages must be pumped on this thread: on Linux by a
    // GTK main loop and on Windows by a win32 message loop. Without them the
    // hidden tray window never receives click callbacks, so the context menu
    // never shows.
    #[cfg(target_os = "linux")]
    let _ = gtk::init();

    let toggle = MenuItem::with_id(TOGGLE_ID, "Show/Hide repo-zoo", true, None);
    let graph = MenuItem::with_id(VIEW_GRAPH_ID, "Graph", true, None);
    let list = MenuItem::with_id(VIEW_LIST_ID, "List", true, None);
    let view = Submenu::with_id(VIEW_ID, "View", true);
    view.append_items(&[&graph, &list])?;
    let quit = MenuItem::with_id(QUIT_ID, "Exit", true, None);

    let menu = Menu::new();
    menu.append(&toggle)?;
    menu.append(&view)?;
    menu.append(&quit)?;

    let icon = tray_icon();

    let _tray = TrayIconBuilder::new()
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .with_tooltip("repo-zoo")
        .build()?;

    let menu_rx = MenuEvent::receiver();
    let tray_rx = tray_icon::TrayIconEvent::receiver();

    loop {
        pump_messages();
        while let Ok(event) = menu_rx.try_recv() {
            let forward = match event.id().as_ref() {
                TOGGLE_ID => Event::Toggle,
                VIEW_GRAPH_ID => Event::View(View::Graph),
                VIEW_LIST_ID => Event::View(View::List),
                QUIT_ID => Event::Quit,
                _ => continue,
            };
            if tx.unbounded_send(forward).is_err() {
                return Ok(());
            }
        }
        while tray_rx.try_recv().is_ok() {}

        #[cfg(target_os = "linux")]
        {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
        }

        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Pumps the win32 message queue so the hidden tray window receives the
/// shell's click callbacks (`WM_USER_TRAYICON`) and `TrackPopupMenu` runs.
#[cfg(target_os = "windows")]
fn pump_messages() {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };

    let mut msg = MSG {
        hwnd: 0,
        message: 0,
        wParam: 0,
        lParam: 0,
        time: 0,
        pt: POINT { x: 0, y: 0 },
    };
    while unsafe { PeekMessageW(&mut msg, 0, 0, 0, PM_REMOVE) } != 0 {
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn pump_messages() {}

/// 22x22 tray icon downscaled from the bundled 256px PNG. The tray size is
/// what Windows and most Linux panels request; the icon crate scales it.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn tray_icon() -> tray_icon::Icon {
    const TRAY_SIZE: u32 = 22;

    let source = image::load_from_memory(include_bytes!("../packaging/repo-zoo.png"))
        .expect("bundled icon must decode")
        .to_rgba8();
    let scaled = image::imageops::resize(
        &source,
        TRAY_SIZE,
        TRAY_SIZE,
        image::imageops::FilterType::Lanczos3,
    );
    tray_icon::Icon::from_rgba(scaled.into_raw(), TRAY_SIZE, TRAY_SIZE)
        .expect("tray icon must be a valid RGBA buffer")
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn run(tx: mpsc::UnboundedSender<Event>) -> Result<(), Box<dyn std::error::Error>> {
    let _ = tx;
    Err("system tray is not supported on this platform yet".into())
}
