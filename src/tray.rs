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

use crate::config::Config;

/// Events produced by the tray icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Show or hide the launcher window.
    Toggle,
    /// Exit the application.
    Quit,
}

const TOGGLE_ID: &str = "repo-zoo-toggle";
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
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};

    // On Linux the appindicator menu and clicks are dispatched by a GTK main
    // loop owned on this thread. Windows needs no such pump: tray-icon runs
    // its own message loop behind the receiver channels.
    #[cfg(target_os = "linux")]
    let _ = gtk::init();

    let toggle = MenuItem::with_id(TOGGLE_ID, "Show/Hide repo-zoo", true, None);
    let quit = MenuItem::with_id(QUIT_ID, "Quit", true, None);

    let menu = Menu::with_items(&[&toggle, &quit])?;

    let icon = tray_icon();

    let _tray = TrayIconBuilder::new()
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .with_tooltip("repo-zoo")
        .build()?;

    let menu_rx = MenuEvent::receiver();
    let tray_rx = tray_icon::TrayIconEvent::receiver();

    loop {
        while let Ok(event) = menu_rx.try_recv() {
            let forward = match event.id().as_ref() {
                TOGGLE_ID => Event::Toggle,
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
