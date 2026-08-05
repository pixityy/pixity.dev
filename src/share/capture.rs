// Platform screen capture. Not implemented yet.

#[allow(dead_code)]
pub struct Planes<'a> {
    pub width: usize,
    pub height: usize,
    pub y: &'a [u8],
    pub y_stride: usize,
    pub uv: &'a [u8],
    pub uv_stride: usize,
    pub pts_nanos: i64,
}

#[allow(dead_code)]
pub trait Sink: Send + Sync + 'static {
    fn video(&self, planes: Planes<'_>);

    fn audio(&self, pcm: &[f32]);
}

#[allow(dead_code)]
pub struct Config {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

pub fn permitted() -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe { mac::CGPreflightScreenCaptureAccess() }
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
        "allow pixity in System Settings > Privacy & Security > Screen & System \
         Audio Recording, then reopen pixity"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "screen capture is not available on this system"
    }
}

pub fn request() -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe { mac::CGRequestScreenCaptureAccess() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

pub fn has_system_audio() -> bool {
    cfg!(target_os = "macos")
}

#[cfg(target_os = "macos")]
pub use mac::Session;

#[cfg(target_os = "macos")]
pub async fn start(cfg: Config, sink: std::sync::Arc<dyn Sink>) -> Result<Session, String> {
    mac::start(cfg, sink).await
}

#[cfg(not(target_os = "macos"))]
pub struct Session;

#[cfg(not(target_os = "macos"))]
pub async fn start(_cfg: Config, _sink: std::sync::Arc<dyn Sink>) -> Result<Session, String> {
    Err("screen capture is not built for this platform".into())
}

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::Arc;
    use std::time::Instant;

    use screencapturekit::content_sharing_picker::{
        SCContentSharingPicker, SCContentSharingPickerConfiguration, SCPickerOutcome,
    };
    use screencapturekit::cv::CVPixelBufferLockFlags;
    use screencapturekit::prelude::*;

    use super::{Config, Planes, Sink};

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        pub fn CGPreflightScreenCaptureAccess() -> bool;
        pub fn CGRequestScreenCaptureAccess() -> bool;
    }

    pub struct Session {
        stream: SCStream,
    }

    impl Session {
        pub fn stop(&self) {
            let _ = self.stream.stop_capture();
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            self.stop();
        }
    }

    struct Handler {
        sink: Arc<dyn Sink>,
        started: Instant,
    }

    impl SCStreamOutputTrait for Handler {
        fn did_output_sample_buffer(&self, sample: CMSampleBuffer, of_type: SCStreamOutputType) {
            match of_type {
                SCStreamOutputType::Screen => self.on_video(&sample),
                SCStreamOutputType::Audio => self.on_audio(&sample),
                _ => {}
            }
        }
    }

    impl Handler {
        fn on_video(&self, sample: &CMSampleBuffer) {
            let Some(buffer) = sample.image_buffer() else { return };

            if buffer.plane_count() < 2 {
                return;
            }
            let Ok(guard) = buffer.lock(CVPixelBufferLockFlags::READ_ONLY) else { return };

            let y_stride = buffer.bytes_per_row_of_plane(0);
            let uv_stride = buffer.bytes_per_row_of_plane(1);
            let y_height = buffer.height_of_plane(0);
            let uv_height = buffer.height_of_plane(1);
            let (Some(y_ptr), Some(uv_ptr)) =
                (guard.base_address_of_plane(0), guard.base_address_of_plane(1))
            else {
                return;
            };

            let (y, uv) = unsafe {
                (
                    std::slice::from_raw_parts(y_ptr, y_stride * y_height),
                    std::slice::from_raw_parts(uv_ptr, uv_stride * uv_height),
                )
            };

            self.sink.video(Planes {
                width: buffer.width(),
                height: buffer.height(),
                y,
                y_stride,
                uv,
                uv_stride,

                pts_nanos: self.started.elapsed().as_nanos() as i64,
            });
        }

        fn on_audio(&self, sample: &CMSampleBuffer) {
            let Some(list) = sample.audio_buffer_list() else { return };

            let mut planes: Vec<&[f32]> = Vec::new();
            let mut idx = 0;
            while let Some(buf) = list.get(idx) {
                let bytes = buf.data();

                let samples = unsafe {
                    std::slice::from_raw_parts(
                        bytes.as_ptr() as *const f32,
                        bytes.len() / std::mem::size_of::<f32>(),
                    )
                };
                planes.push(samples);
                idx += 1;
            }
            if planes.is_empty() {
                return;
            }

            let frames = planes.iter().map(|p| p.len()).min().unwrap_or(0);
            if frames == 0 {
                return;
            }

            let mut pcm = Vec::with_capacity(frames * 2);
            if planes.len() == 1 {
                if list.get(0).map(|b| b.number_channels).unwrap_or(1) >= 2 {
                    pcm.extend_from_slice(planes[0]);
                } else {
                    for &s in planes[0] {
                        pcm.push(s);
                        pcm.push(s);
                    }
                }
            } else {
                for i in 0..frames {
                    pcm.push(planes[0][i]);
                    pcm.push(planes[1][i]);
                }
            }
            self.sink.audio(&pcm);
        }
    }

    pub async fn start(cfg: Config, sink: Arc<dyn Sink>) -> Result<Session, String> {
        let picker = SCContentSharingPickerConfiguration::new();
        let (tx, rx) = tokio::sync::oneshot::channel();
        SCContentSharingPicker::show(&picker, move |outcome| {
            let _ = tx.send(match outcome {
                SCPickerOutcome::Picked(result) => Ok((result.filter(), result.pixel_size())),

                SCPickerOutcome::Cancelled => Err(String::new()),
                SCPickerOutcome::Error(e) => Err(format!("picker: {e}")),
            });
        });
        let (filter, (src_w, src_h)) = rx
            .await
            .map_err(|_| "picker went away".to_string())?
            .map_err(|e| e)?;

        let scale = f64::min(
            cfg.width as f64 / src_w.max(1) as f64,
            cfg.height as f64 / src_h.max(1) as f64,
        )
        .min(1.0);
        let even = |v: f64| ((v.round() as u32) & !1).max(2);
        let width = even(src_w as f64 * scale);
        let height = even(src_h as f64 * scale);

        let config = SCStreamConfiguration::new()
            .with_width(width)
            .with_height(height)
            .with_fps(cfg.fps)
            .with_pixel_format(PixelFormat::YCbCr_420v)
            .with_shows_cursor(true)
            .with_captures_audio(true)
            .with_sample_rate(48_000)
            .with_channel_count(2)
            .with_excludes_current_process_audio(true);

        let mut stream = SCStream::new(&filter, &config);

        let started = Instant::now();
        stream.add_output_handler(
            Handler { sink: Arc::clone(&sink), started },
            SCStreamOutputType::Screen,
        );
        stream.add_output_handler(Handler { sink, started }, SCStreamOutputType::Audio);

        stream
            .start_capture()
            .map_err(|e| format!("could not start capture: {e}"))?;

        Ok(Session { stream })
    }
}
