// The Steam API DLL is loaded on first use instead of at start, so the exe can write it out next
// to itself first (see net.rs) and still runs when Steam support is unavailable.
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if std::env::var("CARGO_FEATURE_STEAM").is_ok() && target.contains("windows-msvc") {
        println!("cargo:rustc-link-arg-bins=/DELAYLOAD:steam_api64.dll");
        println!("cargo:rustc-link-arg-bins=delayimp.lib");
    }
}
