// Native screen capture. Protocol only; no backend yet.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

mod capture;
mod peer;

pub const fn supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "linux"))
}

#[derive(Serialize)]
pub struct Probe {
    pub supported: bool,

    pub permitted: bool,

    pub blocked_reason: Option<String>,

    pub audio: bool,
}

pub fn probe() -> Probe {
    if !supported() {
        return Probe {
            supported: false,
            permitted: false,
            blocked_reason: None,
            audio: false,
        };
    }
    let permitted = capture::permitted();
    Probe {
        supported: true,
        permitted,
        blocked_reason: if permitted {
            None
        } else {
            Some(capture::permission_hint().to_string())
        },
        audio: capture::has_system_audio(),
    }
}

#[derive(Deserialize)]
pub struct StartArgs {
    pub peers: Vec<String>,

    pub turn: Vec<IceServer>,

    pub quality: String,

    #[serde(default)]
    pub audio: bool,
}

#[derive(Deserialize, Clone)]
pub struct IceServer {
    pub urls: Vec<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub credential: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Signal {
    pub peer: String,

    pub kind: String,
    pub payload: serde_json::Value,
}

pub fn init(_app: &AppHandle) {}

pub async fn start(_app: &AppHandle, _args: StartArgs) -> Result<(), String> {
    if !supported() {
        return Err("this build has no native capture".into());
    }

    Err("native screen capture is not built yet".into())
}

pub async fn stop(_app: &AppHandle) {}

pub async fn signal_in(_app: &AppHandle, _sig: Signal) -> Result<(), String> {
    Err("native screen capture is not built yet".into())
}

#[allow(dead_code)]
fn _wire() {
    peer::_placeholder();
}
