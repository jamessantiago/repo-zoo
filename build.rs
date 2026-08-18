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

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let icon = std::path::Path::new(&manifest_dir).join("packaging/repo-zoo.ico");

    // The GNU cross toolchain ships windres, so the icon can always be
    // embedded. The resource object is linked directly (not via a static
    // archive): the object only defines a local `.rsrc` symbol, and GNU ld
    // drops archive members that resolve no undefined symbol — which left the
    // icon out of the exe when winres linked it through `libresource.a`.
    if target_env == "gnu" {
        let rc = std::path::Path::new(&out_dir).join("repo-zoo.rc");
        std::fs::write(&rc, format!("1 ICON \"{}\"\n", icon.display()))
            .expect("write resource script");
        let obj = std::path::Path::new(&out_dir).join("repo-zoo-resource.o");
        let status = std::process::Command::new("x86_64-w64-mingw32-windres")
            .arg(&rc)
            .arg(&obj)
            .status()
            .expect("failed to run windres");
        if !status.success() {
            panic!("windres failed to compile the icon resource");
        }
        println!("cargo:rustc-link-arg-bins={}", obj.display());
        return;
    }

    // MSVC resource compilation needs rc.exe from the Windows SDK, which only
    // exists when building on a Windows host; a cross `cargo check` from Linux
    // has no such tool.
    if target_env == "msvc" && !host.contains("windows") {
        println!("cargo:warning=skipping the app icon: no MSVC resource compiler on this host");
        return;
    }

    let mut res = winres::WindowsResource::new();
    res.set_icon(icon.to_str().unwrap());
    if let Err(err) = res.compile() {
        panic!("failed to embed repo-zoo.ico: {err}");
    }
}
