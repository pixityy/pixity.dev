use std::sync::atomic::{AtomicU32, Ordering};

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::bridge::NotifyArgs;

struct Ids(AtomicU32);

pub fn init(app: &AppHandle) {
    app.manage(Ids(AtomicU32::new(1)));
}

pub fn show(app: &AppHandle, args: NotifyArgs) -> Result<u32, String> {
    let id = match app.try_state::<Ids>() {
        Some(ids) => ids.0.fetch_add(1, Ordering::Relaxed),
        None => 1,
    };

    let mut builder = app.notification().builder().title(&args.title);
    if !args.body.is_empty() {
        builder = builder.body(&args.body);
    }

    if !args.silent {
        builder = builder.sound("default");
    }

    // tag is accepted but unused: the desktop notifier cannot replace.
    let _ = &args.tag;

    builder.show().map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn close(_app: &AppHandle, _id: u32) {}
