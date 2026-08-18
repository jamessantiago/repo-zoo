// Embeds the application icon into the Windows executable as a Win32 resource
// so the exe shows the repo-zoo icon in Explorer and on the taskbar. No-op on
// other platforms.
fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let host = std::env::var("HOST").unwrap_or_default();

    // The GNU cross toolchain ships windres, so the icon can always be
    // embedded. MSVC resource compilation needs rc.exe from the Windows SDK,
    // which only exists when building on a Windows host; a cross `cargo check`
    // from Linux has no such tool.
    if target_env == "msvc" && !host.contains("windows") {
        println!("cargo:warning=skipping the app icon: no MSVC resource compiler on this host");
        return;
    }

    let mut res = winres::WindowsResource::new();
    let icon = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("packaging/repo-zoo.ico");
    res.set_icon(icon.to_str().unwrap());

    // The mingw-w64 cross compiler names its tools with a target prefix.
    if target_env == "gnu" {
        res.set_windres_path("x86_64-w64-mingw32-windres");
    }

    if let Err(err) = res.compile() {
        panic!("failed to embed repo-zoo.ico: {err}");
    }
}
