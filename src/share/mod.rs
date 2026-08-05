// Native screen capture. Protocol only; no backend yet.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

mod capture;
#[cfg(target_os = "macos")]
mod encode;
#[cfg(target_os = "macos")]
mod peer;

pub const fn supported() -> bool {
    cfg!(target_os = "macos")
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

#[cfg(target_os = "macos")]
pub const SIGNAL_EVENT: &str = "pixity://share-signal";

#[cfg(target_os = "macos")]
mod running {
    use super::*;
    use std::sync::Mutex;

    pub struct Running {
        pub capture: capture::Session,
        pub peers: std::sync::Arc<peer::Peers>,
    }

    pub static CURRENT: Mutex<Option<Running>> = Mutex::new(None);
}

pub fn init(_app: &AppHandle) {}

#[cfg(target_os = "macos")]
fn preset(name: &str) -> (u32, u32, u32, u32) {
    match name {
        "crisp" => (1920, 1080, 30, 5_000_000),
        "saver" => (1280, 720, 15, 800_000),
        _ => (1280, 720, 30, 2_500_000),
    }
}

#[cfg(target_os = "macos")]
const TOTAL_BITRATE: u32 = 4_000_000;
#[cfg(target_os = "macos")]
const MIN_BITRATE: u32 = 300_000;

#[cfg(target_os = "macos")]
pub async fn start(app: &AppHandle, args: StartArgs) -> Result<(), String> {
    use std::sync::Arc;
    use tauri::Emitter;

    if !supported() {
        return Err("this build has no native capture".into());
    }
    if args.peers.is_empty() {
        return Err("nobody to share with".into());
    }

    if !capture::permitted() && !capture::request() {
        return Err(capture::permission_hint().to_string());
    }

    stop(app).await;

    let (w, h, fps, preset_bitrate) = preset(&args.quality);

    let viewers = args.peers.len() as u32;
    let bitrate = preset_bitrate.min((TOTAL_BITRATE / viewers.max(1)).max(MIN_BITRATE));

    let handle = app.clone();
    let emit: peer::Emit = Arc::new(move |sig: Signal| {
        let _ = handle.emit(SIGNAL_EVENT, sig);
    });
    let peers = Arc::new(peer::Peers::new(&args.turn, emit));

    let feed = Arc::new(Feed {
        peers: Arc::clone(&peers),
        video: std::sync::Mutex::new(None),
        audio: std::sync::Mutex::new(None),
        fps,
        bitrate,
        want_audio: args.audio,
    });

    let session = capture::start(capture::Config { width: w, height: h, fps }, feed).await?;

    for p in &args.peers {
        peers.add(p).await?;
    }

    *running::CURRENT.lock().unwrap() = Some(running::Running { capture: session, peers });
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub async fn start(_app: &AppHandle, _args: StartArgs) -> Result<(), String> {
    Err("this build has no native capture".into())
}

#[cfg(target_os = "macos")]
pub async fn stop(_app: &AppHandle) {
    let prev = running::CURRENT.lock().unwrap().take();
    if let Some(r) = prev {
        r.capture.stop();
        r.peers.close().await;
    }
}

#[cfg(not(target_os = "macos"))]
pub async fn stop(_app: &AppHandle) {}

#[cfg(target_os = "macos")]
pub async fn signal_in(_app: &AppHandle, sig: Signal) -> Result<(), String> {
    let peers = {
        let cur = running::CURRENT.lock().unwrap();
        match cur.as_ref() {
            Some(r) => std::sync::Arc::clone(&r.peers),

            None => return Ok(()),
        }
    };
    peers.signal(sig).await
}

#[cfg(not(target_os = "macos"))]
pub async fn signal_in(_app: &AppHandle, _sig: Signal) -> Result<(), String> {
    Err("this build has no native capture".into())
}

#[cfg(target_os = "macos")]
struct Feed {
    peers: std::sync::Arc<peer::Peers>,

    video: std::sync::Mutex<Option<encode::Video>>,
    audio: std::sync::Mutex<Option<encode::Audio>>,
    fps: u32,
    bitrate: u32,
    want_audio: bool,
}

#[cfg(target_os = "macos")]
impl capture::Sink for Feed {
    fn video(&self, planes: capture::Planes<'_>) {
        let Ok(mut slot) = self.video.lock() else { return };
        if slot.is_none() {
            let peers = std::sync::Arc::clone(&self.peers);
            let frame = std::time::Duration::from_nanos(1_000_000_000 / self.fps.max(1) as u64);
            let sink: encode::VideoSink = Box::new(move |data, _keyframe| {
                let peers = std::sync::Arc::clone(&peers);

                tauri::async_runtime::spawn(async move {
                    peers.write_video(data, frame).await;
                });
            });

            *slot = encode::Video::new(
                planes.width as u32,
                planes.height as u32,
                self.fps,
                self.bitrate,
                sink,
            )
            .ok();
        }
        if let Some(enc) = slot.as_mut() {
            enc.encode(&planes);
        }
    }

    fn audio(&self, pcm: &[f32]) {
        if !self.want_audio {
            return;
        }
        let Ok(mut slot) = self.audio.lock() else { return };
        if slot.is_none() {
            *slot = encode::Audio::new(128_000).ok();
        }
        let Some(enc) = slot.as_mut() else { return };
        let peers = std::sync::Arc::clone(&self.peers);
        enc.push(pcm, |packet| {
            let peers = std::sync::Arc::clone(&peers);
            let packet = packet.to_vec();
            tauri::async_runtime::spawn(async move {
                peers.write_audio(packet).await;
            });
        });
    }
}
