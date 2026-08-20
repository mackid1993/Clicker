// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial client for Channels DVR Server
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

/// Whether asking to stay on top is a request this desktop will simply ignore.
///
/// Never. `WindowLevel::AlwaysOnTop` becomes `WS_EX_TOPMOST`, which is the
/// oldest promise in this window manager and is kept.
pub fn desktop_owns_stacking() -> bool {
    false
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

/// This thread's own top-level window with exactly this title, if any.
///
/// Scoped to the calling thread, and that is the point of it. The obvious
/// spellings both reach outside this program: `FindWindowW` matches across
/// every process on the desktop, and `EnumWindows` walks every top-level
/// window on it, reading titles that are none of our business, before a
/// process-id test can throw them away. Two copies of this program running at
/// once (an installed one and a test build, say) each found the *other's*
/// picture-in-picture window that way and redressed it.
///
/// `EnumThreadWindows` never leaves the thread, so the fence is structural
/// rather than a filter applied after the fact: a thread belongs to exactly
/// one process, the other copy's windows are never visited at all, and
/// nothing on the rest of the desktop is looked at. It also stops the search
/// reading like an inventory of the user's open windows, which is a thing
/// only a keylogger has any business taking.
///
/// Every window this program owns is on the interface thread, because winit
/// creates them all on the event loop, so every caller here must be on it
/// too. The title is still the handle: eframe never exposes a deferred
/// viewport's HWND.
fn find_own_window(title: &str) -> Option<windows::Win32::Foundation::HWND> {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, GetWindowTextW};

    struct Search {
        wanted: Vec<u16>,
        found: Option<HWND>,
    }
    unsafe extern "system" fn visit(
        hwnd: HWND,
        lparam: LPARAM,
    ) -> windows::Win32::Foundation::BOOL {
        let search = &mut *(lparam.0 as *mut Search);
        let mut text = [0u16; 128];
        let len = GetWindowTextW(hwnd, &mut text) as usize;
        if text[..len] == search.wanted[..search.wanted.len() - 1] {
            search.found = Some(hwnd);
            return false.into();
        }
        true.into()
    }

    let mut search = Search {
        wanted: wide(title),
        found: None,
    };
    unsafe {
        // False when the callback stops the walk early, which is the found
        // case, not a failure.
        let _ = EnumThreadWindows(
            GetCurrentThreadId(),
            Some(visit),
            LPARAM(&mut search as *mut Search as isize),
        );
    }
    search.found
}

/// Round the popped-out picture's corners, the way `apply_chrome` rounds the
/// main window's.
///
/// Windows 11 rounds every window that owns a frame and leaves undecorated
/// ones square, so a window that is nothing but picture ships with the only
/// sharp corners on the desktop unless it asks. It has to be found by title:
/// the main window's handle came from eframe at startup, and a deferred
/// viewport's handle is never exposed. Windows 10 has no corner attribute and
/// rejects the call, which is why nothing is checked, square being correct
/// there.
///
/// Interface thread only, like everything `find_own_window` backs.
pub fn dress_pip(title: &str) {
    if let Some(hwnd) = find_own_window(title) {
        dress_window(hwnd);
    }
}

/// Everything the popped-out window should be wearing: round corners, and the
/// top of the pile.
///
/// Once, when the window is dressed. `with_always_on_top` on the builder is
/// what keeps it there afterwards; this is belt to that suspenders, for the
/// first frame, because the style bit alone let the window spawn behind. It
/// is emphatically not a keeper: a program that re-asserts its own z-order on
/// a timer forever is shaped like an overlay nobody asked for, and Windows
/// keeps `WS_EX_TOPMOST` perfectly well without being nagged.
///
/// The raise comes without the activation. Called from `dress_pip` this runs
/// *inside* the window's first pass, and a `SetForegroundWindow` here sends
/// the whole activation cascade back into the window procedure mid-paint,
/// after which the window never presents a frame again: absent from the
/// composition while its blits counted merrily on. Treat any programmatic
/// activation of this window as suspect; winit makes it foreground at
/// creation on its own.
fn dress_window(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };

    unsafe {
        let corner = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const _ as *const _,
            std::mem::size_of::<i32>() as u32,
        );
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// Whether this window is off the screen: minimized, or hidden to the tray.
///
/// Asked from the wake thread, which is why it reads the window rather than
/// any state of ours: `IsIconic` and `IsWindowVisible` are two loads out of
/// the window's own bits and are safe from any thread.
///
/// Both cases matter and they are different mechanisms. The caption's
/// minimize sends the window iconic; minimize-to-tray sends
/// `ViewportCommand::Visible(false)`, which hides it without ever making it
/// iconic. Nobody can see the main window in either case.
pub fn window_hidden(handle: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindowVisible};
    let hwnd = HWND(handle as *mut _);
    unsafe { IsIconic(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() }
}

/// Whether this window's thread is inside a native move or resize loop.
fn in_move_or_size(handle: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO, GUI_INMOVESIZE,
    };
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let thread = GetWindowThreadProcessId(HWND(handle as *mut _), None);
        GetGUIThreadInfo(thread, &mut info).is_ok() && info.flags.contains(GUI_INMOVESIZE)
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

/// Make a window paint, from any thread, now rather than at the queue's mercy.
///
/// The ordinary path — invalidate and wait for `WM_PAINT` — only delivers
/// when the thread's message queue goes idle, and the synthesis that happens
/// then picks *one* dirty window, preferring the topmost. While the
/// popped-out picture repaints at video rate it is always the topmost and
/// always dirty again by the next idle, so the main window loses every draw:
/// measured at two paints a second against the thirty asked for, with plain
/// `InvalidateRect` at any cadence. `RDW_UPDATENOW` is the documented way
/// out: it delivers the `WM_PAINT` for the region this call itself dirties
/// before returning, riding the cross-thread send path rather than entering
/// the idle lottery at all. (A hand-made *posted* `WM_PAINT` is not the same
/// thing and crashed the process within seconds — a paint the window system
/// never booked. This one is booked by the invalidate half of the same
/// call.)
pub fn request_window_paint(handle: isize) {
    use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{RedrawWindow, RDW_INVALIDATE, RDW_UPDATENOW};

    // Not while somebody is dragging the window by its caption.
    //
    // A native move loop is a message loop of its own, and it advances the
    // window one mouse message at a time. A paint forced into it is not a
    // paint the loop asked for: it is sent, it is synchronous, and the loop
    // cannot look at the mouse again until the whole browse screen has been
    // drawn. Sixty of those a second is a window that follows the pointer in
    // steps — which is exactly what dragging felt like. A few a second keeps
    // the screen alive under the drag and leaves the rest of the time to the
    // pointer.
    if in_move_or_size(handle) {
        static BEAT: AtomicU32 = AtomicU32::new(0);
        if BEAT.fetch_add(1, Relaxed) % 8 != 0 {
            return;
        }
    }
    unsafe {
        let _ = RedrawWindow(
            HWND(handle as *mut _),
            None,
            None,
            RDW_INVALIDATE | RDW_UPDATENOW,
        );
    }
}

/// How often to beat the heart that keeps the main window painting while the
/// picture is popped out.
///
/// Every beat is a whole browse screen drawn, forced, whether anything
/// changed or not — so this number is the price of the feature, paid sixty
/// seconds a minute for as long as the picture is out. It started at 16ms
/// because `InvalidateRect` used to lose a lottery to the popped window and
/// no cadence helped; with `RedrawWindow` the rate is exactly what is asked
/// for, and asking for sixty a second bought nothing but heat — a browse
/// screen nobody is looking at does not need more frames than a browse
/// screen anybody is looking at. Thirty is smooth to the hand and half the
/// work; the channel from the popped window is drained on the same beat, so
/// its buttons still answer within a frame.
///
/// `CLICKER_PIP_WAKE` overrides it from the bench, in milliseconds.
pub fn pip_wake_ms() -> u64 {
    static MS: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *MS.get_or_init(|| {
        std::env::var("CLICKER_PIP_WAKE")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|ms| *ms > 0)
            .unwrap_or(33)
    })
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

/// Nothing to add: Segoe UI is already at the front of the chain and carries
/// the arrows and dashes an interface reaches for. See the Linux
/// implementation for what this is for.
pub fn fallback_font() -> Option<Vec<u8>> {
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
    fn LoadLibraryExW(name: *const u16, reserved: *mut c_void, flags: u32) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
    fn GetModuleFileNameW(module: *mut c_void, filename: *mut u16, size: u32) -> u32;
    fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
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

// --- OpenGL, and which OpenGL ------------------------------------------------

/// The module WGL and the OpenGL 1.1 entry points are read out of.
///
/// By name, which resolves the way the loader resolves every request for this
/// name in this process: the executable's own directory first, and System32
/// after it. That is not an implementation detail to work around — it is the
/// mechanism. `place_software_opengl` puts Mesa in the first of those
/// directories, and from the next start everything in the process draws with
/// it: this program, glutin, and mpv through `gl_proc_address`, without any of
/// them being told.
fn opengl32() -> *mut c_void {
    static MODULE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *MODULE.get_or_init(|| unsafe { LoadLibraryW(wide("opengl32.dll").as_ptr()) as usize })
        as *mut c_void
}

/// Whether a software OpenGL can be deployed here, and so whether the setting
/// that chooses one is worth showing. This platform alone: it is the only one
/// with a state to escape — an OpenGL that exists, answers, and cannot draw.
pub const HAS_SOFTWARE_GL: bool = true;

/// The handful of WGL calls this program makes, resolved out of whichever
/// `opengl32.dll` is being asked about.
struct Wgl {
    get_proc_address: unsafe extern "system" fn(*const u8) -> *mut c_void,
    get_current_dc: unsafe extern "system" fn() -> *mut c_void,
    get_current_context: unsafe extern "system" fn() -> *mut c_void,
    create_context: unsafe extern "system" fn(*mut c_void) -> *mut c_void,
    share_lists: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
    make_current: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
    delete_context: unsafe extern "system" fn(*mut c_void) -> i32,
}

impl Wgl {
    /// Out of a particular module, which is what the probe needs: it asks
    /// about a library that is deliberately not this process's own.
    fn from_module(module: *mut c_void) -> Option<Self> {
        if module.is_null() {
            return None;
        }
        unsafe fn find<T>(module: *mut c_void, name: &[u8]) -> Option<T> {
            let found = GetProcAddress(module, name.as_ptr());
            if found.is_null() {
                return None;
            }
            Some(std::mem::transmute_copy::<*mut c_void, T>(&found))
        }
        unsafe {
            Some(Wgl {
                get_proc_address: find(module, b"wglGetProcAddress\0")?,
                get_current_dc: find(module, b"wglGetCurrentDC\0")?,
                get_current_context: find(module, b"wglGetCurrentContext\0")?,
                create_context: find(module, b"wglCreateContext\0")?,
                share_lists: find(module, b"wglShareLists\0")?,
                make_current: find(module, b"wglMakeCurrent\0")?,
                delete_context: find(module, b"wglDeleteContext\0")?,
            })
        }
    }

    /// The process's own OpenGL, whichever one that turned out to be.
    fn get() -> Option<&'static Self> {
        static WGL: std::sync::OnceLock<Option<Wgl>> = std::sync::OnceLock::new();
        WGL.get_or_init(|| Wgl::from_module(opengl32())).as_ref()
    }
}

/// Load a DLL's own directory alongside it, so a software OpenGL finds the
/// driver library that sits beside it.
const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x08;

/// Where a software OpenGL is kept, most specific first. These are the files
/// to copy *from*; what makes one take effect is `place_software_opengl`.
///
/// `CLICKER_OPENGL` may name either the pair of libraries' directory or the
/// loader itself. The second is what the installer ships. The third is the
/// hatch for a copy installed without one, needing no administrator.
pub fn software_opengl_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(asked) = std::env::var_os("CLICKER_OPENGL") {
        let asked = PathBuf::from(asked);
        candidates.push(if asked.extension().is_some() {
            asked.parent().map(PathBuf::from).unwrap_or(asked)
        } else {
            asked
        });
    }
    if let Some(beside) = app_directory() {
        candidates.push(beside.join("mesa"));
    }
    if let Some(data) = crate::paths::data_dir() {
        candidates.push(data.join("mesa"));
    }
    candidates
}

/// The two libraries a Mesa deployment needs. Since Mesa 21.3.0 the first is
/// only a loader and the second is where the drivers are, so one without the
/// other is a library that opens and cannot make a context.
const RENDERER: [&str; 2] = ["opengl32.dll", "libgallium_wgl.dll"];

/// Where this program is installed, which is also the first directory the
/// loader searches for every library this process asks for by name.
fn app_directory() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(PathBuf::from)
}

/// A software OpenGL that is complete, and where it is.
pub fn software_opengl() -> Option<PathBuf> {
    static FOUND: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    FOUND
        .get_or_init(|| {
            software_opengl_candidates()
                .into_iter()
                .find(|dir| RENDERER.iter().all(|name| dir.join(name).is_file()))
        })
        .clone()
}

/// Windows' own OpenGL, by full path.
///
/// Asked for by path and never by name, because the name is exactly what this
/// has to see past: where the software renderer is deployed, `opengl32.dll`
/// means Mesa in this process, and the question here is what the *machine*
/// has.
fn system_opengl() -> Option<PathBuf> {
    let mut buffer = [0u16; 512];
    let len = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if len == 0 || len as usize >= buffer.len() {
        return None;
    }
    use std::os::windows::ffi::OsStringExt;
    let dir = std::ffi::OsString::from_wide(&buffer[..len as usize]);
    Some(PathBuf::from(dir).join("opengl32.dll"))
}

/// Whether this process is drawing with a software OpenGL rather than the
/// machine's.
///
/// Read out of the loader rather than remembered from a decision: the decision
/// was made by a previous launch, when it put a library where this one would
/// find it, and what actually got loaded is the only honest answer.
pub fn software_gl_in_use() -> bool {
    let loaded = match module_path(opengl32()) {
        Some(path) => path,
        None => return false,
    };
    match system_opengl() {
        Some(system) => !loaded.as_os_str().eq_ignore_ascii_case(system.as_os_str()),
        None => false,
    }
}

/// What happened when the software renderer was asked for.
pub enum Placement {
    /// Already what this process is drawing with.
    Active,
    /// Put where the next launch will find it. This one cannot use it: the
    /// libraries a process draws with are bound to it before any of its own
    /// code runs.
    Placed,
    /// There is no software renderer to place.
    Missing,
    /// The directory would not take it, which on a machine-wide install means
    /// it belongs to Administrators.
    Refused(String),
}

/// Put the software renderer where the loader will find it: beside the
/// executable, which is the first directory searched for every library this
/// process asks for by name.
///
/// **Why a file and not a switch.** glutin makes its context through
/// `wglCreateContext` and its neighbours, and those are *static* imports —
/// `glutin_wgl_sys` generates them with gl_generator's `StaticGenerator` over
/// a bare `#[link(name = "opengl32")]`, so the loader binds them before `main`
/// runs and nothing this program does afterwards can point them somewhere
/// else. `GLUTIN_WGL_OPENGL_DLL` looks like the lever and is not: glutin uses
/// the library it names only as a fallback for proc addresses, so setting it
/// yields a context from one OpenGL and entry points from another — which is
/// not a fallback, it is a crash.
///
/// What the loader does honour is the search order, and the executable's own
/// directory is the first thing in it. So the switch is a file, and it takes
/// effect at the next start.
pub fn place_software_opengl() -> Placement {
    if software_gl_in_use() {
        return Placement::Active;
    }
    let (Some(from), Some(into)) = (software_opengl(), app_directory()) else {
        return Placement::Missing;
    };
    // Checked before anything is copied, because the failure it prevents is
    // the worst one this feature can cause. An `opengl32.dll` beside the
    // executable is bound by the loader before any of this program runs, so a
    // library for the wrong processor there does not fall back to anything —
    // it stops the program from starting at all, with Windows' own "This app
    // can't run on your PC" and nothing to say which file did it. The
    // installer stages a matching pair; `CLICKER_OPENGL` and a copy dropped in
    // by hand are where a mismatch comes from.
    for name in RENDERER {
        match pe_machine(&from.join(name)) {
            Some(machine) if machine == THIS_MACHINE => {}
            Some(machine) => {
                return Placement::Refused(format!(
                    "{} is for {machine:#06x}, not this processor",
                    from.join(name).display()
                ))
            }
            None => {
                return Placement::Refused(format!("{} could not be read", from.join(name).display()))
            }
        }
    }
    for name in RENDERER {
        if let Err(e) = std::fs::copy(from.join(name), into.join(name)) {
            // Half a renderer is worse than none: the loader would find a
            // loader with no drivers behind it. Anything already copied goes
            // back out.
            remove_software_opengl();
            return Placement::Refused(format!("{}: {e}", into.join(name).display()));
        }
    }
    Placement::Placed
}

/// What processor this build is for, in the numbers a PE header uses.
const THIS_MACHINE: u16 = if cfg!(target_arch = "aarch64") { 0xAA64 } else { 0x8664 };

/// What processor a PE file was built for, read out of its header: the four
/// bytes at 0x3C hold the offset of the PE signature, and the machine field is
/// the two bytes after it. The same reading `build.ps1` does before it packages
/// anything, for the same reason.
fn pe_machine(path: &std::path::Path) -> Option<u16> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path).ok()?;
    let mut four = [0u8; 4];
    file.seek(SeekFrom::Start(0x3C)).ok()?;
    file.read_exact(&mut four).ok()?;
    file.seek(SeekFrom::Start(u32::from_le_bytes(four) as u64 + 4)).ok()?;
    let mut two = [0u8; 2];
    file.read_exact(&mut two).ok()?;
    Some(u16::from_le_bytes(two))
}

/// Take it away again, so the next start is back on the machine's own OpenGL.
///
/// Returns whether anything was actually removed, which is the difference
/// between "restart to apply" and nothing to say.
pub fn remove_software_opengl() -> bool {
    let Some(dir) = app_directory() else {
        return false;
    };
    let mut removed = false;
    for name in RENDERER {
        let file = dir.join(name);
        if file.is_file() && std::fs::remove_file(&file).is_ok() {
            removed = true;
        }
    }
    removed
}

/// Where a loaded module came from on disk.
///
/// Asked rather than assumed. A path handed to the loader is what was meant to
/// be loaded; this is what it actually mapped, and it is how
/// `software_gl_in_use` knows which OpenGL this process is drawing with rather
/// than guessing from a decision some earlier launch made.
fn module_path(module: *mut c_void) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;

    if module.is_null() {
        return None;
    }
    unsafe {
        let mut name = [0u16; 1024];
        let len = GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32);
        if len == 0 {
            return None;
        }
        Some(PathBuf::from(std::ffi::OsString::from_wide(
            &name[..len as usize],
        )))
    }
}

#[link(name = "user32")]
extern "system" {
    fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: c_int,
        y: c_int,
        width: c_int,
        height: c_int,
        parent: *mut c_void,
        menu: *mut c_void,
        instance: *mut c_void,
        param: *mut c_void,
    ) -> *mut c_void;
    fn DestroyWindow(window: *mut c_void) -> i32;
    fn GetDC(window: *mut c_void) -> *mut c_void;
    fn ReleaseDC(window: *mut c_void, dc: *mut c_void) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    fn ChoosePixelFormat(dc: *mut c_void, want: *const PixelFormat) -> c_int;
    fn SetPixelFormat(dc: *mut c_void, format: c_int, want: *const PixelFormat) -> i32;
}

/// `PIXELFORMATDESCRIPTOR`, declared by hand for the same reason the rest of
/// this file declares things by hand: it is one structure that has not changed
/// since Windows 95, and the crate feature that would provide it also links
/// the OpenGL entry points, which this file deliberately resolves itself.
#[repr(C)]
#[derive(Default)]
struct PixelFormat {
    size: u16,
    version: u16,
    flags: u32,
    pixel_type: u8,
    color_bits: u8,
    red_bits: u8,
    red_shift: u8,
    green_bits: u8,
    green_shift: u8,
    blue_bits: u8,
    blue_shift: u8,
    alpha_bits: u8,
    alpha_shift: u8,
    accum_bits: u8,
    accum_red_bits: u8,
    accum_green_bits: u8,
    accum_blue_bits: u8,
    accum_alpha_bits: u8,
    depth_bits: u8,
    stencil_bits: u8,
    aux_buffers: u8,
    layer_type: u8,
    reserved: u8,
    layer_mask: u32,
    visible_mask: u32,
    damage_mask: u32,
}

const PFD_DRAW_TO_WINDOW: u32 = 0x04;
const PFD_SUPPORT_OPENGL: u32 = 0x20;
const PFD_DOUBLEBUFFER: u32 = 0x01;

const GL_VENDOR: u32 = 0x1F00;
const GL_RENDERER: u32 = 0x1F01;
const GL_VERSION: u32 = 0x1F02;

/// What the machine's own OpenGL is, asked before there is a window.
///
/// The system library by full path, never by name: where the software renderer
/// is deployed the name means Mesa in this process, and the question here is
/// what the machine would offer without it. Loaded for the length of the
/// question and given back.
///
/// It also fills the gap in the log. When the graphics are too old the window
/// fails before any of this program's own code runs, so the line that usually
/// says what they are is never written; this is where it comes from instead.
///
/// `None` means OpenGL could not be brought up at all, which is a different
/// and worse answer than an old version.
pub fn probe_opengl() -> Option<super::GlReport> {
    use std::os::windows::ffi::OsStrExt;

    let path = system_opengl()?;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let module = unsafe {
        LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH)
    };
    let wgl = Wgl::from_module(module)?;
    let report = probe_with(&wgl, module);
    // The reference this took is its own. What keeps the library mapped for
    // the rest of the process is the executable's import table, which is not
    // this function's to drop.
    unsafe { FreeLibrary(module) };
    report
}

/// The window and the context, made and taken down around one question.
fn probe_with(wgl: &Wgl, module: *mut c_void) -> Option<super::GlReport> {
    unsafe {
        // `STATIC` is a class the system has already registered, which saves
        // registering and unregistering one of our own for a window that
        // exists for a hundredth of a second. No `WS_VISIBLE`: nothing is ever
        // meant to appear, and a style of zero is `WS_OVERLAPPED`.
        let class = wide("STATIC");
        let title = wide("");
        let window = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            1,
            1,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        if window.is_null() {
            return None;
        }
        let answer = probe_through(window, wgl, module);
        DestroyWindow(window);
        answer
    }
}

/// The middle of `probe_with`, split out so that every way of failing still
/// gives the device context and the context back.
unsafe fn probe_through(
    window: *mut c_void,
    wgl: &Wgl,
    module: *mut c_void,
) -> Option<super::GlReport> {
    let dc = GetDC(window);
    if dc.is_null() {
        return None;
    }
    let want = PixelFormat {
        size: std::mem::size_of::<PixelFormat>() as u16,
        version: 1,
        flags: PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER,
        // PFD_TYPE_RGBA, and the only one anything has used this century.
        pixel_type: 0,
        color_bits: 32,
        ..Default::default()
    };
    let mut answer = None;
    let format = ChoosePixelFormat(dc, &want);
    if format != 0 && SetPixelFormat(dc, format, &want) != 0 {
        let context = (wgl.create_context)(dc);
        if !context.is_null() {
            if (wgl.make_current)(dc, context) != 0 {
                // Out of the module under test, not out of this process's own
                // OpenGL: those are different libraries whenever the software
                // renderer is deployed, and asking the wrong one would
                // describe the wrong machine.
                let get_string: Option<unsafe extern "system" fn(u32) -> *const u8> = {
                    let found = GetProcAddress(module, b"glGetString\0".as_ptr());
                    if found.is_null() {
                        None
                    } else {
                        Some(std::mem::transmute(found))
                    }
                };
                if let Some(get_string) = get_string {
                    let read = |name: u32| {
                        let text = get_string(name);
                        if text.is_null() {
                            // A driver that will not say. All three of these
                            // have been in OpenGL since 1.0, so this is a
                            // refusal rather than an old version, and one
                            // missing word should not lose the other two.
                            "unknown".to_string()
                        } else {
                            std::ffi::CStr::from_ptr(text as *const c_char)
                                .to_string_lossy()
                                .into_owned()
                        }
                    };
                    let version = read(GL_VERSION);
                    // "1.1.0", "4.6.0 NVIDIA 551.23", "4.5 (Core Profile) Mesa
                    // 24.0" — the major number is the leading digits of the
                    // first word. Anything unreadable is taken as modern: a
                    // version string nobody here has seen is far likelier to be
                    // a graphics card than the software renderer from 1996, and
                    // guessing the other way would put a working machine on
                    // llvmpipe.
                    let major = version
                        .split(['.', ' '])
                        .next()
                        .and_then(|n| n.parse::<u32>().ok())
                        .unwrap_or(u32::MAX);
                    // The version is what the driver claims; this is what it
                    // can do. egui compiles a shader before it draws anything,
                    // and `glCreateShader` arrived with OpenGL 2.0. WGL answers
                    // "no such function" with these four as well as with null —
                    // see `gl_proc_address`, which has known it for longer.
                    let shaders = {
                        let found = (wgl.get_proc_address)(b"glCreateShader\0".as_ptr()) as usize;
                        ![0usize, 1, 2, 3, usize::MAX].contains(&found)
                    };
                    answer = Some(super::GlReport {
                        identity: format!("{} · {} · {}", read(GL_VENDOR), read(GL_RENDERER), version),
                        major,
                        shaders,
                    });
                }
                (wgl.make_current)(std::ptr::null_mut(), std::ptr::null_mut());
            }
            (wgl.delete_context)(context);
        }
    }
    ReleaseDC(window, dc);
    answer
}

/// Say something to somebody who has no console and no window.
///
/// The one message this exists for is the graphics failing to come up, which
/// is the only fault that can happen before there is an interface to report it
/// in: a release build is `windows_subsystem = "windows"`, so the error that
/// would otherwise be printed goes to a handle nobody is holding, and the
/// program looks like it did nothing at all.
pub fn alert(title: &str, body: &str) {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_OK, MB_SETFOREGROUND,
    };

    let title = wide(title);
    let body = wide(body);
    unsafe {
        MessageBoxW(
            HWND(std::ptr::null_mut()),
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}

// --- a second GL context, for the render thread ------------------------------

/// The interface's WGL context and the device context it belongs to,
/// captured on the interface thread.
///
/// Handles as integers because this crosses a thread boundary. The third is
/// `wglCreateContextAttribsARB` if the driver has it: it must be asked for
/// through `wglGetProcAddress`, which only answers while a context is
/// current, so it has to be fetched here rather than on the worker.
pub struct GlShare {
    dc: usize,
    context: usize,
    create_attribs: usize,
}

/// A context of the worker's own, current on the worker's thread.
pub struct GlWorker {
    context: usize,
}

type CreateContextAttribs =
    unsafe extern "system" fn(*mut c_void, *mut c_void, *const i32) -> *mut c_void;

const WGL_CONTEXT_MAJOR_VERSION_ARB: i32 = 0x2091;
const WGL_CONTEXT_MINOR_VERSION_ARB: i32 = 0x2092;

/// Capture the interface's context. Interface thread only, where it is
/// current.
pub fn gl_share() -> Option<GlShare> {
    let wgl = Wgl::get()?;
    unsafe {
        let dc = (wgl.get_current_dc)();
        let context = (wgl.get_current_context)();
        if dc.is_null() || context.is_null() {
            return None;
        }
        // Asked for here because it can only be asked for here. A null answer
        // is fine: `gl_worker_begin` falls back to the older pair of calls.
        let create_attribs = (wgl.get_proc_address)(b"wglCreateContextAttribsARB\0".as_ptr());
        Some(GlShare {
            dc: dc as usize,
            context: context as usize,
            create_attribs: create_attribs as usize,
        })
    }
}

/// A sibling of that context, made current on this thread.
///
/// Two ways to get one, and the modern way is tried first because it is the
/// one with no ordering trap in it: `wglCreateContextAttribsARB` takes the
/// context to share with as an argument, so the sharing is part of creating
/// it. The fallback creates a context and then calls `wglShareLists`, which
/// is only lawful while the new context has no objects of its own — true
/// here, since it has just been made and nothing has touched it.
pub fn gl_worker_begin(share: &GlShare) -> Option<GlWorker> {
    let wgl = Wgl::get()?;
    unsafe {
        let dc = share.dc as *mut c_void;
        let source = share.context as *mut c_void;

        let context = if share.create_attribs != 0 {
            let create: CreateContextAttribs = std::mem::transmute(share.create_attribs);
            let attribs = [
                WGL_CONTEXT_MAJOR_VERSION_ARB, 3,
                WGL_CONTEXT_MINOR_VERSION_ARB, 2,
                0,
            ];
            create(dc, source, attribs.as_ptr())
        } else {
            let fresh = (wgl.create_context)(dc);
            if !fresh.is_null() && (wgl.share_lists)(source, fresh) == 0 {
                (wgl.delete_context)(fresh);
                return None;
            }
            fresh
        };

        if context.is_null() {
            return None;
        }
        if (wgl.make_current)(dc, context) == 0 {
            (wgl.delete_context)(context);
            return None;
        }
        Some(GlWorker {
            context: context as usize,
        })
    }
}

/// Give it back, on the thread that owns it.
pub fn gl_worker_end(worker: GlWorker) {
    let Some(wgl) = Wgl::get() else { return };
    unsafe {
        (wgl.make_current)(std::ptr::null_mut(), std::ptr::null_mut());
        (wgl.delete_context)(worker.context as *mut c_void);
    }
}

/// How mpv finds the OpenGL functions of the context eframe created.
///
/// `wglGetProcAddress` answers only for OpenGL 1.2 and later; everything from
/// 1.1 lives in `opengl32.dll` itself and has to be looked up there. mpv asks
/// for both kinds, so a loader that consults only one of them fails at
/// context creation with nothing useful to say about why.
pub unsafe extern "C" fn gl_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    let Some(wgl) = Wgl::get() else {
        return std::ptr::null_mut();
    };
    let found = (wgl.get_proc_address)(name as *const u8);
    // These four are what WGL returns for "no such function", rather than null.
    let bad = [0usize, 1, 2, 3, usize::MAX];
    if !bad.contains(&(found as usize)) {
        return found;
    }
    let module = opengl32();
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
