// Platform screen capture. Not implemented yet.

pub fn permitted() -> bool {
    #[cfg(target_os = "macos")]
    {
        false
    }
    #[cfg(target_os = "linux")]
    {
        false
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

pub fn permission_hint() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "allow pixity in System Settings > Privacy & Security > Screen & System Audio Recording"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "screen capture is not available on this system"
    }
}

pub fn has_system_audio() -> bool {
    cfg!(target_os = "macos")
}
