//! Global hotkey support (X11/XWayland sessions).
//!
//! iced has no built-in global key listener, so the configured combination is
//! grabbed on the X server's root window from a dedicated thread and forwarded
//! to the application through an iced [`Subscription`]. On Wayland sessions
//! without an X server the hotkey is simply unavailable and the subscription
//! stays empty.

#[cfg(not(target_os = "windows"))]
use std::time::Duration;

#[cfg(not(target_os = "windows"))]
use iced::futures::Stream;
#[cfg(not(target_os = "windows"))]
use iced::futures::channel::mpsc;

use iced::Subscription;

use crate::config::Config;

/// Events produced by the global hotkey listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// The configured hotkey combination was pressed.
    Toggle,
}

/// A parsed hotkey combination: an X11 modifier bit mask and a keysym target.
#[cfg(not(target_os = "windows"))]
#[derive(Debug, Clone, Hash)]
struct Spec {
    mods: u16,
    keysym: u32,
}

/// Returns a subscription that fires [`Event::Toggle`] whenever the configured
/// global hotkey is pressed. Empty (disabled) when no hotkey is configured or
/// when the platform cannot grab one.
#[cfg(not(target_os = "windows"))]
pub fn subscription(config: &Config) -> Subscription<Event> {
    let Some(spec) = parse(&config.hotkey) else {
        return Subscription::none();
    };
    Subscription::run_with(spec, |spec| stream(spec.clone()))
}

#[cfg(target_os = "windows")]
pub fn subscription(config: &Config) -> Subscription<Event> {
    let Some(combo) = win::parse_hotkey(&config.hotkey) else {
        return Subscription::none();
    };
    Subscription::run_with(combo, |combo| win::stream(*combo))
}

/// Asks the running instance to toggle (used by `repo-zoo --toggle`).
#[cfg(target_os = "windows")]
pub fn toggle_active_instance() -> Result<(), String> {
    win::toggle_active_instance()
}

#[cfg(not(target_os = "windows"))]
fn parse(combo: &str) -> Option<Spec> {
    #[cfg(target_os = "linux")]
    {
        use x11rb::protocol::xproto::ModMask;

        let mut mods = ModMask::default();
        let mut key = None;

        for token in combo.split('+') {
            match token.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" => mods |= ModMask::CONTROL,
                "shift" => mods |= ModMask::SHIFT,
                "alt" | "option" | "mod1" => mods |= ModMask::M1,
                "super" | "cmd" | "win" | "mod4" => mods |= ModMask::M4,
                "mod2" => mods |= ModMask::M2,
                "" => {}
                name => {
                    if key.is_some() {
                        return None;
                    }
                    key = Some(keysym(name)?);
                }
            }
        }

        Some(Spec {
            mods: mods.into(),
            keysym: key?,
        })
    }
    #[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
    {
        let _ = combo;
        None
    }
}

/// Maps a hotkey token to an X keysym value.
#[cfg(target_os = "linux")]
fn keysym(name: &str) -> Option<u32> {
    match name {
        "space" => return Some(0x20),
        "return" | "enter" => return Some(0xff0d),
        "tab" => return Some(0xff09),
        "backspace" => return Some(0xff08),
        "escape" | "esc" => return Some(0xff1b),
        "delete" | "del" => return Some(0xffff),
        "home" => return Some(0xff50),
        "end" => return Some(0xff57),
        "pageup" | "pgup" => return Some(0xff55),
        "pagedown" | "pgdn" => return Some(0xff56),
        "insert" | "ins" => return Some(0xff63),
        "left" => return Some(0xff51),
        "up" => return Some(0xff52),
        "right" => return Some(0xff53),
        "down" => return Some(0xff54),
        _ => {}
    }

    // A bare "f" is the letter, not a function key: parse fails and the single
    // character branch below handles it.
    if let Some(n) = name
        .strip_prefix('f')
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|&n| (1..=24).contains(&n))
    {
        return Some(0xffbe + n - 1);
    }

    let mut chars = name.chars();
    let c = chars.next()?;
    if chars.next().is_none() && c.is_ascii_alphanumeric() {
        Some(c as u32)
    } else {
        None
    }
}

/// Builds the listener stream: a thread that owns an X connection and forwards
/// hotkey presses until the subscription is dropped (which closes the channel).
#[cfg(target_os = "linux")]
fn stream(spec: Spec) -> impl Stream<Item = Event> {
    let (tx, rx) = mpsc::unbounded();
    std::thread::spawn(move || {
        if let Err(err) = listen(spec, tx) {
            eprintln!("repo-zoo: global hotkey unavailable: {err}");
        }
    });
    rx
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
fn stream(_spec: Spec) -> impl Stream<Item = Event> {
    iced::futures::stream::empty()
}

/// Windows global hotkey support via `RegisterHotKey`.
///
/// A dedicated thread owns a hidden message-only window and a Win32 message
/// loop; `WM_HOTKEY` is turned into [`Event::Toggle`]. `--toggle` reaches the
/// same window by posting a custom message (`FindWindowW` + `PostMessageW`),
/// which doubles as the Windows analogue of the KDE D-Bus toggle.
#[cfg(target_os = "windows")]
mod win {
    use super::Event;
    use iced::futures::Stream;
    use iced::futures::channel::mpsc;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_NOREPEAT, RegisterHotKey};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    /// Window class (and window title) of the hidden hotkey window.
    pub const WINDOW_CLASS: &str = "repo-zoo-hotkey";
    /// Custom message used by `--toggle` to reach the running instance.
    pub const TOGGLE_MSG: u32 = WM_APP + 1;
    const HOTKEY_ID: i32 = 1;

    /// A parsed Windows hotkey: a `MOD_*` modifier mask and a virtual-key code.
    #[derive(Debug, Clone, Copy, Hash)]
    pub struct WinHotkey {
        pub mods: u32,
        pub vk: u16,
    }

    pub fn parse_hotkey(combo: &str) -> Option<WinHotkey> {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN,
        };

        let mut mods: u32 = 0;
        let mut key: Option<u16> = None;
        for token in combo.split('+') {
            match token.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" => mods |= MOD_CONTROL,
                "shift" => mods |= MOD_SHIFT,
                "alt" | "option" | "mod1" => mods |= MOD_ALT,
                "super" | "cmd" | "win" | "mod4" => mods |= MOD_WIN,
                "" => {}
                name => {
                    if key.is_some() {
                        return None;
                    }
                    key = Some(virtual_key(name)?);
                }
            }
        }
        // Windows refuses to register a hotkey without a modifier.
        if mods == 0 {
            return None;
        }
        Some(WinHotkey { mods, vk: key? })
    }

    fn virtual_key(name: &str) -> Option<u16> {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
        match name {
            "space" => return Some(VK_SPACE),
            "return" | "enter" => return Some(VK_RETURN),
            "tab" => return Some(VK_TAB),
            "backspace" => return Some(VK_BACK),
            "escape" | "esc" => return Some(VK_ESCAPE),
            "delete" | "del" => return Some(VK_DELETE),
            "home" => return Some(VK_HOME),
            "end" => return Some(VK_END),
            "pageup" | "pgup" => return Some(VK_PRIOR),
            "pagedown" | "pgdn" => return Some(VK_NEXT),
            "insert" | "ins" => return Some(VK_INSERT),
            "left" => return Some(VK_LEFT),
            "up" => return Some(VK_UP),
            "right" => return Some(VK_RIGHT),
            "down" => return Some(VK_DOWN),
            _ => {}
        }
        if let Some(n) = name
            .strip_prefix('f')
            .and_then(|n| n.parse::<u32>().ok())
            .filter(|&n| (1..=24).contains(&n))
        {
            return Some(VK_F1 + (n - 1) as u16);
        }
        let mut chars = name.chars();
        let c = chars.next()?;
        if chars.next().is_none() && c.is_ascii_alphanumeric() {
            Some(c.to_ascii_uppercase() as u16)
        } else {
            None
        }
    }

    pub fn stream(combo: WinHotkey) -> impl Stream<Item = Event> {
        let (tx, rx) = mpsc::unbounded();
        std::thread::spawn(move || {
            if let Err(err) = listen(combo, tx) {
                eprintln!("repo-zoo: global hotkey unavailable: {err}");
            }
        });
        rx
    }

    fn wide(s: &str) -> Vec<u16> {
        OsString::from(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// Registers the hotkey on a hidden window and pumps messages until the
    /// window is destroyed.
    fn listen(
        combo: WinHotkey,
        tx: mpsc::UnboundedSender<Event>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        unsafe extern "system" fn wnd_proc(
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            match msg {
                WM_HOTKEY if wparam == HOTKEY_ID as WPARAM => {
                    unsafe {
                        toggle_from(hwnd);
                    }
                    0
                }
                TOGGLE_MSG => {
                    unsafe {
                        toggle_from(hwnd);
                    }
                    0
                }
                WM_DESTROY => {
                    unsafe {
                        PostQuitMessage(0);
                    }
                    0
                }
                _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
            }
        }

        // The window proc has no context, so the channel sender is parked in
        // the window's user data slot.
        unsafe fn toggle_from(hwnd: HWND) {
            unsafe {
                let sender =
                    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut mpsc::UnboundedSender<Event>;
                if !sender.is_null() {
                    let _ = (*sender).unbounded_send(Event::Toggle);
                }
            }
        }

        unsafe {
            let hinstance = GetModuleHandleW(ptr::null());
            let class_name = wide(WINDOW_CLASS);

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: Some(wnd_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinstance,
                hIcon: 0,
                hCursor: 0,
                hbrBackground: 0,
                lpszMenuName: ptr::null(),
                lpszClassName: class_name.as_ptr(),
                hIconSm: 0,
            };
            RegisterClassExW(&wc);

            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                wide("repo-zoo").as_ptr(),
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                hinstance,
                ptr::null(),
            );
            if hwnd == 0 {
                return Err("failed to create the hotkey window".into());
            }

            let sender = Box::into_raw(Box::new(tx)) as isize;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, sender);

            // MOD_NOREPEAT avoids spamming toggles while the key is held down.
            if RegisterHotKey(hwnd, HOTKEY_ID, combo.mods | MOD_NOREPEAT, combo.vk as u32) == 0 {
                drop(Box::from_raw(sender as *mut mpsc::UnboundedSender<Event>));
                return Err(
                    format!("failed to register the hotkey (error {})", GetLastError()).into(),
                );
            }

            let mut msg = std::mem::zeroed::<MSG>();
            while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            // The message loop has ended; no more dispatches can touch the
            // sender, so it can be reclaimed.
            drop(Box::from_raw(sender as *mut mpsc::UnboundedSender<Event>));
        }
        Ok(())
    }

    /// Asks the running instance to toggle (used by `repo-zoo --toggle`).
    pub fn toggle_active_instance() -> Result<(), String> {
        let hwnd = unsafe { FindWindowW(wide(WINDOW_CLASS).as_ptr(), ptr::null()) };
        if hwnd == 0 {
            return Err("repo-zoo is not running".to_string());
        }
        let ok = unsafe { PostMessageW(hwnd, TOGGLE_MSG, 0, 0) };
        if ok == 0 {
            return Err(format!("failed to toggle repo-zoo (error {})", unsafe {
                GetLastError()
            }));
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

        #[test]
        fn parses_super_f_hotkey() {
            let combo = parse_hotkey("super+f").expect("super+f should parse");
            assert_eq!(combo.mods, MOD_WIN);
            assert_eq!(combo.vk, 'F' as u16);
        }

        #[test]
        fn parses_ctrl_shift_f12() {
            let combo = parse_hotkey("ctrl+shift+f12").expect("should parse");
            assert_eq!(combo.mods, MOD_CONTROL | MOD_SHIFT);
            assert_eq!(combo.vk, VK_F12);
        }

        #[test]
        fn rejects_two_keys_and_missing_modifier() {
            assert!(parse_hotkey("super+f+w").is_none());
            assert!(
                parse_hotkey("f").is_none(),
                "a bare key cannot be registered"
            );
        }
    }
}

/// Grabs `spec` on the root window and pumps X events until the application
/// stops listening.
#[cfg(target_os = "linux")]
fn listen(spec: Spec, tx: mpsc::UnboundedSender<Event>) -> Result<(), Box<dyn std::error::Error>> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{self, ConnectionExt as XProto};

    let (conn, screen_num) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen_num].root;
    let mods = xproto::ModMask::from(spec.mods);

    let keycode = find_keycode(&conn, spec.keysym)?;

    // Grab both with and without the lock modifiers (CapsLock/NumLock), so the
    // hotkey keeps working regardless of toggle state.
    let lock = xproto::ModMask::LOCK | xproto::ModMask::M2;
    for modifiers in [mods, mods | lock] {
        conn.grab_key(
            false,
            root,
            modifiers,
            keycode,
            xproto::GrabMode::ASYNC,
            xproto::GrabMode::ASYNC,
        )?
        .check()
        .map_err(|err| {
            format!(
                "failed to grab hotkey (mods 0x{:x}): {err}",
                u16::from(modifiers)
            )
        })?;
    }
    conn.flush()?;

    loop {
        match conn.poll_for_event()? {
            Some(x11rb::protocol::Event::KeyPress(press))
                if press.detail == keycode && (u16::from(press.state) & spec.mods) == spec.mods =>
            {
                if tx.unbounded_send(Event::Toggle).is_err() {
                    // The subscription went away; releasing the grab happens
                    // when the connection is dropped on return.
                    return Ok(());
                }
            }
            Some(_) | None => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}

/// Resolves a keysym to a keycode using the server's current keyboard mapping.
/// The base (unmodified) keysym of each key is compared, which is stable across
/// layouts for a physical key position.
#[cfg(target_os = "linux")]
fn find_keycode(
    conn: &impl x11rb::connection::Connection,
    target: u32,
) -> Result<u8, Box<dyn std::error::Error>> {
    use x11rb::protocol::xproto::ConnectionExt as XProto;

    let setup = conn.setup();
    let first = setup.min_keycode;
    let count = setup.max_keycode - first + 1;
    let reply = conn.get_keyboard_mapping(first, count)?.reply()?;

    let per = reply.keysyms_per_keycode as usize;
    for (index, chunk) in reply.keysyms.chunks(per).enumerate() {
        if chunk.first().copied() == Some(target) {
            return Ok(first + index as u8);
        }
    }
    Err(format!("hotkey '{}' is not in the current keyboard mapping", target).into())
}

#[cfg(test)]
#[cfg(not(target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn parses_super_f_hotkey() {
        let spec = parse("super+f").expect("super+f should parse");
        assert!(spec.mods & u16::from(x11rb::protocol::xproto::ModMask::M4) != 0);
        assert_eq!(spec.keysym, 'f' as u32);
    }

    #[test]
    fn rejects_multiple_keys() {
        // Modifier combos are fine, but two key tokens are not.
        assert!(parse("super+shift+f").is_some());
        assert!(parse("super+f+w").is_none());
    }
}
