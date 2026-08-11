// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native client for Channels DVR
// Copyright (c) 2026 David Brustein

//! Windows. The platform this program grew up on, so everything here is the
//! original code moved rather than new code written — the port must not cost
//! the platform that already worked anything.

use std::ffi::{c_char, c_int, c_void};
use std::path::PathBuf;

// --- window chrome -----------------------------------------------------------

/// The frame is drawn by the application: no system caption, and the
/// interface provides its own buttons and resize edges. That is what lets
/// the material run edge to edge, and it is the Windows convention besides.
pub const NATIVE_FRAME: bool = false;

/// Nothing sits in the top-left corner but our own title, so it needs no
/// clearance.
pub const CAPTION_INSET: f32 = 0.0;

/// An undecorated window, everything inside it ours to draw.
pub fn shape_window(viewport: eframe::egui::ViewportBuilder) -> eframe::egui::ViewportBuilder {
    viewport.with_decorations(false)
}

/// Dark title-bar tinting, and rounded corners where the system has them.
///
/// This used to ask DWM for Mica as well, which is what made the application
/// Windows 11 only — the material does not exist before build 22000, and a
/// window shaped for it on Windows 10 is a transparent hole to the desktop.
/// The material is painted in `backdrop` now, on every version alike. What is
/// left here are the two attributes worth asking the system for, both of which
/// degrade quietly on their own.
pub fn apply_chrome(handle: isize, dark: bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_ROUND,
    };

    let hwnd = HWND(handle as *mut _);
    unsafe {
        // Dark mode. Windows 10 1809 was the release that added this, which is
        // where the installer's floor comes from: below it the shadow and the
        // resize border around the window are drawn light, against an
        // application that is entirely dark.
        let dark_flag: i32 = if dark { 1 } else { 0 };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_flag as *const _ as *const _,
            std::mem::size_of::<i32>() as u32,
        );

        // Rounded corners, on the versions that round. Windows 11 rounds app
        // windows and says so explicitly here because this window draws its
        // own frame; Windows 10 has no such attribute and rejects the call,
        // which is why the result is discarded rather than checked. Square
        // corners there are correct — every other window on that desktop has
        // them.
        let corner = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const _ as *const _,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

/// The app's own window handle, for the DWM attributes and for the tray.
///
/// Taken from the window itself rather than GetForegroundWindow: the app is not
/// necessarily foreground when it first paints, and asking the OS for whatever
/// happens to be in front once applied dark chrome to a terminal instead.
pub fn window_handle(cc: &eframe::CreationContext<'_>) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = cc.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
        _ => None,
    }
}

/// Bring the window back and put it in front, from any thread.
pub fn restore_window(handle: isize) {
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
        };

        let hwnd = HWND(handle as *mut _);
        // Both, in this order: SW_SHOW undoes the hide, SW_RESTORE undoes a
        // minimise. Whichever way it left the screen, it comes back.
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// The notification-area icon works here: the watcher thread can restore a
/// hidden window through Win32, which is the half of the feature that makes
/// hiding one defensible.
pub const HAS_TRAY: bool = true;

/// The bounding box of every monitor together, in logical pixels.
pub fn desktop_bounds() -> Option<(f32, f32, f32, f32)> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };
    unsafe {
        let (x, y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        let (w, h) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
        if w <= 0 || h <= 0 {
            return None;
        }
        Some((x as f32, y as f32, (x + w) as f32, (y + h) as f32))
    }
}

/// This machine's name, for the DVR's client list.
pub fn machine_name() -> Option<String> {
    std::env::var("COMPUTERNAME").ok().filter(|s| !s.is_empty())
}

/// Nothing here refuses the local network, so no failure is ever that.
pub fn permission_denied(_message: &str) -> bool {
    false
}

/// Never called: nothing to open, no permission to grant.
pub fn open_local_network_settings() {}

/// Nothing stands between this program and the local network here.
pub const LOCAL_NETWORK_HINT: &str = "";

/// Nothing to request: there is no local network permission here.
pub fn request_local_network() {}

/// No menu bar: this platform puts an application's commands inside its
/// window, which is where this one draws them.
pub fn install_menu_bar() {}

/// Hand a link to the desktop's browser.
pub fn open_url(url: &str) {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
}

/// Nothing to keep in step: there is no menu bar here, so a rebinding
/// changes the settings page and nothing else.
pub fn sync_menu_shortcuts(_settings: &crate::settings::Settings) {}

/// Never anything, there being no menu to ask from.
pub fn menu_command() -> Option<String> {
    None
}

// --- where files go ----------------------------------------------------------

/// The profile root settings live under. Roams with the user.
pub fn config_home() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?))
}

/// The root for everything large or rebuildable. Local to the machine.
pub fn data_home() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?))
}

// --- fonts -------------------------------------------------------------------

/// The interface face: Segoe UI Variable where Windows 11 has it, Segoe UI
/// otherwise. Read from the system rather than shipped, because it is already
/// on every machine this build runs on.
pub fn text_font() -> Option<Vec<u8>> {
    for candidate in [
        r"C:\Windows\Fonts\SegoeUIVariableStatic-Display.ttf",
        r"C:\Windows\Fonts\SegoeUIVariableStatic-Regular.ttf",
        r"C:\Windows\Fonts\segoeui.ttf",
    ] {
        if let Ok(bytes) = std::fs::read(candidate) {
            return Some(bytes);
        }
    }
    None
}

/// Caption and control glyphs come from the Windows icon font. Substituting
/// lookalike Unicode characters is what makes custom chrome read as wrong:
/// the shapes, weights and optical sizes do not match the real thing.
pub fn icon_font() -> Option<Vec<u8>> {
    for candidate in [
        r"C:\Windows\Fonts\SegoeIcons.ttf", // Segoe Fluent Icons (Windows 11)
        r"C:\Windows\Fonts\segmdl2.ttf",    // Segoe MDL2 Assets (Windows 10)
    ] {
        if let Ok(bytes) = std::fs::read(candidate) {
            return Some(bytes);
        }
    }
    None
}

// --- local time --------------------------------------------------------------

/// Offset from UTC, in seconds. GetLocalTime and GetSystemTime differ by
/// exactly the offset; the Win32 time zone API's bias fields are signed the
/// opposite way round to how everyone expects and are a reliable source of
/// off-by-an-hour bugs.
pub fn local_utc_offset_seconds() -> i64 {
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};

    let (local, system): (SYSTEMTIME, SYSTEMTIME) =
        unsafe { (GetLocalTime(), GetSystemTime()) };

    let minutes = |t: &SYSTEMTIME| {
        t.wHour as i64 * 60 + t.wMinute as i64 + t.wDay as i64 * 24 * 60
    };
    let mut delta = (minutes(&local) - minutes(&system)) * 60;
    // Guard against a month boundary making the day component wrap.
    if delta > 15 * 3600 {
        delta -= 24 * 3600;
    } else if delta < -15 * 3600 {
        delta += 24 * 3600;
    }
    delta
}

// --- the live buffer's disk tricks -------------------------------------------

/// Mark the buffer sparse, so regions can later be given back to the disk.
///
/// Best effort. A file system that will not do this — FAT32 on a removable
/// disk, say — still works; the buffer simply keeps everything it is given and
/// the window bounds addressability rather than bytes on disk.
pub fn make_sparse(file: &tokio::fs::File) {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Ioctl::FSCTL_SET_SPARSE;
    use windows::Win32::System::IO::DeviceIoControl;

    let handle = HANDLE(file.as_raw_handle());
    let mut returned = 0u32;
    unsafe {
        let _ = DeviceIoControl(
            handle,
            FSCTL_SET_SPARSE,
            None,
            0,
            None,
            0,
            Some(&mut returned),
            None,
        );
    }
}

/// Give a range at the front of the buffer back to the disk.
///
/// The file does not shrink and nothing after the hole moves — that is the
/// entire point, because the demuxer is holding byte offsets into it and a
/// shift would land it in the middle of a packet. Reading a released range
/// returns zeros, which is why the player is told to stop offering seeks into
/// it.
pub fn punch_hole(file: &tokio::fs::File, from: u64, len: u64) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Ioctl::{FILE_ZERO_DATA_INFORMATION, FSCTL_SET_ZERO_DATA};
    use windows::Win32::System::IO::DeviceIoControl;

    if len == 0 {
        return false;
    }
    let zero = FILE_ZERO_DATA_INFORMATION {
        FileOffset: from as i64,
        BeyondFinalZero: (from + len) as i64,
    };
    let handle = HANDLE(file.as_raw_handle());
    let mut returned = 0u32;
    unsafe {
        DeviceIoControl(
            handle,
            FSCTL_SET_ZERO_DATA,
            Some(&zero as *const _ as *const std::ffi::c_void),
            std::mem::size_of::<FILE_ZERO_DATA_INFORMATION>() as u32,
            None,
            0,
            Some(&mut returned),
            None,
        )
        .is_ok()
    }
}

// --- loading libraries and OpenGL --------------------------------------------

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetCurrentThread() -> *mut c_void;
    fn GetThreadTimes(
        thread: *mut c_void,
        creation: *mut u64,
        exit: *mut u64,
        kernel: *mut u64,
        user: *mut u64,
    ) -> c_int;
}

#[link(name = "opengl32")]
extern "system" {
    fn wglGetProcAddress(name: *const u8) -> *mut c_void;
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// What the mpv library is called here, for messages about not finding it.
pub const MPV_LIBRARY: &str = "libmpv-2.dll";

/// Where libmpv might be, most specific first: beside the executable, the
/// working directory, the build tree.
pub fn mpv_candidates() -> Vec<String> {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(MPV_LIBRARY)))
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    [beside.as_str(), MPV_LIBRARY, "third_party/mpv/libmpv-2.dll"]
        .iter()
        .filter(|c| !c.is_empty())
        .map(|c| c.to_string())
        .collect()
}

/// Why the last attempt to open a library failed. Windows reports this
/// through GetLastError rather than a string, and the codes it gives for a
/// missing dependency are not worth translating, so nothing is offered.
pub fn library_error() -> Option<String> {
    None
}

/// Open a shared library by path or name. Null on failure.
pub fn open_library(name: &str) -> *mut c_void {
    unsafe { LoadLibraryW(wide(name).as_ptr()) }
}

/// A symbol out of an opened library. `name` must be NUL-terminated.
///
/// # Safety
///
/// `module` must be a handle `open_library` returned.
pub unsafe fn library_symbol(module: *mut c_void, name: *const u8) -> *mut c_void {
    GetProcAddress(module, name)
}

/// How mpv finds the OpenGL functions of the context eframe created.
///
/// `wglGetProcAddress` answers only for OpenGL 1.2 and later; everything from
/// 1.1 lives in `opengl32.dll` itself and has to be looked up there. mpv asks
/// for both kinds, so a loader that consults only one of them fails at
/// context creation with nothing useful to say about why.
pub unsafe extern "C" fn gl_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    let found = wglGetProcAddress(name as *const u8);
    // These four are what WGL returns for "no such function", rather than null.
    let bad = [0usize, 1, 2, 3, usize::MAX];
    if !bad.contains(&(found as usize)) {
        return found;
    }
    static OPENGL32: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let module = *OPENGL32.get_or_init(|| {
        let name = wide("opengl32.dll");
        LoadLibraryW(name.as_ptr()) as usize
    }) as *mut c_void;
    if module.is_null() {
        return std::ptr::null_mut();
    }
    GetProcAddress(module, name as *const u8)
}

// --- thread accounting -------------------------------------------------------

/// Processor time this thread has used, in milliseconds.
///
/// Wall time is useless here and was actively misleading. `mpv_render_context_render`
/// returns when the frame is *due*, not when the work is done, so timing it
/// with a clock measures the video's frame interval: it read 16.6ms on 60fps
/// content whatever the machine or the picture size, which looks exactly like
/// a player that is only just keeping up. Measured properly the same frames
/// cost 13.6ms of processor at 1080p and 7.8ms at a third of the size.
pub fn thread_cpu_ms() -> f64 {
    let (mut created, mut exited, mut kernel, mut user) = (0u64, 0u64, 0u64, 0u64);
    let ok = unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    if ok == 0 {
        return 0.0;
    }
    // Both are in 100-nanosecond units.
    (kernel + user) as f64 / 10_000.0
}
