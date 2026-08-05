#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod badge;
mod bridge;
mod notify;
mod share;

use tauri::{Manager, WindowEvent};

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.unminimize();
                let _ = win.show();
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            notify::init(app.handle());
            share::init(app.handle());
            Ok(())
        })
        .on_window_event(|win, event| {
            if let WindowEvent::CloseRequested { .. } = event {
                win.app_handle().exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
            bridge::native_info,
            bridge::notify_show,
            bridge::notify_close,
            bridge::badge_set,
            bridge::share_probe,
            bridge::share_start,
            bridge::share_stop,
            bridge::share_signal_in,
        ])
        .run(tauri::generate_context!())
        .expect("pixity: failed to start");
}
