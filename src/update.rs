//! Self-update from GitHub Releases, so the game never needs Git. Every push to main builds a new
//! PixelHorde.exe and publishes it as release `build-N`. On start the game asks GitHub for the latest
//! release; if it is newer than this build it downloads it next to the running exe and swaps it in
//! (Windows lets a running exe be renamed, so the old one becomes PixelHorde.old and is deleted on the
//! next start). The Mac app does the same with the program inside PixelHorde.app, which every
//! release also carries on its own as PixelHorde-macOS.

use std::io::Read;
use std::sync::{Arc, Mutex};

pub const REPO: &str = "Littlebutta/Horde-defence-game";

/// The release asset this platform updates from.
const ASSET: &str = if cfg!(target_os = "macos") { "PixelHorde-macOS" } else { "PixelHorde.exe" };

pub fn build_number() -> u32 {
    option_env!("BUILD_NUMBER")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

#[derive(Clone, PartialEq)]
pub enum UpState {
    Checking,
    UpToDate,
    Downloading(u32),
    Ready(u32),
    Failed(String),
    Dev(Option<u32>),
}

pub struct Updater {
    pub state: Arc<Mutex<UpState>>,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(120))
        .user_agent(concat!("PixelHorde/", env!("CARGO_PKG_VERSION")))
        .build()
}

impl Updater {
    pub fn start() -> Updater {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::fs::remove_file(exe.with_extension("old"));
        }
        let state = Arc::new(Mutex::new(UpState::Checking));
        let st = state.clone();
        std::thread::spawn(move || {
            let r = check_and_fetch(&st);
            if let Err(e) = r {
                *st.lock().unwrap() = UpState::Failed(e);
            }
        });
        Updater { state }
    }
    pub fn get(&self) -> UpState {
        self.state.lock().unwrap().clone()
    }
    pub fn label(&self) -> String {
        let b = build_number();
        match self.get() {
            UpState::Checking => format!("Build {} · checking for updates…", b),
            UpState::UpToDate => format!("Build {} · up to date", b),
            UpState::Downloading(n) => format!("Build {} · downloading build {}…", b, n),
            UpState::Ready(n) => format!("Build {} downloaded. Restart to play it", n),
            UpState::Failed(e) => format!("Build {} · update check failed: {}", b, e),
            UpState::Dev(Some(n)) => format!("Development build · latest release is build {}", n),
            UpState::Dev(None) => "Development build".to_string(),
        }
    }
    pub fn restart() {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe).spawn();
        }
        std::process::exit(0);
    }
}

fn check_and_fetch(st: &Arc<Mutex<UpState>>) -> Result<(), String> {
    let a = agent();
    let url = format!("https://api.github.com/repos/{}/releases/latest", REPO);
    let resp = a
        .get(&url)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(404, _) => "no release yet (or the repo is private)".to_string(),
            e => short(&e.to_string()),
        })?;
    let j: serde_json::Value = resp.into_json().map_err(|e| short(&e.to_string()))?;
    let tag = j["tag_name"].as_str().unwrap_or("");
    let latest: u32 = tag.trim_start_matches("build-").parse().unwrap_or(0);
    let mine = build_number();
    if mine == 0 {
        *st.lock().unwrap() = UpState::Dev(if latest > 0 { Some(latest) } else { None });
        return Ok(());
    }
    if latest <= mine {
        *st.lock().unwrap() = UpState::UpToDate;
        return Ok(());
    }
    let asset = j["assets"]
        .as_array()
        .and_then(|v| {
            v.iter()
                .find(|x| x["name"].as_str() == Some(ASSET))
        })
        .and_then(|x| x["browser_download_url"].as_str())
        .ok_or(format!("release has no {}", ASSET))?;
    *st.lock().unwrap() = UpState::Downloading(latest);
    let mut bytes = Vec::new();
    a.get(asset)
        .call()
        .map_err(|e| short(&e.to_string()))?
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() < 100_000 {
        return Err("download was too small".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let new = exe.with_extension("new");
    let old = exe.with_extension("old");
    std::fs::write(&new, &bytes).map_err(|e| {
        // macOS runs an app straight from Downloads out of a read-only copy
        if cfg!(target_os = "macos") && e.kind() == std::io::ErrorKind::ReadOnlyFilesystem {
            "move Pixel Horde to Applications to get updates".to_string()
        } else {
            e.to_string()
        }
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(e.to_string());
    }
    *st.lock().unwrap() = UpState::Ready(latest);
    Ok(())
}

fn short(s: &str) -> String {
    s.chars().take(70).collect()
}
