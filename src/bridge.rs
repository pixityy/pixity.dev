use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::{badge, notify, share};

#[derive(Serialize)]
pub struct NativeInfo {
    pub app: &'static str,
    pub version: &'static str,
    pub platform: &'static str,

    pub capture: bool,
}

#[tauri::command]
pub fn native_info() -> NativeInfo {
    NativeInfo {
        app: "pixity",
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        capture: share::supported(),
    }
}

#[derive(Deserialize)]
pub struct NotifyArgs {
    pub title: String,
    #[serde(default)]
    pub body: String,

    #[serde(default)]
    pub tag: Option<String>,

    #[serde(default)]
    pub silent: bool,
}

#[tauri::command]
pub fn notify_show(app: AppHandle, args: NotifyArgs) -> Result<u32, String> {
    notify::show(&app, args)
}

#[tauri::command]
pub fn notify_close(app: AppHandle, id: u32) {
    notify::close(&app, id);
}

#[tauri::command]
pub fn badge_set(app: AppHandle, count: u32) {
    badge::set(&app, count);
}

#[tauri::command]
pub fn share_probe() -> share::Probe {
    share::probe()
}

#[tauri::command]
pub async fn share_start(app: AppHandle, args: share::StartArgs) -> Result<(), String> {
    share::start(&app, args).await
}

#[tauri::command]
pub async fn share_stop(app: AppHandle) {
    share::stop(&app).await
}

#[tauri::command]
pub async fn share_signal_in(app: AppHandle, sig: share::Signal) -> Result<(), String> {
    share::signal_in(&app, sig).await
}
