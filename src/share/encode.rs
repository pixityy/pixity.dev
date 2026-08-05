use std::ffi::c_void;

type OSStatus = i32;
type CFTypeRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFStringRef = *const c_void;
type CVPixelBufferRef = *mut c_void;
type CMSampleBufferRef = *mut c_void;
type CMBlockBufferRef = *mut c_void;
type CMFormatDescriptionRef = *const c_void;
type VTCompressionSessionRef = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct CMTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}

impl CMTime {
    fn new(value: i64, timescale: i32) -> Self {
        CMTime { value, timescale, flags: 1, epoch: 0 }
    }

    fn invalid() -> Self {
        CMTime { value: 0, timescale: 0, flags: 0, epoch: 0 }
    }
}

type VTCompressionOutputCallback = extern "C" fn(
    output_callback_ref_con: *mut c_void,
    source_frame_ref_con: *mut c_void,
    status: OSStatus,
    info_flags: u32,
    sample_buffer: CMSampleBufferRef,
);

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFNumberCreate(allocator: CFAllocatorRef, the_type: i32, value_ptr: *const c_void)
        -> CFTypeRef;
}

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    fn CVPixelBufferCreate(
        allocator: CFAllocatorRef,
        width: usize,
        height: usize,
        pixel_format_type: u32,
        pixel_buffer_attributes: CFDictionaryRef,
        pixel_buffer_out: *mut CVPixelBufferRef,
    ) -> i32;
    fn CVPixelBufferLockBaseAddress(pixel_buffer: CVPixelBufferRef, flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(pixel_buffer: CVPixelBufferRef, flags: u64) -> i32;
    fn CVPixelBufferGetBaseAddressOfPlane(
        pixel_buffer: CVPixelBufferRef,
        plane_index: usize,
    ) -> *mut c_void;
    fn CVPixelBufferGetBytesPerRowOfPlane(
        pixel_buffer: CVPixelBufferRef,
        plane_index: usize,
    ) -> usize;
}

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferGetDataBuffer(sbuf: CMSampleBufferRef) -> CMBlockBufferRef;
    fn CMSampleBufferGetFormatDescription(sbuf: CMSampleBufferRef) -> CMFormatDescriptionRef;
    fn CMBlockBufferGetDataPointer(
        the_buffer: CMBlockBufferRef,
        offset: usize,
        length_at_offset_out: *mut usize,
        total_length_out: *mut usize,
        data_pointer_out: *mut *mut u8,
    ) -> OSStatus;
    fn CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
        video_desc: CMFormatDescriptionRef,
        parameter_set_index: usize,
        parameter_set_pointer_out: *mut *const u8,
        parameter_set_size_out: *mut usize,
        parameter_set_count_out: *mut usize,
        nal_unit_header_length_out: *mut i32,
    ) -> OSStatus;
}

#[link(name = "VideoToolbox", kind = "framework")]
extern "C" {
    fn VTCompressionSessionCreate(
        allocator: CFAllocatorRef,
        width: i32,
        height: i32,
        codec_type: u32,
        encoder_specification: CFDictionaryRef,
        source_image_buffer_attributes: CFDictionaryRef,
        compressed_data_allocator: CFAllocatorRef,
        output_callback: Option<VTCompressionOutputCallback>,
        output_callback_ref_con: *mut c_void,
        compression_session_out: *mut VTCompressionSessionRef,
    ) -> OSStatus;
    fn VTCompressionSessionEncodeFrame(
        session: VTCompressionSessionRef,
        image_buffer: CVPixelBufferRef,
        presentation_time_stamp: CMTime,
        duration: CMTime,
        frame_properties: CFDictionaryRef,
        source_frame_ref_con: *mut c_void,
        info_flags_out: *mut u32,
    ) -> OSStatus;
    fn VTCompressionSessionCompleteFrames(
        session: VTCompressionSessionRef,
        complete_until_presentation_time_stamp: CMTime,
    ) -> OSStatus;
    fn VTCompressionSessionInvalidate(session: VTCompressionSessionRef);
    fn VTSessionSetProperty(
        session: VTCompressionSessionRef,
        property_key: CFStringRef,
        property_value: CFTypeRef,
    ) -> OSStatus;

    static kVTCompressionPropertyKey_RealTime: CFStringRef;
    static kVTCompressionPropertyKey_ProfileLevel: CFStringRef;
    static kVTCompressionPropertyKey_AverageBitRate: CFStringRef;
    static kVTCompressionPropertyKey_MaxKeyFrameInterval: CFStringRef;
    static kVTCompressionPropertyKey_AllowFrameReordering: CFStringRef;
    static kVTCompressionPropertyKey_ExpectedFrameRate: CFStringRef;
    static kVTProfileLevel_H264_Baseline_AutoLevel: CFStringRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFBooleanTrue: CFTypeRef;
    static kCFBooleanFalse: CFTypeRef;
}

const CODEC_H264: u32 = 0x6176_6331;

const PIXEL_FORMAT_NV12: u32 = 0x3432_3076;
const CF_NUMBER_SINT32: i32 = 3;
const CF_NUMBER_FLOAT64: i32 = 6;

fn cf_i32(v: i32) -> CFTypeRef {
    unsafe { CFNumberCreate(std::ptr::null(), CF_NUMBER_SINT32, &v as *const i32 as *const c_void) }
}

fn cf_f64(v: f64) -> CFTypeRef {
    unsafe { CFNumberCreate(std::ptr::null(), CF_NUMBER_FLOAT64, &v as *const f64 as *const c_void) }
}

pub type VideoSink = Box<dyn Fn(Vec<u8>, bool) + Send + Sync>;

pub struct Video {
    session: VTCompressionSessionRef,
    pixels: CVPixelBufferRef,
    width: usize,
    height: usize,

    sink: *mut VideoSink,
}

unsafe impl Send for Video {}

extern "C" fn on_encoded(
    refcon: *mut c_void,
    _source: *mut c_void,
    status: OSStatus,
    _flags: u32,
    sample: CMSampleBufferRef,
) {
    if status != 0 || sample.is_null() || refcon.is_null() {
        return;
    }

    let sink = unsafe { &*(refcon as *const VideoSink) };

    let annexb = match unsafe { annex_b(sample) } {
        Some(v) => v,
        None => return,
    };
    let keyframe = unsafe { is_keyframe(sample) };
    sink(annexb, keyframe);
}

unsafe fn is_keyframe(sample: CMSampleBufferRef) -> bool {
    #[link(name = "CoreMedia", kind = "framework")]
    extern "C" {
        fn CMSampleBufferGetSampleAttachmentsArray(
            sbuf: CMSampleBufferRef,
            create_if_necessary: u8,
        ) -> CFTypeRef;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFArrayGetCount(the_array: CFTypeRef) -> isize;
        fn CFArrayGetValueAtIndex(the_array: CFTypeRef, idx: isize) -> CFTypeRef;
        fn CFDictionaryGetValue(the_dict: CFTypeRef, key: *const c_void) -> CFTypeRef;
    }
    #[link(name = "CoreMedia", kind = "framework")]
    extern "C" {
        static kCMSampleAttachmentKey_NotSync: CFStringRef;
    }

    let arr = CMSampleBufferGetSampleAttachmentsArray(sample, 0);
    if arr.is_null() || CFArrayGetCount(arr) < 1 {
        return true;
    }
    let dict = CFArrayGetValueAtIndex(arr, 0);
    if dict.is_null() {
        return true;
    }
    let not_sync = CFDictionaryGetValue(dict, kCMSampleAttachmentKey_NotSync as *const c_void);
    not_sync.is_null() || not_sync == kCFBooleanFalse
}

unsafe fn annex_b(sample: CMSampleBufferRef) -> Option<Vec<u8>> {
    const START: [u8; 4] = [0, 0, 0, 1];

    let block = CMSampleBufferGetDataBuffer(sample);
    if block.is_null() {
        return None;
    }
    let mut total = 0usize;
    let mut ptr: *mut u8 = std::ptr::null_mut();
    if CMBlockBufferGetDataPointer(block, 0, std::ptr::null_mut(), &mut total, &mut ptr) != 0
        || ptr.is_null()
    {
        return None;
    }
    let avcc = std::slice::from_raw_parts(ptr, total);

    let mut out = Vec::with_capacity(total + 256);

    let fmt = CMSampleBufferGetFormatDescription(sample);
    let mut nal_len = 4i32;
    let mut count = 0usize;
    if !fmt.is_null() {
        CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
            fmt,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut count,
            &mut nal_len,
        );
    }
    let nal_len = if (1..=4).contains(&nal_len) { nal_len as usize } else { 4 };

    if is_keyframe(sample) && !fmt.is_null() {
        {
            {
                for i in 0..count {
                    let mut ps: *const u8 = std::ptr::null();
                    let mut size = 0usize;
                    if CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                        fmt,
                        i,
                        &mut ps,
                        &mut size,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    ) == 0
                        && !ps.is_null()
                    {
                        out.extend_from_slice(&START);
                        out.extend_from_slice(std::slice::from_raw_parts(ps, size));
                    }
                }
            }
        }
    }

    let mut i = 0usize;
    while i + nal_len <= avcc.len() {
        let mut len = 0usize;
        for b in &avcc[i..i + nal_len] {
            len = (len << 8) | *b as usize;
        }
        i += nal_len;
        if len == 0 || i + len > avcc.len() {
            break;
        }
        out.extend_from_slice(&START);
        out.extend_from_slice(&avcc[i..i + len]);
        i += len;
    }
    Some(out)
}

impl Video {
    pub fn new(width: u32, height: u32, fps: u32, bitrate: u32, sink: VideoSink) -> Result<Self, String> {
        let boxed: *mut VideoSink = Box::into_raw(Box::new(sink));
        let mut session: VTCompressionSessionRef = std::ptr::null_mut();

        let status = unsafe {
            VTCompressionSessionCreate(
                std::ptr::null(),
                width as i32,
                height as i32,
                CODEC_H264,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                Some(on_encoded),
                boxed as *mut c_void,
                &mut session,
            )
        };
        if status != 0 || session.is_null() {
            unsafe { drop(Box::from_raw(boxed)) };
            return Err(format!("VTCompressionSessionCreate failed ({status})"));
        }

        unsafe {
            let set = |k: CFStringRef, v: CFTypeRef, release: bool| {
                VTSessionSetProperty(session, k, v);
                if release && !v.is_null() {
                    CFRelease(v);
                }
            };

            set(kVTCompressionPropertyKey_RealTime, kCFBooleanTrue, false);

            set(
                kVTCompressionPropertyKey_ProfileLevel,
                kVTProfileLevel_H264_Baseline_AutoLevel,
                false,
            );
            set(kVTCompressionPropertyKey_AllowFrameReordering, kCFBooleanFalse, false);
            set(kVTCompressionPropertyKey_AverageBitRate, cf_i32(bitrate as i32), true);
            set(kVTCompressionPropertyKey_ExpectedFrameRate, cf_f64(fps as f64), true);

            set(
                kVTCompressionPropertyKey_MaxKeyFrameInterval,
                cf_i32((fps * 2) as i32),
                true,
            );
        }

        let mut pixels: CVPixelBufferRef = std::ptr::null_mut();

        let cv = unsafe {
            CVPixelBufferCreate(
                std::ptr::null(),
                width as usize,
                height as usize,
                PIXEL_FORMAT_NV12,
                std::ptr::null(),
                &mut pixels,
            )
        };
        if cv != 0 || pixels.is_null() {
            unsafe {
                VTCompressionSessionInvalidate(session);
                CFRelease(session as CFTypeRef);
                drop(Box::from_raw(boxed));
            }
            return Err(format!("CVPixelBufferCreate failed ({cv})"));
        }

        Ok(Video {
            session,
            pixels,
            width: width as usize,
            height: height as usize,
            sink: boxed,
        })
    }

    pub fn encode(&mut self, planes: &super::capture::Planes<'_>) {
        if planes.width != self.width || planes.height != self.height {
            return;
        }

        unsafe {
            if CVPixelBufferLockBaseAddress(self.pixels, 0) != 0 {
                return;
            }
            copy_plane(
                CVPixelBufferGetBaseAddressOfPlane(self.pixels, 0) as *mut u8,
                CVPixelBufferGetBytesPerRowOfPlane(self.pixels, 0),
                planes.y,
                planes.y_stride,
                self.width,
                self.height,
            );
            copy_plane(
                CVPixelBufferGetBaseAddressOfPlane(self.pixels, 1) as *mut u8,
                CVPixelBufferGetBytesPerRowOfPlane(self.pixels, 1),
                planes.uv,
                planes.uv_stride,
                self.width,
                self.height / 2,
            );
            CVPixelBufferUnlockBaseAddress(self.pixels, 0);

            let pts = CMTime::new(planes.pts_nanos, 1_000_000_000);
            VTCompressionSessionEncodeFrame(
                self.session,
                self.pixels,
                pts,
                CMTime::invalid(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
    }
}

unsafe fn copy_plane(
    dst: *mut u8,
    dst_stride: usize,
    src: &[u8],
    src_stride: usize,
    width: usize,
    rows: usize,
) {
    if dst.is_null() {
        return;
    }
    let run = width.min(src_stride).min(dst_stride);
    for row in 0..rows {
        let s = row * src_stride;
        if s + run > src.len() {
            break;
        }
        std::ptr::copy_nonoverlapping(src.as_ptr().add(s), dst.add(row * dst_stride), run);
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        unsafe {
            VTCompressionSessionCompleteFrames(self.session, CMTime::invalid());
            VTCompressionSessionInvalidate(self.session);
            CFRelease(self.session as CFTypeRef);
            CFRelease(self.pixels as CFTypeRef);
            drop(Box::from_raw(self.sink));
        }
    }
}

const OPUS_FRAME: usize = 960;

pub struct Audio {
    enc: audiopus::coder::Encoder,
    pending: Vec<f32>,
    out: Vec<u8>,
}

impl Audio {
    pub fn new(bitrate: u32) -> Result<Self, String> {
        use audiopus::{coder::Encoder, Application, Bitrate, Channels, SampleRate};
        let mut enc = Encoder::new(SampleRate::Hz48000, Channels::Stereo, Application::Audio)
            .map_err(|e| format!("opus: {e}"))?;

        enc.set_bitrate(Bitrate::BitsPerSecond(bitrate as i32))
            .map_err(|e| format!("opus bitrate: {e}"))?;
        Ok(Audio { enc, pending: Vec::with_capacity(OPUS_FRAME * 4), out: vec![0u8; 4000] })
    }

    pub fn push(&mut self, pcm: &[f32], mut emit: impl FnMut(&[u8])) {
        self.pending.extend_from_slice(pcm);
        let stride = OPUS_FRAME * 2;
        let mut taken = 0;
        while self.pending.len() - taken >= stride {
            let frame = &self.pending[taken..taken + stride];
            if let Ok(n) = self.enc.encode_float(frame, &mut self.out) {
                emit(&self.out[..n]);
            }
            taken += stride;
        }
        if taken > 0 {
            self.pending.drain(..taken);
        }
    }
}
