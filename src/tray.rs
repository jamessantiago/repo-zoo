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
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{Icon, TrayIconBuilder};

    // On Linux the appindicator menu and clicks are dispatched by a GTK main
    // loop owned on this thread. Windows needs no such pump: tray-icon runs
    // its own message loop behind the receiver channels.
    #[cfg(target_os = "linux")]
    let _ = gtk::init();

    let toggle = MenuItem::with_id(TOGGLE_ID, "Show/Hide repo-zoo", true, None);
    let quit = MenuItem::with_id(QUIT_ID, "Quit", true, None);

    let menu = Menu::with_items(&[&toggle, &quit])?;

    let icon = Icon::from_rgba(tray_icon_pixels(), 22, 22)?;

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

/// 22x22 RGBA icon: a filled disc in the app's accent colour.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn tray_icon_pixels() -> Vec<u8> {
    const SIZE: u32 = 22;
    let (r, g, b) = (0.60, 0.55, 0.90);
    let (r, g, b) = ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8);
    let radius = (SIZE as f32 / 2.0) - 3.0;
    let center = SIZE as f32 / 2.0;

    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            if dx * dx + dy * dy <= radius * radius {
                pixels.extend_from_slice(&[r, g, b, 255]);
            } else {
                pixels.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    pixels
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn run(tx: mpsc::UnboundedSender<Event>) -> Result<(), Box<dyn std::error::Error>> {
    let _ = tx;
    Err("system tray is not supported on this platform yet".into())
}
