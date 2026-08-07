//! Declarations for the C shim in `csrc/rd_media.c`.
//!
//! Only the shim's own flat API is declared here, never FFmpeg's structs. That
//! is the whole point of having a shim: a wrong field offset in a
//! hand-transcribed `AVFrame` is silent memory corruption, whereas a wrong
//! signature here is a link error.

use std::os::raw::{c_char, c_double, c_float, c_int};

#[repr(C)]
pub struct RdMedia {
    _opaque: [u8; 0],
}

pub const RD_NOTHING: c_int = 0;
pub const RD_VIDEO: c_int = 1;
pub const RD_AUDIO: c_int = 2;
pub const RD_EOF: c_int = -1;

extern "C" {
    /// `abort_flag` points at an `i32` the shim hands to FFmpeg's interrupt
    /// callback. Setting it nonzero from any thread makes an in-flight open,
    /// read or seek fail promptly instead of running to completion. It must
    /// stay alive until after `rd_close`, because FFmpeg keeps the pointer.
    /// `live_start_index` is FFmpeg's HLS option of the same name: 0 joins at
    /// the head of the playlist, a negative value that many segments back from
    /// its end. Ignored by every other demuxer.
    pub fn rd_open(
        url: *const c_char,
        out_rate: c_int,
        out_channels: c_int,
        live_start_index: c_int,
        abort_flag: *mut c_int,
        err: *mut c_char,
        errlen: c_int,
    ) -> *mut RdMedia;
    pub fn rd_close(m: *mut RdMedia);

    pub fn rd_video_width(m: *mut RdMedia) -> c_int;
    pub fn rd_video_height(m: *mut RdMedia) -> c_int;
    /// The size of the picture actually decoded, which can differ from what
    /// the container advertised at open. Check before every copy.
    pub fn rd_frame_width(m: *mut RdMedia) -> c_int;
    pub fn rd_frame_height(m: *mut RdMedia) -> c_int;
    pub fn rd_has_audio(m: *mut RdMedia) -> c_int;
    pub fn rd_out_rate(m: *mut RdMedia) -> c_int;
    pub fn rd_out_channels(m: *mut RdMedia) -> c_int;

    pub fn rd_duration(m: *mut RdMedia) -> c_double;
    pub fn rd_seekable(m: *mut RdMedia) -> c_int;

    pub fn rd_next(m: *mut RdMedia, pts_out: *mut c_double) -> c_int;
    /// Returns -1 rather than writing past the end when the decoded picture
    /// does not fit the buffer described by the last three arguments.
    pub fn rd_video_copy(
        m: *mut RdMedia,
        dst: *mut u8,
        dst_stride: c_int,
        dst_width: c_int,
        dst_height: c_int,
    ) -> c_int;
    pub fn rd_audio_samples(m: *mut RdMedia) -> c_int;
    pub fn rd_audio_copy(m: *mut RdMedia, dst: *mut c_float, max_samples: c_int) -> c_int;

    pub fn rd_seek(m: *mut RdMedia, seconds: c_double) -> c_int;

    /// Microseconds spent inside `av_read_frame` and inside the decoders since
    /// this was last called, and the worst single read. Reading resets them.
    pub fn rd_take_timings(
        m: *mut RdMedia,
        read_us: *mut i64,
        decode_us: *mut i64,
        read_max_us: *mut i64,
    );

    pub fn rd_license() -> *const c_char;
    pub fn rd_version() -> *const c_char;
}

/// The FFmpeg build's own licence string.
///
/// Read at startup and logged. The licence position of this application
/// depends on FFmpeg having been built without GPL components, and this is the
/// binary saying so itself rather than a claim in a text file.
pub fn license() -> String {
    unsafe { cstr(rd_license()) }
}

pub fn version() -> String {
    unsafe { cstr(rd_version()) }
}

unsafe fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
}
