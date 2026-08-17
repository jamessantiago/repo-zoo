//! Computes a fixed launcher window geometry: roughly 80% of the primary
//! display's height, a fixed width just large enough for three graph nodes,
//! horizontally centered, anchored just above the display's bottom edge.
//!
//! Geometry is best-effort: on Linux it is probed directly from the X server
//! (including XWayland sessions), on Windows from the primary work area. If
//! no display can be queried, iced's default placement is used. Native
//! Wayland compositors may ignore the requested window position.

use iced::window::{Position, Settings};
use iced::{Point, Size};

/// Fixed window width: a little larger than the three-node-wide canvas
/// (three cards + two gaps + padding ≈ 840) so the graph never clips.
pub const WINDOW_WIDTH: f32 = 880.0;

/// Target window height as a fraction of the display's height.
const HEIGHT_RATIO: f32 = 0.7;

/// Lower bound on the window height so it stays usable on small displays.
const MIN_HEIGHT: f32 = 400.0;

/// Vertical gap between the window's bottom edge and the display's bottom
/// edge, used when no work-area information is available.
const BOTTOM_OFFSET: f32 = 48.0;

struct Monitor {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    /// Bottom edge of the work area (i.e. where the taskbar starts), in the
    /// same coordinate space as the other fields.
    work_area_bottom: Option<f32>,
}

pub fn window_settings() -> Settings {
    let mut settings = Settings {
        size: Size::new(WINDOW_WIDTH, 720.0),
        resizable: true,
        ..Settings::default()
    };

    if let Some((position, size)) = window_rect() {
        settings.size = size;
        settings.position = Position::Specific(position);
    }
    settings
}

/// Computes the launcher's desired placement: size plus the top-left position,
/// in logical (iced) coordinates. Used both for the initial window settings and
/// to re-anchor the window when it is shown again after being hidden.
pub fn window_rect() -> Option<(Point, Size)> {
    let monitor = primary_work_area()?;

    let height = (monitor.height * HEIGHT_RATIO).clamp(MIN_HEIGHT, monitor.height);
    let width = WINDOW_WIDTH.min(monitor.width);
    let x = monitor.x + (monitor.width - width) / 2.0;
    let bottom = monitor
        .work_area_bottom
        .unwrap_or(monitor.y + monitor.height - BOTTOM_OFFSET);
    let y = bottom - height;

    Some((Point::new(x, y), Size::new(width, height)))
}

/// The desired top-left position of the window, in logical coordinates.
pub fn window_position() -> Option<Point> {
    window_rect().map(|(position, _)| position)
}

#[cfg(target_os = "linux")]
fn primary_work_area() -> Option<Monitor> {
    // KDE's `kscreen-doctor -o` reports the primary output in logical pixels.
    // Prefer it: XRandR over XWayland pools scaled outputs into virtual boxes
    // that don't match the display the user actually sees.
    if let Some(monitor) = kscreen_primary() {
        return Some(monitor);
    }
    xrandr_primary()
}

/// Removes ANSI escape sequences (kscreen-doctor colours its output even when
/// it is piped).
#[cfg(target_os = "linux")]
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until the terminating byte for this escape sequence.
            for esc in chars.by_ref() {
                if esc == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Reads the primary output from `kscreen-doctor -o` (priority 1 wins).
#[cfg(target_os = "linux")]
fn kscreen_primary() -> Option<Monitor> {
    let output = std::process::Command::new("kscreen-doctor")
        .arg("-o")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = strip_ansi(&String::from_utf8_lossy(&output.stdout));

    let mut best: Option<(i32, Monitor)> = None;
    let mut priority: Option<i32> = None;
    let mut geometry: Option<(f32, f32, f32, f32)> = None;

    let mut flush = |priority: Option<i32>, geometry: Option<(f32, f32, f32, f32)>| {
        if let (Some(priority), Some((x, y, w, h))) = (priority, geometry)
            && best
                .as_ref()
                .is_none_or(|(best_priority, _)| priority < *best_priority)
        {
            best = Some((
                priority,
                Monitor {
                    x,
                    y,
                    width: w,
                    height: h,
                    work_area_bottom: None,
                },
            ));
        }
    };

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("Output:") {
            flush(priority, geometry);
            priority = None;
            geometry = None;
            continue;
        }
        if let Some(p) = line.strip_prefix("priority ") {
            priority = p.trim().parse().ok();
        }
        if let Some(g) = line.strip_prefix("Geometry: ") {
            let parts: Vec<&str> = g.split([' ', ',']).filter(|s| !s.is_empty()).collect();
            if let (Some(x), Some(y), Some(dims)) = (parts.first(), parts.get(1), parts.get(2)) {
                let mut dims = dims.split('x');
                if let (Some(w), Some(h)) = (dims.next(), dims.next()) {
                    geometry = Some((
                        x.parse().ok()?,
                        y.parse().ok()?,
                        w.parse().ok()?,
                        h.parse().ok()?,
                    ));
                }
            }
        }
    }
    flush(priority, geometry);

    best.map(|(_, monitor)| monitor)
}

/// Falls back to the XRandR primary monitor (X11 and XWayland sessions).
#[cfg(target_os = "linux")]
fn xrandr_primary() -> Option<Monitor> {
    use x11rb::connection::Connection;
    use x11rb::protocol::randr::ConnectionExt as RandrExt;
    use x11rb::protocol::xproto::ConnectionExt as XProtoExt;

    let (conn, screen_num) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots[screen_num].root;

    let mons = conn.randr_get_monitors(root, true).ok()?.reply().ok()?;
    let mon = mons
        .monitors
        .iter()
        .find(|m| m.primary)
        .or_else(|| mons.monitors.first())?;

    let mut monitor = Monitor {
        x: mon.x as f32,
        y: mon.y as f32,
        width: mon.width as f32,
        height: mon.height as f32,
        work_area_bottom: None,
    };

    // `_NET_WORKAREA` from the root window excludes the taskbar/panel, when
    // that bottom edge lies within this monitor.
    let work_area_bottom = (|| {
        let net = conn
            .intern_atom(false, b"_NET_WORKAREA")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let cardinal = conn
            .intern_atom(false, b"CARDINAL")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let reply = conn
            .get_property(false, root, net, cardinal, 0, 4)
            .ok()?
            .reply()
            .ok()?;
        if reply.format != 32 {
            return None;
        }
        let values: Vec<u32> = reply.value32()?.collect();
        if values.len() < 4 {
            return None;
        }
        Some(values[1] as f32 + values[3] as f32)
    })();
    monitor.work_area_bottom = work_area_bottom;

    Some(monitor)
}

#[cfg(target_os = "windows")]
fn primary_work_area() -> Option<Monitor> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETWORKAREA, SystemParametersInfoW};

    let mut rect: RECT = unsafe { std::mem::zeroed() };
    // Safe: `rect` is a valid out-pointer for a `RECT`.
    unsafe {
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut rect as *mut RECT as *mut _, 0);
    }
    let (left, top, right, bottom) = (
        rect.left as f32,
        rect.top as f32,
        rect.right as f32,
        rect.bottom as f32,
    );
    Some(Monitor {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
        work_area_bottom: Some(bottom),
    })
}
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn primary_work_area() -> Option<Monitor> {
    None
}
