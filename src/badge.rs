use tauri::{AppHandle, Manager};

pub fn set(app: &AppHandle, count: u32) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };

    let value = if count == 0 { None } else { Some(count as i64) };
    let _ = win.set_badge_count(value);
}
