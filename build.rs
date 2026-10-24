// The Steam API DLL is loaded on first use instead of at start, so the exe can write it out next
// to itself first (see net.rs) and still runs when Steam support is unavailable.
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if std::env::var("CARGO_FEATURE_STEAM").is_ok() && target.contains("windows-msvc") {
        println!("cargo:rustc-link-arg-bins=/DELAYLOAD:steam_api64.dll");
        println!("cargo:rustc-link-arg-bins=delayimp.lib");
    }
    // Explorer / taskbar icon for PixelHorde.exe
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if target.contains("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.compile().expect("embedding the exe icon");
    }
}
