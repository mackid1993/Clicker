// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial client for Channels DVR Server
// Copyright (c) 2026 David Brustein

//! What macOS and Linux agree on: POSIX.
//!
//! Everything here is declared by hand rather than through the `libc` crate,
//! for the same reason `mpv.rs` declares its C API by hand: these are a dozen
//! functions with layouts that have not changed this century, and a dependency
//! is not a saving at that size.

use std::ffi::{c_char, c_int, c_long, c_void, CString};

// --- window chrome -----------------------------------------------------------

/// Nothing to do. macOS rounds and shades its own windows; on Linux the
/// compositor owns the frame. The interface draws its own chrome identically
/// everywhere, so there is nothing to ask the system for.
pub fn apply_chrome(_handle: isize, _dark: bool) {}

/// No handle, deliberately. The only consumer is the tray's restore-from-
/// another-thread path, which is a Win32 mechanism; the tray reads this `None`
/// as "closing quits", which is the honest capability on these platforms
/// until a native activation path earns its place.
pub fn window_handle(_cc: &eframe::CreationContext<'_>) -> Option<isize> {
    None
}

/// Never called while `window_handle` returns `None`; the tray refuses to
/// build without a handle.
pub fn restore_window(_handle: isize) {}

/// `None` means the caller keeps the window position it already has rather
/// than clamping it to a desktop it cannot measure.
pub fn desktop_bounds() -> Option<(f32, f32, f32, f32)> {
    None
}

/// No tray, because half the feature is impossible: hiding the window is
/// easy, but bringing it back needs `restore_window`, which has no
/// implementation here yet. Offering the setting without the way back would
/// be a checkbox that makes windows disappear.
pub const HAS_TRAY: bool = false;

// --- loading libraries -------------------------------------------------------

extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}

/// Why the last attempt to open a library failed, in the loader's own words.
///
/// "libavcodec.so.62: cannot open shared object file" is the sentence that
/// tells somebody their bundle is missing a dependency; "not found" is the
/// sentence that tells them nothing.
pub fn library_error() -> Option<String> {
    let message = unsafe { dlerror() };
    if message.is_null() {
        return None;
    }
    Some(unsafe { std::ffi::CStr::from_ptr(message) }.to_string_lossy().into_owned())
}

/// The same value on macOS and Linux, one of the few flags that is.
const RTLD_NOW: c_int = 2;

/// Open a shared library by path or name. Null on failure.
pub fn open_library(name: &str) -> *mut c_void {
    let Ok(name) = CString::new(name) else {
        return std::ptr::null_mut();
    };
    unsafe { dlopen(name.as_ptr(), RTLD_NOW) }
}

/// A symbol out of an opened library. `name` must be NUL-terminated.
///
/// # Safety
///
/// `module` must be a handle `open_library` returned.
pub unsafe fn library_symbol(module: *mut c_void, name: *const u8) -> *mut c_void {
    dlsym(module, name as *const c_char)
}

extern "C" {
    fn gethostname(name: *mut c_char, len: usize) -> c_int;
}

/// This machine's name, for the DVR's client list.
///
/// The `.local` suffix is stripped: it is mDNS plumbing, and a client list
/// reading "Davids-MacBook-Pro.local" is a client list written by a machine
/// for machines.
pub fn machine_name() -> Option<String> {
    let mut buf = [0u8; 256];
    let ok = unsafe { gethostname(buf.as_mut_ptr() as *mut c_char, buf.len()) };
    if ok != 0 {
        return None;
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    let name = String::from_utf8_lossy(&buf[..end])
        .trim_end_matches(".local")
        .to_string();
    (!name.is_empty()).then_some(name)
}

// --- local time --------------------------------------------------------------

/// `struct tm`, with the BSD `tm_gmtoff` extension — which glibc, musl and
/// macOS all carry, in the same position. That field is the entire reason to
/// call this: it is the offset, computed by the platform's own zone database,
/// daylight saving already applied.
#[repr(C)]
struct Tm {
    tm_sec: c_int,
    tm_min: c_int,
    tm_hour: c_int,
    tm_mday: c_int,
    tm_mon: c_int,
    tm_year: c_int,
    tm_wday: c_int,
    tm_yday: c_int,
    tm_isdst: c_int,
    tm_gmtoff: c_long,
    tm_zone: *const c_char,
}

extern "C" {
    fn localtime_r(time: *const i64, result: *mut Tm) -> *mut Tm;
}

/// Offset from UTC, in seconds, straight from `tm_gmtoff`.
pub fn local_utc_offset_seconds() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut tm = unsafe { std::mem::zeroed::<Tm>() };
    let ok = unsafe { localtime_r(&now, &mut tm) };
    if ok.is_null() {
        return 0;
    }
    tm.tm_gmtoff as i64
}

// --- thread accounting -------------------------------------------------------

#[repr(C)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: c_long,
}

extern "C" {
    fn clock_gettime(id: c_int, tp: *mut Timespec) -> c_int;
}

/// The per-thread processor clock. Different numbers for the same idea.
#[cfg(target_os = "macos")]
const CLOCK_THREAD_CPUTIME_ID: c_int = 16;
#[cfg(target_os = "linux")]
const CLOCK_THREAD_CPUTIME_ID: c_int = 3;

/// Processor time this thread has used, in milliseconds.
///
/// The same measurement `GetThreadTimes` makes on Windows, for the same
/// reason: `mpv_render_context_render` returns when the frame is *due*, not
/// when the work is done, so a wall clock around it measures the video's
/// frame rate and nothing else.
pub fn thread_cpu_ms() -> f64 {
    let mut ts = Timespec { tv_sec: 0, tv_nsec: 0 };
    let ok = unsafe { clock_gettime(CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    if ok != 0 {
        return 0.0;
    }
    ts.tv_sec as f64 * 1_000.0 + ts.tv_nsec as f64 / 1_000_000.0
}
