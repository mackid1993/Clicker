// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native client for Channels DVR
// Copyright (c) 2026 David Brustein

//! Linux. Written to the XDG conventions and the two GL worlds — EGL under
//! Wayland, GLX under X11 — without trying to guess which one the session is;
//! the loader asks both.

use std::ffi::{c_char, c_int, c_void};
use std::path::PathBuf;

// --- window chrome -----------------------------------------------------------

/// The compositor's frame, not ours.
///
/// This was the other way round to begin with, on the reasoning that the free
/// desktops draw their own headers anyway. They do — but *they* do it, from
/// inside the toolkit, in cooperation with the compositor. An undecorated
/// window that provides its own buttons and its own resize edges depends on
/// asking the compositor to take over a drag, and what that costs is not
/// uniform: X11 and Wayland differ, GNOME and KDE differ, and a window whose
/// buttons do nothing or which cannot be resized is not a stylistic
/// disappointment, it is a broken window.
///
/// So the system decorates it. The title bar sits above a surface that is
/// otherwise entirely ours, which is what every other application on that
/// desktop looks like, and the buttons are the ones the user's own theme
/// draws — working, in the place they expect, on every compositor.
pub const NATIVE_FRAME: bool = true;

/// The frame is above the surface rather than over it, so nothing has to be
/// kept clear the way the traffic lights do on a Mac.
pub const CAPTION_INSET: f32 = 0.0;

/// A decorated window, with the interior ours.
pub fn shape_window(viewport: eframe::egui::ViewportBuilder) -> eframe::egui::ViewportBuilder {
    viewport.with_decorations(true)
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
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

/// Nothing to keep in step: there is no menu bar here, so a rebinding
/// changes the settings page and nothing else.
pub fn sync_menu_shortcuts(_settings: &crate::settings::Settings) {}

/// Never anything, there being no menu to ask from.
pub fn menu_command() -> Option<String> {
    None
}

// --- where files go ----------------------------------------------------------

fn home() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?))
}

fn xdg(variable: &str, fallback: &[&str]) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(variable) {
        let dir = PathBuf::from(dir);
        if dir.is_absolute() {
            return Some(dir);
        }
    }
    let mut dir = home()?;
    for part in fallback {
        dir = dir.join(part);
    }
    Some(dir)
}

/// Settings, under `XDG_CONFIG_HOME`.
pub fn config_home() -> Option<PathBuf> {
    xdg("XDG_CONFIG_HOME", &[".config"])
}

/// Downloads, caches, buffers and logs, under `XDG_DATA_HOME` rather than
/// `XDG_CACHE_HOME`: offline downloads are whole recordings someone means to
/// keep, and the cache directory is fair game for every cleanup tool.
pub fn data_home() -> Option<PathBuf> {
    xdg("XDG_DATA_HOME", &[".local", "share"])
}

// --- fonts -------------------------------------------------------------------

/// egui's bundled face, by returning nothing. A Linux desktop has no one
/// system font to read the way Windows has Segoe and macOS has San
/// Francisco, and guessing at distribution font paths buys inconsistency.
pub fn text_font() -> Option<Vec<u8>> {
    None
}

/// A face to fall back on for the glyphs egui's own font has never heard of.
///
/// Windows and macOS answer this with their system face: Segoe and San
/// Francisco carry arrows, dashes and the rest of the punctuation an
/// interface reaches for, so `text_font` above already covers it there. On
/// Linux nothing does — egui's bundled Ubuntu-Light stops not far past Latin
/// — and `0:00 → 0:42` in the stats card drew `0:00 □ 0:42`.
///
/// So the question is put to fontconfig, which is the one piece of this that
/// every desktop Linux agrees on, and it is put as "a sans face that contains
/// this arrow" rather than as a font name: `charset=2192` is answered from
/// what the machine actually has installed, which is DejaVu on Debian and
/// Ubuntu, Noto on Fedora, Liberation where neither is. Guessing at paths is
/// what this used to refuse to do, and the refusal was right — asking is not
/// the same as guessing.
///
/// The path list underneath is for a system with no `fc-match` at all, which
/// is rare enough that being approximately right there is fine.
pub fn fallback_font() -> Option<Vec<u8>> {
    let asked = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", "sans:charset=2192"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|path| !path.is_empty());

    let candidates = asked.into_iter().chain(
        [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            "/usr/share/fonts/liberation-sans/LiberationSans-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
        ]
        .into_iter()
        .map(str::to_string),
    );

    for path in candidates {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        // Checked before it is handed on, because the thing that parses it
        // panics rather than declines: epaint's loader unwraps, and this is
        // the only place in the program where bytes chosen by another program
        // reach a parser. fontconfig can legitimately answer with a bitmap
        // font, a Type 1, or a collection, and ab_glyph reads none of those.
        // A window that never opens is a far worse outcome than a box where
        // an arrow should be, so anything that is not a plain sfnt is passed
        // over for the next candidate.
        let magic = bytes.get(..4).unwrap_or_default();
        if !matches!(magic, [0x00, 0x01, 0x00, 0x00] | b"OTTO" | b"true") {
            crate::log::line(&format!(
                "[clicker] not a font this can read, passing over it: {path}"
            ));
            continue;
        }
        crate::log::line(&format!("[clicker] fallback font: {path}"));
        return Some(bytes);
    }
    crate::log::line("[clicker] no fallback font: some glyphs will draw as boxes");
    None
}

/// The bundled Fluent UI System Icons subset — Microsoft's own icon set,
/// MIT-licensed, cut down to the twenty-eight glyphs the interface draws.
/// See `theme::icon` for the codepoint table it must stay in step with, and
/// `licenses/FluentSystemIcons-MIT.txt` for its terms.
pub fn icon_font() -> Option<Vec<u8>> {
    Some(include_bytes!("../../assets/FluentIcons-Clicker.ttf").to_vec())
}

// --- a second GL context, for the render thread ------------------------------

/// The interface's EGL context, captured on the interface thread.
///
/// Pointers as integers because this crosses a thread boundary, and a raw
/// pointer is not `Send` for reasons that do not apply to a handle the driver
/// hands back and expects to see again.
pub struct GlShare {
    display: usize,
    context: usize,
}

/// A context of the worker's own, current on the worker's thread.
pub struct GlWorker {
    display: usize,
    context: usize,
}

const EGL_OPENGL_API: u32 = 0x30A2;
const EGL_NONE: i32 = 0x3038;
const EGL_CONTEXT_MAJOR_VERSION: i32 = 0x3098;
const EGL_CONTEXT_MINOR_VERSION: i32 = 0x30FB;

struct Egl {
    get_current_display: unsafe extern "C" fn() -> *mut c_void,
    get_current_context: unsafe extern "C" fn() -> *mut c_void,
    bind_api: unsafe extern "C" fn(u32) -> u32,
    create_context:
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *const i32) -> *mut c_void,
    make_current:
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> u32,
    destroy_context: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32,
    release_thread: unsafe extern "C" fn() -> u32,
}

fn egl() -> Option<&'static Egl> {
    static EGL: std::sync::OnceLock<Option<Egl>> = std::sync::OnceLock::new();
    EGL.get_or_init(|| {
        let lib = super::open_library("libEGL.so.1");
        if lib.is_null() {
            return None;
        }
        unsafe {
            macro_rules! sym {
                ($name:literal) => {{
                    let p = super::library_symbol(lib, concat!($name, "\0").as_ptr());
                    if p.is_null() {
                        return None;
                    }
                    std::mem::transmute(p)
                }};
            }
            Some(Egl {
                get_current_display: sym!("eglGetCurrentDisplay"),
                get_current_context: sym!("eglGetCurrentContext"),
                bind_api: sym!("eglBindAPI"),
                create_context: sym!("eglCreateContext"),
                make_current: sym!("eglMakeCurrent"),
                destroy_context: sym!("eglDestroyContext"),
                release_thread: sym!("eglReleaseThread"),
            })
        }
    })
    .as_ref()
}

/// Capture the interface's context. Interface thread only, where it is
/// current. `None` where the session is not EGL at all — an X11 session on
/// GLX is the case — and the caller falls back to rendering in the paint.
pub fn gl_share() -> Option<GlShare> {
    let egl = egl()?;
    let (display, context) = unsafe { ((egl.get_current_display)(), (egl.get_current_context)()) };
    if display.is_null() || context.is_null() {
        return None;
    }
    Some(GlShare {
        display: display as usize,
        context: context as usize,
    })
}

/// A sibling of that context, made current on this thread.
///
/// No config and no surface, which Mesa allows and which is all a renderer
/// that only ever draws into framebuffers needs. Same share group, so the
/// textures made here are the ones the interface blits.
pub fn gl_worker_begin(share: &GlShare) -> Option<GlWorker> {
    let egl = egl()?;
    let display = share.display as *mut c_void;
    unsafe {
        (egl.bind_api)(EGL_OPENGL_API);
        let attribs = [
            EGL_CONTEXT_MAJOR_VERSION, 3,
            EGL_CONTEXT_MINOR_VERSION, 0,
            EGL_NONE,
        ];
        let ctx = (egl.create_context)(
            display,
            std::ptr::null_mut(),
            share.context as *mut c_void,
            attribs.as_ptr(),
        );
        if ctx.is_null() {
            return None;
        }
        if (egl.make_current)(display, std::ptr::null_mut(), std::ptr::null_mut(), ctx) == 0 {
            (egl.destroy_context)(display, ctx);
            return None;
        }
        Some(GlWorker {
            display: share.display,
            context: ctx as usize,
        })
    }
}

/// Give it back, on the thread that owns it.
pub fn gl_worker_end(worker: GlWorker) {
    let Some(egl) = egl() else { return };
    unsafe {
        let display = worker.display as *mut c_void;
        (egl.make_current)(display, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
        (egl.destroy_context)(display, worker.context as *mut c_void);
        (egl.release_thread)();
    }
}

// --- the live buffer's disk tricks -------------------------------------------

/// Nothing to do: every Linux filesystem this will meet — ext4, btrfs, xfs —
/// keeps files sparse by construction.
pub fn make_sparse(_file: &tokio::fs::File) {}

extern "C" {
    fn fallocate(fd: c_int, mode: c_int, offset: i64, len: i64) -> c_int;
    fn getrlimit(resource: c_int, rlim: *mut Rlimit) -> c_int;
    fn setrlimit(resource: c_int, rlim: *const Rlimit) -> c_int;
}

#[repr(C)]
struct Rlimit {
    cur: u64,
    max: u64,
}

const RLIMIT_NOFILE: c_int = 7;

/// Raise the file-descriptor ceiling to the hard limit.
///
/// Measured with 1024 of 1024 descriptors open one minute into playback, 958
/// of them `anon_inode:sync_file` — GPU fence descriptors the virtio graphics
/// driver exports every frame and never closes. The leak is the driver's, but
/// the death is ours: at the ceiling, sockets, images and the audio device
/// all fail at once and the application appears to have a stroke. Chromium
/// raises this limit at startup for the same class of reason; the hard limit
/// on a systemd desktop is around a million, which turns a one-minute cliff
/// into a horizon nobody meets.
pub fn raise_fd_limit() {
    unsafe {
        let mut limit = Rlimit { cur: 0, max: 0 };
        if getrlimit(RLIMIT_NOFILE, &mut limit) != 0 || limit.cur >= limit.max {
            return;
        }
        let was = limit.cur;
        limit.cur = limit.max;
        if setrlimit(RLIMIT_NOFILE, &limit) == 0 {
            crate::log::line(&format!(
                "[clicker] file descriptor limit raised {was} -> {}",
                limit.max
            ));
        }
    }
}

const FALLOC_FL_KEEP_SIZE: c_int = 0x01;
const FALLOC_FL_PUNCH_HOLE: c_int = 0x02;

/// Give a range at the front of the buffer back to the disk.
///
/// `fallocate` takes byte offsets and rounds to blocks itself — partial
/// blocks are zeroed, whole blocks released — so unlike the macOS call there
/// is no alignment to do here. KEEP_SIZE because the demuxer holds byte
/// offsets into this file and the file must not shrink under it.
pub fn punch_hole(file: &tokio::fs::File, from: u64, len: u64) -> bool {
    use std::os::unix::io::AsRawFd;

    if len == 0 {
        return false;
    }
    unsafe {
        fallocate(
            file.as_raw_fd(),
            FALLOC_FL_PUNCH_HOLE | FALLOC_FL_KEEP_SIZE,
            from as i64,
            len as i64,
        ) == 0
    }
}

// --- libmpv and OpenGL -------------------------------------------------------

/// What the mpv library is called here, for messages about not finding it.
pub const MPV_LIBRARY: &str = "libmpv.so.2";

/// Where libmpv might be, and deliberately nowhere else.
///
/// A `bin`/`lib` split first — which is a Flatpak, and was the AppImage
/// before it — then beside the binary, which is where `make install` and the
/// .deb both put them, then the repository's staged build for anyone running
/// this out of `cargo`.
/// What is *not* here is the system loader's search path: a distribution's
/// FFmpeg is frequently built with GPL components, and this application ships
/// under MIT with an LGPL player. The only libmpv it loads is one built by
/// `scripts/build-mpv.sh` with `-Dgpl=false` and `--disable-gpl`.
pub fn mpv_candidates() -> Vec<String> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from));
    let mut candidates = Vec::new();
    if let Some(dir) = &exe_dir {
        // usr/bin/../lib, for a layout that splits the two.
        candidates.push(dir.join("../lib").join(MPV_LIBRARY).display().to_string());
        candidates.push(dir.join(MPV_LIBRARY).display().to_string());
        candidates.push(
            dir.join("../../third_party/mpv")
                .join(MPV_LIBRARY)
                .display()
                .to_string(),
        );
    }
    candidates
}

/// A GL loader function, resolved out of a library once.
fn loader(library: &str, symbol: &[u8]) -> Option<unsafe extern "C" fn(*const c_char) -> *mut c_void> {
    let module = super::open_library(library);
    if module.is_null() {
        return None;
    }
    let found = unsafe { super::library_symbol(module, symbol.as_ptr()) };
    if found.is_null() {
        return None;
    }
    Some(unsafe { std::mem::transmute(found) })
}

/// How mpv finds the OpenGL functions of the context eframe created.
///
/// The session decides whether that context is EGL or GLX, and this does not
/// know which, so it asks in order: `eglGetProcAddress`, `glXGetProcAddressARB`,
/// then a plain `dlsym` into libGL for the 1.1 entry points the older
/// loaders decline to return.
pub unsafe extern "C" fn gl_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    type Loader = unsafe extern "C" fn(*const c_char) -> *mut c_void;
    static LOADERS: std::sync::OnceLock<(Option<Loader>, Option<Loader>, usize)> =
        std::sync::OnceLock::new();
    let (egl, glx, libgl) = *LOADERS.get_or_init(|| {
        (
            loader("libEGL.so.1", b"eglGetProcAddress\0"),
            loader("libGL.so.1", b"glXGetProcAddressARB\0"),
            super::open_library("libGL.so.1") as usize,
        )
    });

    if let Some(egl) = egl {
        let found = egl(name);
        if !found.is_null() {
            return found;
        }
    }
    if let Some(glx) = glx {
        let found = glx(name);
        if !found.is_null() {
            return found;
        }
    }
    let libgl = libgl as *mut c_void;
    if libgl.is_null() {
        return std::ptr::null_mut();
    }
    super::library_symbol(libgl, name as *const u8)
}

