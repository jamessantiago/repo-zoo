//! KDE/Plasma global hotkey support via KWin scripting + D-Bus.
//!
//! On Plasma sessions always running under Wayland (or X11) a plain X11 root
//! grab does not fire, so the tab-combo handed to the X11 listener in
//! [`crate::hotkey`] is instead registered with KWin through a tiny script
//! package. KWin (kglobalacceld) owns the native grab, shows the shortcut under
//! System Settings → Shortcuts → KWin, and on activation `callDBus`s back to
//! this process, which exposes a small D-Bus service that turns the call into
//! an iced [`Message::Toggle`].

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use iced::Subscription;
use iced::futures::channel::mpsc;

use crate::config::Config;

/// KWin script plugin id and D-Bus contract used for activation.
const PLUGIN_ID: &str = "repo-zoo";
const SERVICE_NAME: &str = "org.repozoo";
const OBJECT_PATH: &str = "/org/repozoo/Hotkey";
const INTERFACE_NAME: &str = "org.repozoo.Hotkey";
const ACTION_NAME: &str = "toggle-repo-zoo";

/// Events produced by the KDE hotkey/activation bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// The registered shortcut was activated (or `--toggle` was requested).
    Toggle,
}

/// True when the current desktop session is KDE/Plasma.
pub fn is_plasma() -> bool {
    let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let desktop = desktop.to_ascii_lowercase();
    desktop.contains("kde") || desktop.contains("plasma")
}

/// Installs (or refreshes) the KWin script that registers the configured
/// hotkey and points it at [`SERVICE_NAME`], then activates it in the running
/// compositor without a restart.
pub fn install(config: &Config) -> Result<(), String> {
    let sequence = qt_key_sequence(&config.hotkey)
        .ok_or_else(|| format!("hotkey {:?} cannot be expressed for KWin", config.hotkey))?;

    let package = scripts_dir()?.join(PLUGIN_ID);
    let code_dir = package.join("contents/code");
    fs::create_dir_all(&code_dir).map_err(|e| e.to_string())?;

    fs::write(package.join("metadata.json"), metadata_json().as_bytes())
        .map_err(|e| e.to_string())?;
    fs::write(code_dir.join("main.js"), main_js(&sequence).as_bytes())
        .map_err(|e| e.to_string())?;

    // Persist the enablement so KWin loads the script automatically at login.
    let _ = Command::new("kwriteconfig6")
        .args([
            "--file",
            "kwinrc",
            "--group",
            "Plugins",
            "--key",
            &format!("{PLUGIN_ID}Enabled"),
            "true",
        ])
        .status();

    load_and_start(&code_dir.join("main.js"))
}

/// Loads the script into the running KWin and starts it via its D-Bus Script
/// object so the shortcut is active immediately (no compositor restart).
fn load_and_start(main_js: &std::path::Path) -> Result<(), String> {
    let conn = zbus_blocking()?;
    let scripting = ("org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting");

    // A previous run may already have it loaded; replace so config changes
    // (e.g. a new hotkey) take effect.
    let _: bool = conn
        .call_method(
            Some(scripting.0),
            scripting.1,
            Some(scripting.2),
            "unloadScript",
            &(PLUGIN_ID,),
        )
        .and_then(|m| m.body().deserialize())
        .unwrap_or(false);

    let file = main_js.to_string_lossy().into_owned();
    let id: i32 = conn
        .call_method(
            Some(scripting.0),
            scripting.1,
            Some(scripting.2),
            "loadScript",
            &(file,),
        )
        .and_then(|m| m.body().deserialize())
        .map_err(|e| format!("kwin loadScript: {e}"))?;

    if id >= 0 {
        let script_path = format!("/Scripting/Script{id}");
        conn.call_method(
            Some(scripting.0),
            script_path.as_str(),
            Some("org.kde.kwin.Script"),
            "run",
            &(),
        )
        .map_err(|e| format!("kwin start script: {e}"))?;
    }
    // id == -1 means "already loaded"; our unload above should have prevented
    // that, but if it didn't the script is still active, which is fine.

    Ok(())
}

/// Removes the KWin script and its persistence bits.
pub fn uninstall() -> Result<(), String> {
    if let Ok(conn) = zbus_blocking() {
        let _: bool = conn
            .call_method(
                Some("org.kde.KWin"),
                "/Scripting",
                Some("org.kde.kwin.Scripting"),
                "unloadScript",
                &(PLUGIN_ID,),
            )
            .and_then(|m| m.body().deserialize())
            .unwrap_or(false);
    }
    if let Ok(dir) = scripts_dir() {
        let _ = fs::remove_dir_all(dir.join(PLUGIN_ID));
    }
    let _ = Command::new("kwriteconfig6")
        .args([
            "--file",
            "kwinrc",
            "--group",
            "Plugins",
            "--key",
            &format!("{PLUGIN_ID}Enabled"),
            "--delete",
        ])
        .status();
    Ok(())
}

/// Returns a subscription that hosts the D-Bus `Toggle` service and forwards
/// activations to [`Event::Toggle`]. Empty when not on Plasma or when no
/// hotkey is configured.
pub fn subscription(config: &Config) -> Subscription<Event> {
    if !is_plasma() || config.hotkey.trim().is_empty() {
        return Subscription::none();
    }
    Subscription::run_with(PLUGIN_ID, |_| {
        let (tx, rx) = mpsc::unbounded();
        thread::spawn(move || host_service(tx));
        rx
    })
}

/// Hosts the D-Bus service for the lifetime of the thread. If another
/// instance already owns the name (or the bus is unavailable) it quietly
/// exits; the receiver then just stays closed.
fn host_service(tx: mpsc::UnboundedSender<Event>) {
    let Ok(conn) = zbus_blocking() else {
        return;
    };
    let Ok(..) = conn
        .object_server()
        .at(OBJECT_PATH, ToggleService { tx: Mutex::new(tx) })
    else {
        return;
    };
    // Err(NameTaken) (= another instance already owns the name) or any other
    // failure means someone else is reachable; this instance must not hijack it.
    if conn.request_name(SERVICE_NAME).is_err() {
        return;
    }
    // keep `conn` (and its executor) alive until the subscription is dropped
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

/// Sends a `Toggle` call to the running instance (used by `--toggle`).
#[cfg(not(target_os = "windows"))]
pub fn toggle_active_instance() -> Result<(), String> {
    let conn = zbus_blocking()?;
    conn.call_method(
        Some(SERVICE_NAME),
        OBJECT_PATH,
        Some(INTERFACE_NAME),
        "Toggle",
        &(),
    )
    .map_err(|e| e.to_string())
    .map(|_| ())
}

struct ToggleService {
    tx: Mutex<mpsc::UnboundedSender<Event>>,
}

#[zbus::interface(name = "org.repozoo.Hotkey")]
impl ToggleService {
    fn toggle(&self) {
        let _ = self.tx.lock().unwrap().unbounded_send(Event::Toggle);
    }
}

/// Converts the config hotkey notation (e.g. `ctrl+alt+z`) into the Qt key
/// sequence form KWin's `registerShortcut` expects (e.g. `Ctrl+Alt+Z`).
fn qt_key_sequence(hotkey: &str) -> Option<String> {
    use std::borrow::Cow;

    let mut parts: Vec<Cow<'static, str>> = Vec::new();
    for token in hotkey.split('+') {
        let token = token.trim().to_ascii_lowercase();
        let part: Cow<'static, str> = match token.as_str() {
            "ctrl" | "control" => "Ctrl".into(),
            "shift" => "Shift".into(),
            "alt" | "option" | "mod1" => "Alt".into(),
            "super" | "cmd" | "win" | "mod4" => "Meta".into(),
            "space" => "Space".into(),
            "return" | "enter" => "Return".into(),
            "tab" => "Tab".into(),
            "escape" | "esc" => "Esc".into(),
            "backspace" => "BackSpace".into(),
            "delete" | "del" => "Del".into(),
            "insert" | "ins" => "Ins".into(),
            "home" => "Home".into(),
            "end" => "End".into(),
            "pageup" | "pgup" => "PgUp".into(),
            "pagedown" | "pgdn" => "PgDown".into(),
            "left" => "Left".into(),
            "up" => "Up".into(),
            "right" => "Right".into(),
            "down" => "Down".into(),
            name => {
                if let Some(n) = name.strip_prefix('f') {
                    match n.parse::<u32>() {
                        Ok(n) if (1..=24).contains(&n) => return Some(format!("F{n}")),
                        // A bare "f" is the letter, not a function key.
                        _ => {}
                    }
                }
                let mut chars = name.chars();
                let c = chars.next()?;
                if chars.next().is_none() && c.is_ascii_alphanumeric() {
                    c.to_ascii_uppercase().to_string().into()
                } else {
                    return None;
                }
            }
        };
        parts.push(part);
    }
    (!parts.is_empty()).then(|| {
        parts
            .iter()
            .map(|p| p.as_ref())
            .collect::<Vec<_>>()
            .join("+")
    })
}

fn scripts_dir() -> Result<PathBuf, String> {
    let base = dirs::data_dir().ok_or("cannot locate XDG data dir")?;
    Ok(base.join("kwin/scripts"))
}

fn metadata_json() -> String {
    format!(
        r#"{{
    "KPackageStructure": "KWin/Script",
    "KPlugin": {{
        "Id": "{PLUGIN_ID}",
        "Name": "repo-zoo toggle",
        "Description": "Global shortcut that toggles the repo-zoo launcher",
        "Icon": "preferences-system-windows-script-test",
        "License": "MIT",
        "Version": "1.0",
        "EnabledByDefault": true,
        "Authors": [{{"Name": "repo-zoo"}}]
    }},
    "X-Plasma-API": "javascript"
}}"#
    )
}

fn main_js(sequence: &str) -> String {
    format!(
        r#"
// Toggles the repo-zoo launcher. Installed by repo-zoo itself via kde::install.
registerShortcut("{ACTION_NAME}", "Toggle repo-zoo visibility", "{sequence}", function () {{
    callDBus("{SERVICE_NAME}", "{OBJECT_PATH}", "{INTERFACE_NAME}", "Toggle");
}});
"#
    )
}

fn zbus_blocking() -> Result<zbus::blocking::Connection, String> {
    zbus::blocking::Connection::session().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn super_f_maps_to_meta_f_for_kwin() {
        assert_eq!(qt_key_sequence("super+f").as_deref(), Some("Meta+F"));
    }

    #[test]
    fn maps_supported_modifier_tokens() {
        assert_eq!(qt_key_sequence("ctrl+alt+z").as_deref(), Some("Ctrl+Alt+Z"));
        assert_eq!(qt_key_sequence("win+f").as_deref(), Some("Meta+F"));
    }
}
