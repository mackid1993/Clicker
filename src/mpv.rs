// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native Windows client for Channels DVR
// Copyright (c) 2026 David Brustein

//! Playback through libmpv.
//!
//! The player. Not one of two: everything Clicker plays comes through here,
//! and there is no setting that turns it off. There used to be a hand-rolled
//! pipeline over FFmpeg beside it, and it was faster, but it was one person's
//! implementation of a problem mpv has been having solved for twenty years —
//! timestamp discontinuities, damaged segments, odd containers, streams that
//! stop and start — and every one of those arrives as a bug report about a
//! file nobody here can reproduce.
//!
//! **The picture never leaves the graphics chip.** mpv decodes into a
//! framebuffer this owns and the interface blits from it, so there is no
//! readback, no conversion on the processor, and no upload. The alternative is
//! mpv's software renderer, whose own header calls it "an extremely simple
//! (but slow) renderer... You probably don't want to use this", and measurement
//! agreed: on one 1080p60 recording it cost 0.70 of a core against 0.08 here,
//! because hardware decoding into a software renderer means the frame crosses
//! the bus three times to arrive where it started. That is why eframe is on
//! glow rather than wgpu — mpv renders through OpenGL or not at all.
//!
//! Two threads, and which one does what matters:
//!   * **the interface thread** owns the OpenGL context, so it creates the
//!     renderer, loads the file, and draws — see `start` and `present`
//!   * **the event thread** does nothing but wait on mpv and report
//!
//! Drawing is paced by mpv, through `on_frame_ready`, rather than by a timer.
//!
//! The library is loaded by name at runtime rather than linked — through
//! `platform`, which knows what it is called and how to open it on each
//! system. The habit started on Windows out of necessity: mpv cannot be built
//! with MSVC, so the DLL comes from mingw with an import library an MSVC
//! target cannot use. It turned out to be the porting seam too, and it means
//! a missing library is a message rather than a program that will not start.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::stream::{JoinAt, Transport};

// --- the slice of the C API this needs ---------------------------------------

const MPV_FORMAT_STRING: c_int = 1;
const MPV_FORMAT_FLAG: c_int = 3;
const MPV_FORMAT_INT64: c_int = 4;
const MPV_FORMAT_DOUBLE: c_int = 5;

const MPV_EVENT_NONE: c_int = 0;
const MPV_EVENT_SHUTDOWN: c_int = 1;
const MPV_EVENT_LOG_MESSAGE: c_int = 2;
const MPV_EVENT_END_FILE: c_int = 7;
const MPV_EVENT_FILE_LOADED: c_int = 8;
/// The picture's size became known, or changed. Live television changes it
/// mid-stream more often than anyone expects.
const MPV_EVENT_VIDEO_RECONFIG: c_int = 17;

const MPV_RENDER_PARAM_INVALID: c_int = 0;
const MPV_RENDER_PARAM_API_TYPE: c_int = 1;
const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
const MPV_RENDER_PARAM_OPENGL_FBO: c_int = 3;
const MPV_RENDER_PARAM_FLIP_Y: c_int = 4;
/// Whether `render` waits until the frame is due before returning. It defaults
/// to waiting, which is right for a thread that does nothing else and very
/// wrong for the one drawing the interface.
const MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME: c_int = 11;

const MPV_RENDER_UPDATE_FRAME: u64 = 1;

#[repr(C)]
struct OpenGlInitParams {
    get_proc_address: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
struct OpenGlFbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
}

#[repr(C)]
struct RenderParam {
    kind: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct Event {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

#[repr(C)]
struct LogMessage {
    prefix: *const c_char,
    level: *const c_char,
    text: *const c_char,
    log_level: c_int,
}

// How mpv finds the OpenGL functions of the context eframe created, how this
// program finds mpv itself, and how a thread's processor time is read all
// live in `platform`: they are the three places this file used to speak
// Win32, and the three places a port has anything to say.
use crate::platform::{gl_proc_address, thread_cpu_ms};

macro_rules! api {
    ($($field:ident: $name:literal => fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        pub struct Api { $($field: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)* }
        impl Api {
            unsafe fn load(module: *mut c_void) -> Result<Self, String> {
                $(
                    let $field =
                        crate::platform::library_symbol(module, concat!($name, "\0").as_ptr());
                    if $field.is_null() {
                        return Err(format!("{} is missing from libmpv", $name));
                    }
                )*
                Ok(Self { $($field: std::mem::transmute($field),)* })
            }
        }
    };
}

api! {
    create: "mpv_create" => fn() -> *mut c_void;
    initialize: "mpv_initialize" => fn(*mut c_void) -> c_int;
    terminate: "mpv_terminate_destroy" => fn(*mut c_void);
    set_option: "mpv_set_option_string" => fn(*mut c_void, *const c_char, *const c_char) -> c_int;
    command: "mpv_command" => fn(*mut c_void, *const *const c_char) -> c_int;
    wait_event: "mpv_wait_event" => fn(*mut c_void, f64) -> *mut Event;
    get_property: "mpv_get_property" => fn(*mut c_void, *const c_char, c_int, *mut c_void) -> c_int;
    set_property: "mpv_set_property" => fn(*mut c_void, *const c_char, c_int, *mut c_void) -> c_int;
    free: "mpv_free" => fn(*mut c_void);
    request_log: "mpv_request_log_messages" => fn(*mut c_void, *const c_char) -> c_int;
    error_string: "mpv_error_string" => fn(c_int) -> *const c_char;
    render_create: "mpv_render_context_create" => fn(*mut *mut c_void, *mut c_void, *mut RenderParam) -> c_int;
    render_set_update_callback: "mpv_render_context_set_update_callback" => fn(*mut c_void, unsafe extern "C" fn(*mut c_void), *mut c_void);
    render_report_swap: "mpv_render_context_report_swap" => fn(*mut c_void);
    render_update: "mpv_render_context_update" => fn(*mut c_void) -> u64;
    render: "mpv_render_context_render" => fn(*mut c_void, *mut RenderParam) -> c_int;
    render_free: "mpv_render_context_free" => fn(*mut c_void);
}

// --- the slice of OpenGL this needs ------------------------------------------
//
// Loaded by hand rather than through glow. eframe owns a `glow::Context` and
// will lend it out, but mpv needs raw function pointers for its own loader
// anyway, and the eight calls below are all that is required to own one
// framebuffer. Adding a second way to reach OpenGL for the sake of eight
// functions is not a saving.

const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_RGBA8: i32 = 0x8058;
const GL_RGBA: u32 = 0x1908;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_LINEAR: i32 = 0x2601;
const GL_CLAMP_TO_EDGE: i32 = 0x812F;
const GL_TEXTURE_MAG_FILTER: u32 = 0x2800;
const GL_TEXTURE_MIN_FILTER: u32 = 0x2801;
const GL_TEXTURE_WRAP_S: u32 = 0x2802;
const GL_TEXTURE_WRAP_T: u32 = 0x2803;
const GL_FRAMEBUFFER: u32 = 0x8D40;
const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
const GL_DRAW_FRAMEBUFFER_BINDING: u32 = 0x8CA6;

/// The OpenGL entry points, resolved once against the current context.
pub struct GlFns {
    gen_textures: unsafe extern "system" fn(i32, *mut u32),
    delete_textures: unsafe extern "system" fn(i32, *const u32),
    bind_texture: unsafe extern "system" fn(u32, u32),
    tex_image_2d:
        unsafe extern "system" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void),
    tex_parameteri: unsafe extern "system" fn(u32, u32, i32),
    gen_framebuffers: unsafe extern "system" fn(i32, *mut u32),
    delete_framebuffers: unsafe extern "system" fn(i32, *const u32),
    bind_framebuffer: unsafe extern "system" fn(u32, u32),
    framebuffer_texture_2d: unsafe extern "system" fn(u32, u32, u32, u32, i32),
    check_framebuffer_status: unsafe extern "system" fn(u32) -> u32,
    get_integerv: unsafe extern "system" fn(u32, *mut i32),
    blit_framebuffer:
        unsafe extern "system" fn(i32, i32, i32, i32, i32, i32, i32, i32, u32, u32),
    pixel_storei: unsafe extern "system" fn(u32, i32),
    bind_buffer: unsafe extern "system" fn(u32, u32),
    enable: unsafe extern "system" fn(u32),
    disable: unsafe extern "system" fn(u32),
    is_enabled: unsafe extern "system" fn(u32) -> u8,
    flush: unsafe extern "system" fn(),
    // How the render thread hands a frame over: it finishes, then publishes.
    // A GL fence was tried first and is the reason the thread was parked for
    // a while — see the worker, which explains why it is not one.
    finish: unsafe extern "system" fn(),
}

impl GlFns {
    /// Resolve them, or say which one is missing.
    ///
    /// # Safety
    ///
    /// An OpenGL context must be current on the calling thread.
    pub unsafe fn load() -> Result<Self, String> {
        unsafe fn find<T>(name: &str) -> Result<T, String> {
            let c = CString::new(name).map_err(|_| name.to_string())?;
            let p = gl_proc_address(std::ptr::null_mut(), c.as_ptr());
            if p.is_null() {
                return Err(format!("OpenGL is missing {name}"));
            }
            Ok(std::mem::transmute_copy::<*mut c_void, T>(&p))
        }
        Ok(Self {
            gen_textures: find("glGenTextures")?,
            delete_textures: find("glDeleteTextures")?,
            bind_texture: find("glBindTexture")?,
            tex_image_2d: find("glTexImage2D")?,
            tex_parameteri: find("glTexParameteri")?,
            gen_framebuffers: find("glGenFramebuffers")?,
            delete_framebuffers: find("glDeleteFramebuffers")?,
            bind_framebuffer: find("glBindFramebuffer")?,
            framebuffer_texture_2d: find("glFramebufferTexture2D")?,
            check_framebuffer_status: find("glCheckFramebufferStatus")?,
            get_integerv: find("glGetIntegerv")?,
            blit_framebuffer: find("glBlitFramebuffer")?,
            pixel_storei: find("glPixelStorei")?,
            bind_buffer: find("glBindBuffer")?,
            enable: find("glEnable")?,
            disable: find("glDisable")?,
            is_enabled: find("glIsEnabled")?,
            flush: find("glFlush")?,
            finish: find("glFinish")?,
        })
    }

    unsafe fn gen_textures(&self, n: i32, out: *mut u32) {
        (self.gen_textures)(n, out)
    }
    unsafe fn delete_textures(&self, n: i32, ids: *const u32) {
        (self.delete_textures)(n, ids)
    }
    unsafe fn bind_texture(&self, target: u32, id: u32) {
        (self.bind_texture)(target, id)
    }
    #[allow(clippy::too_many_arguments)]
    unsafe fn tex_image_2d(
        &self,
        target: u32,
        level: i32,
        internal: i32,
        w: i32,
        h: i32,
        border: i32,
        format: u32,
        kind: u32,
        pixels: *const c_void,
    ) {
        (self.tex_image_2d)(target, level, internal, w, h, border, format, kind, pixels)
    }
    unsafe fn tex_parameteri(&self, target: u32, name: u32, value: i32) {
        (self.tex_parameteri)(target, name, value)
    }
    unsafe fn gen_framebuffers(&self, n: i32, out: *mut u32) {
        (self.gen_framebuffers)(n, out)
    }
    unsafe fn delete_framebuffers(&self, n: i32, ids: *const u32) {
        (self.delete_framebuffers)(n, ids)
    }
    unsafe fn bind_framebuffer(&self, target: u32, id: u32) {
        (self.bind_framebuffer)(target, id)
    }
    unsafe fn framebuffer_texture_2d(
        &self,
        target: u32,
        attachment: u32,
        tex_target: u32,
        texture: u32,
        level: i32,
    ) {
        (self.framebuffer_texture_2d)(target, attachment, tex_target, texture, level)
    }
    unsafe fn check_framebuffer_status(&self, target: u32) -> u32 {
        (self.check_framebuffer_status)(target)
    }
    /// Whatever framebuffer is bound now, so it can be put back.
    ///
    /// mpv binds its own target and does not restore this, and egui is midway
    /// through drawing into one when it hands over. Leaving mpv's binding in
    /// place sends every widget after the video into the video's texture.
    unsafe fn draw_framebuffer(&self) -> u32 {
        let mut bound = 0i32;
        (self.get_integerv)(GL_DRAW_FRAMEBUFFER_BINDING, &mut bound);
        bound.max(0) as u32
    }
}

const GL_UNPACK_ALIGNMENT: u32 = 0x0CF5;
const GL_UNPACK_ROW_LENGTH: u32 = 0x0CF2;
const GL_UNPACK_SKIP_PIXELS: u32 = 0x0CF4;
const GL_UNPACK_SKIP_ROWS: u32 = 0x0CF3;
const GL_PIXEL_UNPACK_BUFFER: u32 = 0x88EC;
const GL_PIXEL_UNPACK_BUFFER_BINDING: u32 = 0x88EF;

const GL_READ_FRAMEBUFFER: u32 = 0x8CA8;
const GL_DRAW_FRAMEBUFFER: u32 = 0x8CA9;
const GL_COLOR_BUFFER_BIT: u32 = 0x0000_4000;
const GL_SCISSOR_TEST: u32 = 0x0C11;

/// A pointer that may cross to the render thread.
///
/// Sound because mpv's own handle is thread safe for properties and commands,
/// and because the render context is only ever touched by the one thread that
/// created it.
#[derive(Clone, Copy)]
struct Ptr(*mut c_void);
unsafe impl Send for Ptr {}
unsafe impl Sync for Ptr {}

/// Load the library once, from wherever the platform says it might be.
fn library() -> Result<&'static Api, String> {
    static LOADED: std::sync::OnceLock<Result<Api, String>> = std::sync::OnceLock::new();
    LOADED
        .get_or_init(|| {
            // Every attempt's reason is kept, because the interesting failure
            // is almost never "no such file". A bundled library that cannot
            // find its own dependencies fails here too, and reporting that as
            // "not found" sends everybody looking for a missing file that is
            // sitting right there.
            let mut refusals = Vec::new();
            for candidate in crate::platform::mpv_candidates() {
                let module = crate::platform::open_library(&candidate);
                if !module.is_null() {
                    return unsafe { Api::load(module) };
                }
                if let Some(why) = crate::platform::library_error() {
                    refusals.push(why);
                }
            }
            // Every one of them, not the last. The last candidate is the
            // development path, and reporting only that named a directory
            // nobody had shipped while the real failure — a bundled library
            // whose own dependency had moved on a newer distribution — sat
            // silently in the first refusal. Two of these are one line each;
            // this only ever appears when playback is already broken.
            let detail = if refusals.is_empty() {
                String::new()
            } else {
                format!(":\n{}", refusals.join("\n"))
            };
            Err(format!(
                "{} could not be loaded{detail}",
                crate::platform::MPV_LIBRARY
            ))
        })
        .as_ref()
        .map_err(|e| e.clone())
}


/// What is installed, read out of what is installed.
///
/// Both versions come from mpv itself rather than being written down here, so
/// the About line cannot claim a build that is not on disk. Asked once and
/// kept: it costs a throwaway handle, and the About panel repaints every frame.
fn versions(api: &Api) -> &'static str {
    static VERSIONS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSIONS.get_or_init(|| unsafe {
        let handle = (api.create)();
        if handle.is_null() {
            return "mpv".to_string();
        }
        // Initialized, but with no output of any kind. Both properties read
        // back empty on a handle that has only been created, and a core with
        // no video output, no audio output and nothing loaded costs a few
        // milliseconds once.
        for (name, value) in [("vo", "null"), ("ao", "null"), ("terminal", "no")]
            .into_iter()
            .chain(NO_SCRIPTS)
        {
            // A blank value means "leave the option alone", which is what the
            // environment overrides above resolve to when unset.
            if value.is_empty() {
                continue;
            }
            let (n, v) = (CString::new(name).unwrap(), CString::new(value).unwrap());
            (api.set_option)(handle, n.as_ptr(), v.as_ptr());
        }
        (api.initialize)(handle);

        let read = |name: &str| -> Option<String> {
            let key = CString::new(name).ok()?;
            let mut out: *mut c_char = std::ptr::null_mut();
            let rc = (api.get_property)(
                handle,
                key.as_ptr(),
                MPV_FORMAT_STRING,
                &mut out as *mut *mut c_char as *mut c_void,
            );
            if rc < 0 || out.is_null() {
                return None;
            }
            let text = CStr::from_ptr(out).to_string_lossy().into_owned();
            (api.free)(out as *mut c_void);
            Some(text)
        };
        let mpv = read("mpv-version").unwrap_or_else(|| "mpv".to_string());
        let ffmpeg = read("ffmpeg-version").unwrap_or_default();
        (api.terminate)(handle);
        if ffmpeg.is_empty() {
            format!("{mpv}, LGPL")
        } else {
            format!("{mpv} · FFmpeg {ffmpeg} · LGPL v2.1 or later")
        }
    })
}

struct Shared {
    quit: AtomicBool,
    /// Frames handed to the interface. Stands in for the decoder's own count:
    /// what the stall watchdog needs to know is whether pictures are still
    /// arriving, and this is that.
    rendered: AtomicU64,
    dropped: AtomicU64,
    error: Mutex<Option<String>>,
    /// What share of one core the renderer costs, smoothed, for the stats card.
    render_load: Mutex<f32>,
    /// How long `render` actually takes on the clock, smoothed.
    ///
    /// Processor time is the wrong measure for deciding whether the interface
    /// is being starved. On a machine where OpenGL is translated somewhere
    /// else — a virtual machine, a remote display — the call spends its time
    /// waiting rather than working: the processor figure stays low, the
    /// interface thread sits inside the call for tens of milliseconds, and
    /// the window stops answering the mouse while every counter says the
    /// renderer is healthy.
    render_ms: Mutex<f32>,
    /// Frames offered to the renderer, for the skipping below.
    offered: AtomicU64,
    /// Frames actually put on screen, and when that was last reported.
    ///
    /// The one number that settles an argument about playback. Render times,
    /// processor load and drop counters can all read healthy while half the
    /// frames never reach the glass, and every renderer theory this project
    /// has entertained was argued from one of those instead of from this.
    presented: AtomicU64,
    reported: Mutex<Option<Instant>>,
    last_reported: AtomicU64,
    last_painted: AtomicU64,
    /// When the last frame was drawn, so the figure above is per second of
    /// real time rather than per second of drawing.
    last_render: Mutex<Option<Instant>>,
    /// The stream's picture size, packed as `width << 32 | height`. Written by
    /// the event thread, read by the interface.
    size: AtomicU64,
    /// Ask the interface for a frame.
    ///
    /// Kept here because two different things need it: the event thread, and
    /// mpv's own "a frame is ready" callback, which is what actually paces
    /// playback now that drawing happens inside a paint callback.
    repaint: Box<dyn Fn() + Send + Sync>,
}

/// Whether this is the Flatpak build, running inside its sandbox.
///
/// The file is what Flatpak itself puts there for exactly this question, and
/// asking it costs one stat, once.
fn in_flatpak() -> bool {
    static INSIDE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *INSIDE.get_or_init(|| std::path::Path::new("/.flatpak-info").exists())
}

/// Where mpv is asked to draw its picture.
///
/// `Offscreen` is the arrangement this application is built around: mpv draws
/// into a framebuffer of its own and the interface blits that into whatever
/// egui is drawing into, which is what lets the controls, the guide and the
/// picture share one window and one pass.
///
/// `Window` hands mpv the window's own framebuffer instead — no offscreen
/// target, no blit — and with it the whole window, so the interface's own
/// drawing underneath is overwritten. That is not something to ship, and it
/// is the only way to weigh the offscreen target itself: `CLICKER_VIDEO=window`
/// against the default, on the machine in question, reading the frame rate
/// this file now logs.
#[derive(Clone, Copy, PartialEq)]
enum VideoTarget {
    Offscreen,
    Window,
}

fn video_target() -> VideoTarget {
    static CHOSEN: std::sync::OnceLock<VideoTarget> = std::sync::OnceLock::new();
    *CHOSEN.get_or_init(|| match std::env::var("CLICKER_VIDEO").as_deref() {
        Ok("window") => VideoTarget::Window,
        _ => VideoTarget::Offscreen,
    })
}

/// The built-in Lua scripts, every one of them turned off.
///
/// mpv ships nine, and every one is dead weight here: they draw an on-screen
/// interface this application already draws, and answer keys it already
/// handles. Nine fewer threads, and a core that starts sooner.
///
/// It is also the difference between running and not running on a signed
/// Mac. The scripts are executed by LuaJIT, which compiles Lua to machine
/// code at runtime, and macOS's hardened runtime kills any process that
/// executes a page it did not sign: `EXC_BAD_ACCESS (SIGKILL (Code Signature
/// Invalid))`, "Namespace CODESIGNING, Code 2, Invalid Page", on a thread
/// named after whichever script got there first. The alternative is the
/// `allow-jit` and `allow-unsigned-executable-memory` entitlements — asking
/// macOS to relax the one protection that matters so nine unwanted scripts
/// can run.
///
/// Two things this list learned the hard way:
///
///   * **Every script has to be named.** `load-scripts=no` is not the switch
///     it sounds like — it governs the *user's* scripts directory and leaves
///     the built-ins exactly where they were. It is kept anyway, so a stray
///     script in somebody's config cannot reintroduce the problem.
///   * **Every handle has to get the list**, not just the one that plays
///     video. `versions` creates a throwaway core to read the version out of
///     mpv, and a throwaway core loads the same scripts and dies in the same
///     way — three seconds into startup, before anything has been played.
const NO_SCRIPTS: [(&str, &str); 10] = [
    ("load-scripts", "no"),
    ("osc", "no"),
    ("ytdl", "no"),
    ("load-stats-overlay", "no"),
    ("load-console", "no"),
    ("load-auto-profiles", "no"),
    ("load-select", "no"),
    ("load-positioning", "no"),
    ("load-commands", "no"),
    ("load-context-menu", "no"),
];

/// mpv has a frame ready. Called from mpv's own thread, so it does the one
/// cheap thing it is allowed to do and returns.
///
/// Without this the picture advanced only when some *other* event caused a
/// repaint — a log line, a property change — which is a handful of times a
/// second rather than sixty. That is what "slow" looked like: the frames were
/// all being produced on time and almost none of them were being asked for.
unsafe extern "C" fn on_frame_ready(data: *mut c_void) {
    if data.is_null() {
        return;
    }
    let shared = &*(data as *const Shared);
    (shared.repaint)();
}

/// The OpenGL objects the picture lands in, all owned on the interface thread.
struct Surface {
    /// Two of everything, drawn into alternately.
    ///
    /// One texture reused every frame makes each new frame a write into the
    /// resource the previous frame's blit may still be reading on the GPU.
    /// A real driver hides that hazard; through virtio the driver answers it
    /// with a fence wait — DRM_IOCTL_VIRTGPU_WAIT, which is exactly where the
    /// paint thread was found parked while audio played on — and a wait per
    /// frame that costs several frames is a slideshow that ends in a black
    /// window. A window system double-buffers for precisely this reason; mpv
    /// in its own window inherits that, and rendering into our own texture
    /// forfeited it. So it is done by hand: draw into one while the other is
    /// being read, and never make the driver wait.
    bufs: [(u32, u32); 2], // (fbo, texture)
    /// Which of the two was rendered into most recently — the one to show.
    which: usize,
    width: i32,
    height: i32,
    /// Whether mpv has drawn into it yet.
    ///
    /// A framebuffer is black when it is made, and one gets made on the first
    /// frame and again whenever the picture changes size mid-stream. Blitting
    /// it before mpv has filled it paints a black rectangle over whatever was
    /// on screen, which is a fault that appears and clears itself and looks
    /// like nothing in particular.
    painted: bool,
}

pub struct Player {
    api: &'static Api,
    ctx: Ptr,
    /// Held rather than loaded at once. See `open`, and `start`.
    uri: CString,
    started: AtomicBool,
    /// mpv's renderer, created on the interface thread because that is where
    /// the OpenGL context is current, and destroyed there for the same reason.
    render_ctx: Mutex<Ptr>,
    surface: Mutex<Option<Surface>>,
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// The render thread, where one exists: Linux only, and only when a
    /// shared context could be made. `None` inside means it was tried and
    /// could not be, and the in-paint path below carries on as everywhere
    /// else.
    #[cfg(target_os = "linux")]
    threaded: std::sync::OnceLock<Option<Arc<worker::Link>>>,
    #[cfg(target_os = "linux")]
    render_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// The interface's own framebuffers wrapping the worker's textures, per
    /// generation. Containers are per-context even when textures are shared.
    #[cfg(target_os = "linux")]
    ui_fbos: Mutex<(u64, [u32; 2])>,
}

impl Player {
    /// What is playing this, for the About line and the log.
    pub fn backend() -> String {
        match library() {
            Ok(api) => versions(api).to_string(),
            Err(e) => format!("mpv is missing: {e}"),
        }
    }

    /// Open a URL and start playing it.
    ///
    /// `resume_at` is elapsed seconds from the start, or zero. There is no
    /// origin to add: mpv reports time from the beginning of the item whatever
    /// the container's timestamps say.
    pub fn open(
        uri: &str,
        resume_at: f64,
        join: JoinAt,
        transport: Transport,
        software_decoding: bool,
        repaint: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let api = library()?;

        let ctx = unsafe { (api.create)() };
        if ctx.is_null() {
            return Err("mpv_create failed".into());
        }
        let ctx = Ptr(ctx);

        let check = |what: &str, rc: c_int| -> Result<(), String> {
            if rc < 0 {
                let message = unsafe { CStr::from_ptr((api.error_string)(rc)) }
                    .to_string_lossy()
                    .into_owned();
                Err(format!("{what}: {message}"))
            } else {
                Ok(())
            }
        };

        for (name, value) in [
            ("vo", "libmpv"),
            ("terminal", "no"),
            // Hardware decoding where the driver offers it, and "auto-safe"
            // rather than "auto" so it only takes paths known to be sound.
            // This trims the decode and not the composite, which is why it
            // measured 87% of a core down to 72% and not to nothing.
            //
            // The GPU it lands on is the integrated one: the application asks
            // for that before any graphics context exists, and never exports
            // the vendor symbols that ask for the discrete chip. See
            // `prefer_integrated_gpu`.
            // "auto-safe" everywhere but a Flatpak, where it is "auto-copy".
            //
            // auto-safe lets mpv keep decoded frames in driver memory and
            // hand this application the hardware surface. That needs the
            // decoder's driver and the GL context's driver to be the same
            // Mesa — true on a normal Linux desktop, and routinely false
            // inside a sandbox, where the runtime ships one Mesa and the host
            // has another. What that produces is a stream that opens, reports
            // no error, and then stalls or tears: exactly "live TV craps
            // out", and only on the packaged build.
            //
            // auto-copy still decodes on the chip and copies the frame back
            // to normal memory, which costs a little bandwidth and works
            // whatever the two drivers are. Windows and macOS are untouched:
            // there is one driver stack on each and auto-safe is right there.
            (
                "hwdec",
                if software_decoding {
                    "no"
                } else if in_flatpak() {
                    // A sandbox's decode stack and its display stack are not
                    // the matched pair a desktop has, and full hardware
                    // decode trusts that match. auto-copy decodes on whatever
                    // the chip offers and copies the frame back to memory it
                    // owns, which survives the mismatch.
                    "auto-copy"
                } else {
                    "auto-safe"
                },
            ),
            // No `hwdec-codecs` here, deliberately, and it is worth writing
            // down why the obvious hazard is already handled. `auto-safe`
            // whitelists decoding *methods*, not codecs, so a chip can be
            // handed a stream it cannot decode and report success — Intel's
            // Arc parts removed the MPEG-2 decoder outright, and a great deal
            // of broadcast television is still MPEG-2, which comes out as
            // macroblock hash with every counter reading healthy. mpv's own
            // default for that option is already
            // `h264,vc1,hevc,vp8,vp9,av1,prores,prores_raw,ffv1,dpx`, with
            // MPEG-2 absent, so the protection is the default and setting it
            // here would only be a chance to get it wrong.
            //
            // The read-ahead this application argued itself into having: mpv
            // caches compressed packets, which is minutes of protection for
            // the memory a couple of seconds of decoded frames would cost.
            ("cache", "yes"),
            ("demuxer-max-bytes", "96MiB"),
            ("demuxer-readahead-secs", "60"),
            // Keep the file open at the end rather than tearing down, so the
            // last frame stays on screen instead of a black window.
            ("keep-open", "yes"),
            // Broadcast captions, which are not a subtitle track.
            //
            // CEA-608 and CEA-708 ride inside the video itself, in H.264 SEI
            // user data and MPEG-2 picture user data, so there is nothing in
            // the stream list to select and mpv does not make a track for them
            // unless it is asked to. Without this the caption button had
            // nothing to turn on, on files that plainly have captions.
            ("sub-create-cc-track", "yes"),
            // Made to look like broadcast captions rather than film subtitles.
            //
            // What a television draws for CEA-608 is white monospaced text in a
            // solid black band, and that is not a style choice — it is what
            // makes captions readable over a bright sky or a white shirt, which
            // is exactly where mpv's default of outlined text on nothing falls
            // apart.
            //
            // The face is named per platform because a name is all libass
            // gets, and a name that is not installed does not degrade
            // gracefully — captions come out in whatever fontconfig picks,
            // at the wrong metrics, or not at all. Consolas is on every
            // Windows; Menlo on every Mac; DejaVu Sans Mono is what a Linux
            // desktop and the freedesktop runtime both actually carry.
            #[cfg(windows)]
            ("sub-font", "Consolas"),
            #[cfg(target_os = "macos")]
            ("sub-font", "Menlo"),
            #[cfg(all(unix, not(target_os = "macos")))]
            ("sub-font", "DejaVu Sans Mono"),
            ("sub-color", "#FFFFFFFF"),
            ("sub-border-style", "background-box"),
            ("sub-back-color", "#FF000000"),
            ("sub-border-size", "0"),
            ("sub-shadow-offset", "0"),
            ("sub-bold", "no"),
            // No alignment override, deliberately.
            //
            // These forced captions to the bottom centre, which is right for a
            // subtitle file and wrong for broadcast closed captions. A caption
            // track carries its own geometry: every event arrives tagged
            // `{\an7}` — anchor to the top-left — and padded with hard spaces
            // to put it in a particular row and column, because that is what
            // roll-up captions are. Forcing an anchor of our own fights that
            // per-event positioning, and the rows end up stacked against the
            // order they were written in, which is what "the subtitles are
            // backwards" looks like on screen.
            //
            // Verified out of a real broadcast: ffmpeg decodes channel 2.1's
            // captions in the right order, with those tags on them. What was
            // reordering them was here.
            // Pace the picture against the display rather than the sound.
            //
            // The default, `audio`, presents each frame at the time the audio
            // clock says it is due and drops it if that moment has passed.
            // Nothing guarantees those moments line up with the display's
            // refresh, so on 60fps content on a 60Hz screen a steady few
            // frames a second land in the gaps and are thrown away — which is
            // exactly what a drop counter climbing next to a healthy decoder
            // and 0ms of A/V skew was saying. `display-resample` aligns frames
            // to the refresh instead and stretches the audio imperceptibly to
            // match, which is what it is for.
            //
            // It needs to know the real refresh rate, which is what the
            // `report_swap` call in `present` is telling it — and that is
            // exactly what it does not get on a translated OpenGL.
            //
            // Measured on a virtual machine: display 56fps against a 60Hz
            // screen, decoder drops zero, A/V sync zero, render costing five
            // percent of a core, and the dropped counter climbing steadily
            // until playback stopped. Nothing was short of time. mpv was
            // discarding frames it believed were late, against a refresh rate
            // it had estimated wrongly from swap timings that a virtualised
            // compositor does not deliver evenly. mpv's own player, on the
            // same machine and the same stream, is fine — and its default is
            // `audio`.
            //
            // So: `audio` where the timing cannot be trusted, which is Linux,
            // where the display may be Wayland, X11, a virtual GPU or a remote
            // desktop, and nothing tells us which. Windows and macOS keep
            // `display-resample`, where it was measured to fix a real problem
            // — a drop counter climbing beside a healthy decoder on 60fps
            // content — and where there is one compositor with honest vsync.
            (
                "video-sync",
                if cfg!(target_os = "linux") {
                    "audio"
                } else {
                    "display-resample"
                },
            ),
            ("user-agent", &crate::settings::user_agent()),
            // The other half of BLOCK_FOR_TARGET_TIME=0, which render.h is
            // explicit about and this embedding never did: "you don't need to
            // wait yourself, or set the video-timing-offset to 0 instead.
            // Disabling this without doing anything in addition will result
            // in A/V sync being slightly off."
            //
            // With the offset at its 50ms default, mpv hands over each frame
            // up to 50ms EARLY — expecting the embedder to hold it until its
            // display time, which is the blocking this render path rightly
            // refuses to do on the interface thread. Every frame was being
            // shown the moment it arrived, early by a different amount each
            // time. Where video is timed to audio, that is an unevenness no
            // renderer fix could touch, and it is why the picture was smooth
            // with no audio and never with it. Zero means: produce the frame
            // at its display time, so showing it immediately is correct.
            ("video-timing-offset", "0"),
            // mpv's own remedy for an untrustworthy audio clock, on every
            // Linux, because the axis that matters is the audio clock pacing
            // the video and that is all of Linux. The manual names the symptom:
            // "an uneven video
            // framerate in a movie which plays fine with --no-audio" — which
            // is word for word what this machine showed — and prescribes
            // autosync to smooth video timing against jittery audio delay
            // measurements instead of trusting each one. 30 is the manual's
            // own suggested value. Blank elsewhere, and blank is skipped.
            (
                "autosync",
                if cfg!(target_os = "linux") { "30" } else { "" },
            ),
            // Which audio output, from the environment only; mpv's own probe
            // order stands otherwise.
            //
            // It earned its place the hard way: on Linux video presentation is
            // slaved to the audio clock, so a sound path that jitters or
            // underruns — a virtual machine's, for one — makes *video* stumble
            // while every video counter reads healthy. `CLICKER_AO=alsa`
            // tries the device directly; `CLICKER_AO=null` silences it
            // entirely and is the ten-second test of whether a sound device is
            // what is ruining the picture.
            ("ao", &std::env::var("CLICKER_AO").unwrap_or_default()),
            // Where a live playlist is joined. The HLS demuxer defaults to
            // three segments from the end, which for Channels would throw away
            // the server-side buffer that makes a channel rewindable from the
            // moment it is tuned. Ignored by every other demuxer.
            (
                "demuxer-lavf-o",
                &format!("live_start_index={}", join.live_start_index()),
            ),
        ]
        .into_iter()
        .chain(
            // Only for the live buffer: `follow` belongs to the file protocol,
            // and handing it to an HTTP stream is an option that source has
            // never heard of.
            (transport == Transport::Timeshift).then_some(("stream-lavf-o", "follow=1")),
        )
        .chain(NO_SCRIPTS)
        {
            // A blank value means "leave the option alone", which is what the
            // environment overrides above resolve to when unset.
            if value.is_empty() {
                continue;
            }
            let (n, v) = (CString::new(name).unwrap(), CString::new(value).unwrap());
            // Unknown options are reported and skipped: a libmpv-only build has
            // no command line player, so options belonging to it do not exist,
            // and refusing to start over one that is already absent is daft.
            let rc = unsafe { (api.set_option)(ctx.0, n.as_ptr(), v.as_ptr()) };
            if rc < 0 {
                crate::log::line(&format!("[mpv] option {name}: ignored"));
            }
        }

        // Anything else, from the environment, applied last so it wins.
        //
        // `CLICKER_MPV_OPTS="profile=fast,hwdec=no"` — mpv's own option names,
        // comma separated. This exists because the alternative is what this
        // port kept doing: rebuild, reinstall and replay a channel to learn
        // what one option does. Every knob mpv has is now a restart away, and
        // what gets hard-coded above is only ever what a run like that proved.
        if let Ok(extra) = std::env::var("CLICKER_MPV_OPTS") {
            for option in extra.split(',').filter(|s| !s.trim().is_empty()) {
                let (name, value) = option.split_once('=').unwrap_or((option, "yes"));
                let (name, value) = (name.trim(), value.trim());
                let (Ok(n), Ok(v)) = (CString::new(name), CString::new(value)) else {
                    continue;
                };
                let rc = unsafe { (api.set_option)(ctx.0, n.as_ptr(), v.as_ptr()) };
                crate::log::line(&format!(
                    "[mpv] {name}={value} from the environment{}",
                    if rc < 0 { ": refused" } else { "" }
                ));
            }
        }

        // Everything mpv has to say, into the log file beside the crash log.
        // This is the thing the built-in pipeline could not give a user.
        let level = CString::new("info").unwrap();
        unsafe { (api.request_log)(ctx.0, level.as_ptr()) };

        check("initialize", unsafe { (api.initialize)(ctx.0) })?;

        let shared = Arc::new(Shared {
            quit: AtomicBool::new(false),
            rendered: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            error: Mutex::new(None),
            render_load: Mutex::new(0.0),
            render_ms: Mutex::new(0.0),
            offered: AtomicU64::new(0),
            presented: AtomicU64::new(0),
            reported: Mutex::new(None),
            last_reported: AtomicU64::new(0),
            last_painted: AtomicU64::new(0),
            last_render: Mutex::new(None),
            size: AtomicU64::new(0),
            repaint: Box::new(repaint),
        });

        // Deliberately not loading the file yet.
        //
        // mpv initializes its video output as part of loading, and with
        // `vo=libmpv` that means asking this application for a renderer. If one
        // does not exist by then it logs "Error opening/initializing the
        // selected video_out (--vo) device" and plays the file with video
        // switched off for good — audio, no picture, and no further complaint.
        //
        // The renderer cannot be created here: it needs the OpenGL context,
        // which belongs to the interface thread. So loading waits for `start`,
        // which that thread calls once the renderer is up.
        // A live buffer is a file that is still being written, and mpv has no
        // way to know that. It reads to the end, finds end-of-file, and stops —
        // which on live television is playback halting every time it catches
        // up with the writer, roughly every twenty seconds.
        //
        // FFmpeg's file protocol has a mode for precisely this: `follow` blocks
        // at the end of the file instead of reporting the end of it, the way
        // `tail -f` does. Reaching it means asking mpv to open the file through
        // libavformat rather than its own reader, which is what the `lavf://`
        // prefix does. Forward slashes because the path travels through
        // FFmpeg's protocol parser, where a backslash is not a separator.
        let target = match transport {
            Transport::Timeshift => {
                CString::new(format!("lavf://file:{}", uri.replace('\\', "/")))
            }
            _ => CString::new(uri),
        }
        .map_err(|_| "the address has a NUL in it")?;

        let thread = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("clicker-mpv".into())
                .spawn(move || event_loop(api, ctx, shared, resume_at))
                .map_err(|e| e.to_string())?
        };

        Ok(Self {
            api,
            ctx,
            uri: target,
            started: AtomicBool::new(false),
            render_ctx: Mutex::new(Ptr(std::ptr::null_mut())),
            surface: Mutex::new(None),
            #[cfg(target_os = "linux")]
            threaded: std::sync::OnceLock::new(),
            #[cfg(target_os = "linux")]
            render_thread: Mutex::new(None),
            #[cfg(target_os = "linux")]
            ui_fbos: Mutex::new((0, [0, 0])),
            shared,
            thread: Some(thread),
        })
    }

    /// Create the renderer and begin playing, in that order.
    ///
    /// Called from the interface thread, which is the only one where the
    /// OpenGL context is current. Doing nothing on later calls is deliberate:
    /// it is invoked from `update`, every frame, precisely so that a renderer
    /// which failed to come up the first time gets another attempt rather than
    /// leaving a player that is permanently silent about why.
    ///
    /// # Safety
    ///
    /// An OpenGL context must be current on the calling thread.
    pub unsafe fn start(&self, gl: &GlFns) {
        if !self.ensure_renderer(gl) || self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let load = CString::new("loadfile").unwrap();
        let argv = [load.as_ptr(), self.uri.as_ptr(), std::ptr::null()];
        let rc = (self.api.command)(self.ctx.0, argv.as_ptr());
        if rc < 0 {
            let why = CStr::from_ptr((self.api.error_string)(rc)).to_string_lossy();
            *self.shared.error.lock().unwrap() = Some(format!("mpv could not open it: {why}"));
        }
    }

    /// Bring mpv's renderer up against the current OpenGL context.
    ///
    /// # Safety
    ///
    /// An OpenGL context must be current on the calling thread.
    unsafe fn ensure_renderer(&self, _gl: &GlFns) -> bool {
        // The render thread claims the renderer first, wherever the first
        // call lands. mpv allows one render context per handle, and the
        // warm-up call that runs when a stream loads used to win the race:
        // it created the in-paint renderer before the first paint ever
        // reached the threaded path, and the worker then heard "There is
        // already a mpv_render_context set" and fell back — the whole thread
        // built and lost to its own warm-up.
        #[cfg(target_os = "linux")]
        if self.threaded_active() {
            return true;
        }
        let mut render_ctx = self.render_ctx.lock().unwrap();
        if !render_ctx.0.is_null() {
            return true;
        }
        let api_type = CString::new("opengl").unwrap();
        let mut init = OpenGlInitParams {
            get_proc_address: gl_proc_address,
            get_proc_address_ctx: std::ptr::null_mut(),
        };
        let mut params = [
            RenderParam {
                kind: MPV_RENDER_PARAM_API_TYPE,
                data: api_type.as_ptr() as *mut c_void,
            },
            RenderParam {
                kind: MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: &mut init as *mut OpenGlInitParams as *mut c_void,
            },
            RenderParam {
                kind: MPV_RENDER_PARAM_INVALID,
                data: std::ptr::null_mut(),
            },
        ];
        let mut created: *mut c_void = std::ptr::null_mut();
        let rc = (self.api.render_create)(&mut created, self.ctx.0, params.as_mut_ptr());
        if rc < 0 {
            let why = CStr::from_ptr((self.api.error_string)(rc)).to_string_lossy();
            *self.shared.error.lock().unwrap() =
                Some(format!("mpv could not use this graphics context: {why}"));
            return false;
        }
        // What paces playback. mpv calls this the moment a frame is ready, and
        // the interface repaints, and the paint callback draws it.
        //
        // The pointer is to the `Shared` inside this player's `Arc`, which
        // outlives the render context: `release_gl` frees the context, and
        // `Drop` unsets this callback before anything else, so mpv can never
        // be left calling into a freed one.
        (self.api.render_set_update_callback)(
            created,
            on_frame_ready,
            Arc::as_ptr(&self.shared) as *mut c_void,
        );

        crate::log::line("[mpv] OpenGL renderer ready");
        *render_ctx = Ptr(created);
        true
    }

    /// Draw the current frame into an OpenGL framebuffer this owns, and hand
    /// back the texture behind it.
    ///
    /// Called on the interface thread, inside a paint callback, because that is
    /// the only place the OpenGL context is current. Returns `None` until there
    /// is a picture, which is the caller's cue to keep showing the spinner.
    ///
    /// # Safety
    ///
    /// An OpenGL context must be current on the calling thread.
    pub unsafe fn render_to_texture(&self, gl: &GlFns, target: (i32, i32)) -> Option<(u32, i32, i32)> {
        let (video_w, video_h) = self.video_size();
        if video_w == 0 || video_h == 0 {
            return None;
        }
        let video_w = video_w as i32;

        if !self.ensure_renderer(gl) {
            return None;
        }
        let render_ctx = self.render_ctx.lock().unwrap();

        // A framebuffer the size of the picture on screen, not the size of the
        // stream.
        //
        // It used to be the stream's size on the reasoning that "the interface
        // scales the texture afterwards, which the GPU does for nothing". That
        // is true of a graphics card and false of everything else. Rendering
        // 1080p60 into a 1920x1080 target and then blitting it down to a
        // smaller window is two full-frame operations per frame — mpv shades
        // two megapixels, then the blit resamples two megapixels — where mpv
        // playing the same stream in its own window does one, at the size of
        // the window. Through a translated OpenGL, which is what a virtual
        // machine or a remote display gives you, that difference is the whole
        // budget.
        //
        // mpv is built to render into whatever target it is handed and fits
        // the picture to it, so asking for the size it will actually be seen
        // at costs nothing and saves the rest. Clamped to the stream's own
        // size, because rendering larger than the source only invents pixels
        // more expensively than the blit would.
        //
        // Rounded up to a multiple of 32 so that dragging a window edge does
        // not reallocate the framebuffer on every pixel of the drag.
        // The height comes from the width by the rectangle's own ratio, so
        // the framebuffer has exactly the aspect of the rectangle it will be
        // stretched into. Rounding each axis up to 32 independently — which
        // is what stood here — skewed the aspect by up to two percent, and by
        // a different amount at every scale step: mpv letterboxed the
        // mismatch inside the framebuffer, the blit stretched bars and all,
        // and each step visibly squeezed the picture. Width alone is rounded,
        // so a drag still does not reallocate per pixel; the height merely
        // follows it.
        let round32 = |n: i32| ((n + 31) / 32 * 32).max(32);
        let width = round32(target.0).min(video_w);
        let height = (((width as i64 * target.1 as i64) / target.0.max(1) as i64) as i32
            & !1)
            .max(2);
        let mut surface = self.surface.lock().unwrap();
        let stale = surface
            .as_ref()
            .map(|s| s.width != width || s.height != height)
            .unwrap_or(true);
        if stale {
            if let Some(old) = surface.take() {
                for (fbo, texture) in old.bufs {
                    gl.delete_framebuffers(1, &fbo);
                    gl.delete_textures(1, &texture);
                }
            }
            let mut bufs = [(0u32, 0u32); 2];
            let previous = gl.draw_framebuffer();
            let mut failed = false;
            for buf in bufs.iter_mut() {
                let mut texture = 0u32;
                gl.gen_textures(1, &mut texture);
                gl.bind_texture(GL_TEXTURE_2D, texture);
                gl.tex_image_2d(
                    GL_TEXTURE_2D,
                    0,
                    GL_RGBA8,
                    width,
                    height,
                    0,
                    GL_RGBA,
                    GL_UNSIGNED_BYTE,
                    std::ptr::null(),
                );
                gl.tex_parameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
                gl.tex_parameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
                gl.tex_parameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
                gl.tex_parameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
                gl.bind_texture(GL_TEXTURE_2D, 0);

                let mut fbo = 0u32;
                gl.gen_framebuffers(1, &mut fbo);
                gl.bind_framebuffer(GL_FRAMEBUFFER, fbo);
                gl.framebuffer_texture_2d(
                    GL_FRAMEBUFFER,
                    GL_COLOR_ATTACHMENT0,
                    GL_TEXTURE_2D,
                    texture,
                    0,
                );
                if gl.check_framebuffer_status(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE {
                    failed = true;
                }
                *buf = (fbo, texture);
            }
            gl.bind_framebuffer(GL_FRAMEBUFFER, previous);
            if failed {
                for (fbo, texture) in bufs {
                    gl.delete_framebuffers(1, &fbo);
                    gl.delete_textures(1, &texture);
                }
                *self.shared.error.lock().unwrap() =
                    Some(format!("the graphics driver refused a {width}x{height} target"));
                return None;
            }
            *surface = Some(Surface {
                bufs,
                which: 0,
                width,
                height,
                painted: false,
            });
        }
        // Copied out rather than borrowed: `painted` is set further down, and
        // holding a reference to the surface until then would borrow it for
        // the whole function.
        let (texture, width, height, painted) = surface
            .as_ref()
            .map(|s| (s.bufs[s.which].1, s.width, s.height, s.painted))?;

        // Nothing new to draw: hand back what is already there rather than
        // asking mpv to redraw a frame it has already produced — unless
        // nothing has ever been drawn into it, in which case what is there is
        // black and had better not reach the screen.
        if (self.api.render_update)(render_ctx.0) & MPV_RENDER_UPDATE_FRAME == 0 {
            return painted.then_some((texture, width, height));
        }

        // Skip a frame rather than starve the interface.
        //
        // This runs inside egui's paint, on the thread that also answers the
        // mouse, so however long `render` takes is time the window is not
        // responding. On hardware that keeps up it is a millisecond or two and
        // this never triggers. Where it does not — 60fps through a translated
        // OpenGL, which is a virtual machine or a remote display — the call
        // can cost most of a frame interval, and asking for every frame leaves
        // nothing for anything else: the picture is smooth and the program is
        // deaf.
        //
        // So the renderer gets a budget — but of processor time, not of clock.
        //
        // This was decided on the wall clock and it was wrong in the way that
        // matters. The render call issues OpenGL commands that the driver
        // throttles to the display's refresh, so on a compositor that makes
        // the caller wait — a virtual machine, a remote display, or an
        // ordinary vsync — the call takes most of a frame interval while
        // costing almost no work. The clock said "struggling" where the truth
        // was "waiting", and frames were dropped to buy responsiveness that
        // was never short.
        //
        // It could not recover, either. The estimate is only updated by frames
        // that are actually rendered, and those still wait exactly as long, so
        // a reading that crossed the threshold stayed across it. Skipping made
        // it worse: fewer renders meant more accumulated work in each one, the
        // measurement climbed, and the rule escalated from every second frame
        // to two in three and stayed there. mpv, meanwhile, had already been
        // told the frame was collected, and with video-sync expecting a render
        // at display cadence its timing estimate went with it.
        //
        // Processor time cannot be confused that way. If the paint thread is
        // genuinely spending a core on this, skipping a frame gives the mouse
        // somewhere to go; if it is parked in the driver waiting for the
        // display, skipping buys nothing and costs a frame. And the ceiling is
        // every second frame — never two in three — because there is no
        // reading of a spiral that justifies deepening it.
        let offered = self.shared.offered.fetch_add(1, Ordering::Relaxed);
        let load = *self.shared.render_load.lock().unwrap();
        if load > 0.9 && offered % 2 != 0 {
            self.shared.dropped.fetch_add(1, Ordering::Relaxed);
            // Decay while skipping, so the estimate can come back down. The
            // old one was only ever fed by rendered frames, which is how it
            // latched.
            let mut smoothed = self.shared.render_load.lock().unwrap();
            *smoothed *= 0.95;
            return painted.then_some((texture, width, height));
        }

        // Hand mpv a clean slate for pixel uploads.
        //
        // mpv and egui share one OpenGL context, and egui's painter sets its
        // own unpack state when it uploads a font atlas or an image and does
        // not put it back. mpv then uploads each frame's planes — nv12, so two
        // of them per frame — with whatever row length, alignment and pixel
        // buffer binding were left behind, and reads its own frame data at the
        // wrong stride. The picture that comes out is horizontal hash, from a
        // decoder that succeeded and a renderer that reported no error, which
        // is why every counter said playback was healthy.
        //
        // These are the OpenGL defaults. Restored afterwards, because egui is
        // entitled to the same courtesy this was not being shown.
        let mut unpack = [0i32; 4];
        for (index, name) in [
            GL_UNPACK_ALIGNMENT,
            GL_UNPACK_ROW_LENGTH,
            GL_UNPACK_SKIP_PIXELS,
            GL_UNPACK_SKIP_ROWS,
        ]
        .into_iter()
        .enumerate()
        {
            (gl.get_integerv)(name, &mut unpack[index]);
        }
        let mut unpack_buffer = 0i32;
        (gl.get_integerv)(GL_PIXEL_UNPACK_BUFFER_BINDING, &mut unpack_buffer);

        (gl.pixel_storei)(GL_UNPACK_ALIGNMENT, 4);
        (gl.pixel_storei)(GL_UNPACK_ROW_LENGTH, 0);
        (gl.pixel_storei)(GL_UNPACK_SKIP_PIXELS, 0);
        (gl.pixel_storei)(GL_UNPACK_SKIP_ROWS, 0);
        if unpack_buffer != 0 {
            (gl.bind_buffer)(GL_PIXEL_UNPACK_BUFFER, 0);
        }

        let cpu_before = thread_cpu_ms();
        let wall_before = Instant::now();

        // Into the buffer that is NOT on screen. The front one may still be
        // in flight inside the driver, and writing to it is the fence wait
        // this structure exists to avoid.
        let mut fbo = OpenGlFbo {
            fbo: surface
                .as_ref()
                .map(|s| s.bufs[s.which ^ 1].0)
                .unwrap_or(0) as c_int,
            w: width,
            h: height,
            internal_format: 0,
        };
        // Flipped, and the reasoning that said otherwise was wrong.
        //
        // The argument was that a framebuffer-to-framebuffer blit needs no
        // flip because both count from the bottom. What that missed is that
        // mpv already renders for a target whose origin is top-left, so
        // leaving this off stood the picture on its head. Observed, then
        // fixed; the theory was tidier than the truth.
        let mut flip: c_int = 1;
        // Do not wait for the frame's target time.
        //
        // This is the whole difference between a player and a stutter. By
        // default `render` sleeps until the frame is due, which is exactly
        // right when a thread of its own is doing nothing else — and ruinous
        // here, because this runs inside egui's paint on the interface thread.
        // Every frame the entire application stopped for most of a frame
        // interval waiting for mpv, which beat against vsync and produced 43fps
        // and a drop counter climbing forever. mpv still paces playback; it
        // does it by calling `on_frame_ready` when the next frame is due,
        // which is the correct end of the arrangement to put the waiting in.
        let mut block: c_int = 0;
        let mut params = [
            RenderParam {
                kind: MPV_RENDER_PARAM_OPENGL_FBO,
                data: &mut fbo as *mut OpenGlFbo as *mut c_void,
            },
            RenderParam {
                kind: MPV_RENDER_PARAM_FLIP_Y,
                data: &mut flip as *mut c_int as *mut c_void,
            },
            RenderParam {
                kind: MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
                data: &mut block as *mut c_int as *mut c_void,
            },
            RenderParam {
                kind: MPV_RENDER_PARAM_INVALID,
                data: std::ptr::null_mut(),
            },
        ];
        let rc = (self.api.render)(render_ctx.0, params.as_mut_ptr());
        if rc >= 0 {
            if let Some(live) = surface.as_mut() {
                // The freshly drawn buffer becomes the front one; the old
                // front is free to finish whatever the driver is doing to it.
                live.which ^= 1;
                live.painted = true;
            }
        }

        // Put egui's unpack state back exactly as it was found.
        (gl.pixel_storei)(GL_UNPACK_ALIGNMENT, unpack[0]);
        (gl.pixel_storei)(GL_UNPACK_ROW_LENGTH, unpack[1]);
        (gl.pixel_storei)(GL_UNPACK_SKIP_PIXELS, unpack[2]);
        (gl.pixel_storei)(GL_UNPACK_SKIP_ROWS, unpack[3]);
        if unpack_buffer != 0 {
            (gl.bind_buffer)(GL_PIXEL_UNPACK_BUFFER, unpack_buffer as u32);
        }

        if rc < 0 {
            self.shared.dropped.fetch_add(1, Ordering::Relaxed);
            return painted.then_some((texture, width, height));
        }

        // Processor time spent here, over real time since the last frame: the
        // share of one core this costs.
        //
        // The denominator has to be the interval between frames, not the
        // duration of the call. Dividing by the call's own duration was right
        // only while `render` blocked until the frame was due — it made the
        // call last a frame interval. Now that it returns immediately, that
        // sum divides a millisecond of work by a millisecond of wall clock and
        // reports a third of a core for something costing eight percent.
        // What the call cost on the clock, which is what the interface felt.
        let spent_ms = wall_before.elapsed().as_secs_f32() * 1000.0;
        {
            let mut smoothed = self.shared.render_ms.lock().unwrap();
            *smoothed = if *smoothed > 0.0 {
                *smoothed * 0.85 + spent_ms * 0.15
            } else {
                spent_ms
            };
        }

        let cpu = thread_cpu_ms() - cpu_before;
        let mut last = self.shared.last_render.lock().unwrap();
        if let Some(previous) = last.replace(wall_before) {
            let interval = wall_before.duration_since(previous).as_secs_f64() * 1000.0;
            if interval > 0.0 {
                let load = (cpu / interval) as f32;
                let mut smoothed = self.shared.render_load.lock().unwrap();
                *smoothed = if *smoothed > 0.0 {
                    *smoothed * 0.9 + load * 0.1
                } else {
                    load
                };
            }
        }
        self.shared.rendered.fetch_add(1, Ordering::Relaxed);
        let front = surface.as_ref().map(|s| s.bufs[s.which].1).unwrap_or(texture);
        Some((front, width, height))
    }

    fn get_f64(&self, name: &str) -> Option<f64> {
        let name = CString::new(name).ok()?;
        let mut value = f64::NAN;
        let rc = unsafe {
            (self.api.get_property)(
                self.ctx.0,
                name.as_ptr(),
                MPV_FORMAT_DOUBLE,
                &mut value as *mut _ as *mut c_void,
            )
        };
        (rc >= 0 && value.is_finite()).then_some(value)
    }

    fn get_i64(&self, name: &str) -> Option<i64> {
        let name = CString::new(name).ok()?;
        let mut value = 0i64;
        let rc = unsafe {
            (self.api.get_property)(
                self.ctx.0,
                name.as_ptr(),
                MPV_FORMAT_INT64,
                &mut value as *mut _ as *mut c_void,
            )
        };
        (rc >= 0).then_some(value)
    }

    fn get_string(&self, name: &str) -> Option<String> {
        let name = CString::new(name).ok()?;
        let mut out: *mut c_char = std::ptr::null_mut();
        let rc = unsafe {
            (self.api.get_property)(
                self.ctx.0,
                name.as_ptr(),
                MPV_FORMAT_STRING,
                &mut out as *mut *mut c_char as *mut c_void,
            )
        };
        if rc < 0 || out.is_null() {
            return None;
        }
        let text = unsafe { CStr::from_ptr(out) }.to_string_lossy().into_owned();
        unsafe { (self.api.free)(out as *mut c_void) };
        Some(text)
    }

    fn get_flag(&self, name: &str) -> bool {
        let Ok(name) = CString::new(name) else { return false };
        let mut value: c_int = 0;
        let rc = unsafe {
            (self.api.get_property)(
                self.ctx.0,
                name.as_ptr(),
                MPV_FORMAT_FLAG,
                &mut value as *mut _ as *mut c_void,
            )
        };
        rc >= 0 && value != 0
    }

    fn set_flag(&self, name: &str, on: bool) {
        let Ok(name) = CString::new(name) else { return };
        let mut value: c_int = if on { 1 } else { 0 };
        unsafe {
            (self.api.set_property)(
                self.ctx.0,
                name.as_ptr(),
                MPV_FORMAT_FLAG,
                &mut value as *mut _ as *mut c_void,
            )
        };
    }

    fn set_f64(&self, name: &str, value: f64) {
        let Ok(name) = CString::new(name) else { return };
        let mut value = value;
        unsafe {
            (self.api.set_property)(
                self.ctx.0,
                name.as_ptr(),
                MPV_FORMAT_DOUBLE,
                &mut value as *mut _ as *mut c_void,
            )
        };
    }

    fn command(&self, args: &[&str]) -> bool {
        let owned: Vec<CString> = args.iter().filter_map(|a| CString::new(*a).ok()).collect();
        if owned.len() != args.len() {
            return false;
        }
        let mut argv: Vec<*const c_char> = owned.iter().map(|a| a.as_ptr()).collect();
        argv.push(std::ptr::null());
        unsafe { (self.api.command)(self.ctx.0, argv.as_ptr()) >= 0 }
    }

    // --- the surface the screens use -------------------------------------

    pub fn decoded(&self) -> u64 {
        self.shared.rendered.load(Ordering::Relaxed)
    }

    pub fn dropped(&self) -> u64 {
        self.get_i64("frame-drop-count").unwrap_or(0).max(0) as u64
    }

    /// Frames the decoder threw away before they ever reached output, which is
    /// a different fault from `dropped` and has a different cause: the machine
    /// cannot decode in real time, rather than cannot present in time.
    pub fn decoder_dropped(&self) -> u64 {
        self.get_i64("decoder-frame-drop-count").unwrap_or(0).max(0) as u64
    }

    /// Seconds the picture is ahead of the sound. mpv's own figure, and the
    /// one number that says whether playback is actually correct.
    pub fn avsync(&self) -> f64 {
        self.get_f64("avsync").unwrap_or(0.0)
    }

    /// The stream's picture size, or zeros before there is one.
    ///
    /// Read from what the event thread published rather than asked of mpv
    /// here: this is called from inside a paint callback, on every frame, and
    /// a property lookup takes mpv's own lock.
    pub fn video_size(&self) -> (u32, u32) {
        let packed = self.shared.size.load(Ordering::Relaxed);
        ((packed >> 32) as u32, (packed & 0xffff_ffff) as u32)
    }

    /// Seconds of media read ahead of playback.
    pub fn buffered(&self) -> f64 {
        self.get_f64("demuxer-cache-duration").unwrap_or(0.0)
    }

    /// What fraction of one processor core mpv's software renderer is costing,
    /// smoothed. 1.0 is one core saturated.
    pub fn render_load(&self) -> f32 {
        *self.shared.render_load.lock().unwrap()
    }

    /// Tell the renderer how large the picture is on screen, in physical
    /// pixels. Called every frame by the interface; see `Shared::on_screen`.
    /// Draw the current frame onto whatever egui is drawing into.
    ///
    /// `rect` is where the picture goes, in physical pixels, as
    /// `[left, from_bottom, width, height]` — OpenGL's own convention, and
    /// exactly what egui's `viewport_in_pixels` reports. Taking it from there
    /// rather than converting points by hand is deliberate: the hand-rolled
    /// version had to guess the drawing surface's height from the screen rect
    /// and the scale factor, and guessed slightly wrong, which left a band of
    /// background along the edge of a full screen picture.
    ///
    /// A blit rather than a textured quad. The picture is opaque and sits under
    /// everything else, so there is nothing to blend it with, and a blit needs
    /// no shader, no vertex buffer and no state of its own beyond what is
    /// saved and put back here.
    ///
    /// # Safety
    ///
    /// An OpenGL context must be current on the calling thread.
    pub unsafe fn present(&self, gl: &GlFns, rect: [i32; 4], screen: [i32; 2]) -> bool {
        // The rect first, because the renderer now sizes its target to it.
        let [x, bottom, w, h] = rect;
        if w <= 0 || h <= 0 {
            return false;
        }
        let top = bottom + h;

        // Straight into the window, when something is being weighed. See
        // `VideoTarget`: this is a diagnostic arm, not a mode to ship.
        if video_target() == VideoTarget::Window {
            return self.present_to_window(gl, screen);
        }

        // The render thread, where one exists. mpv renders over there, into
        // shared textures, and this thread only waits on a GPU-side fence and
        // blits — so however long the driver holds the renderer hostage, it
        // is holding a thread nobody is typing at.
        #[cfg(target_os = "linux")]
        if let Some(done) = self.threaded_present(gl, rect) {
            return done;
        }

        if self.render_to_texture(gl, (w, h)).is_none() {
            return false;
        }
        let Some(source) = self
            .surface
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| (s.bufs[s.which].0, s.width, s.height))
        else {
            return false;
        };
        let (fbo, sw, sh) = source;

        let target = gl.draw_framebuffer();
        // egui leaves the scissor set to the clip rectangle of whatever it drew
        // last, and a blit obeys it. Without this the picture is cropped to
        // some other widget's bounds, which looks like a rendering fault
        // several layers away from the cause.
        let scissoring = (gl.is_enabled)(GL_SCISSOR_TEST) != 0;
        if scissoring {
            (gl.disable)(GL_SCISSOR_TEST);
        }

        gl.bind_framebuffer(GL_READ_FRAMEBUFFER, fbo);
        gl.bind_framebuffer(GL_DRAW_FRAMEBUFFER, target);
        (gl.blit_framebuffer)(
            0, 0, sw, sh, // the whole picture
            x, bottom, x + w, top, // where it goes
            GL_COLOR_BUFFER_BIT,
            GL_LINEAR as u32,
        );

        // Put back what egui expects to still be true.
        gl.bind_framebuffer(GL_READ_FRAMEBUFFER, target);
        gl.bind_framebuffer(GL_DRAW_FRAMEBUFFER, target);
        if scissoring {
            (gl.enable)(GL_SCISSOR_TEST);
        }

        // Tell mpv a frame reached the screen, once per frame it gave us.
        //
        // This was moved to the top of this function on the theory that the
        // previous frame's swap had completed by then and so the timing would
        // be truer. In practice that reported once per *paint* rather than
        // once per *frame*, and playback started shedding frames in quantity.
        // Measured behavior beat the theory, so it is back where it was: the
        // reasoning for moving it may still be right, but it was wrong about
        // what mattered.
        let render_ctx = *self.render_ctx.lock().unwrap();
        if !render_ctx.0.is_null() {
            (self.api.render_report_swap)(render_ctx.0);
        }
        self.note_present((sw, sh));
        true
    }

    /// Draw the frame into the window's own framebuffer, with no target of
    /// this application's own and no blit.
    ///
    /// A measuring instrument. It overwrites everything egui has drawn so far
    /// this frame, because mpv is being handed the whole window and clears it,
    /// and it ignores where the picture was supposed to sit. What it answers
    /// is the one question the shipped path cannot: how much of a frame goes
    /// on the offscreen target and the blit, rather than on decoding and
    /// shading the picture. See `VideoTarget`.
    ///
    /// # Safety
    ///
    /// An OpenGL context must be current on the calling thread.
    unsafe fn present_to_window(&self, gl: &GlFns, screen: [i32; 2]) -> bool {
        let [sw, sh] = screen;
        if sw <= 0 || sh <= 0 || !self.ensure_renderer(gl) {
            return false;
        }
        let render_ctx = *self.render_ctx.lock().unwrap();
        if render_ctx.0.is_null() {
            return false;
        }
        if (self.api.render_update)(render_ctx.0) & MPV_RENDER_UPDATE_FRAME == 0 {
            return false;
        }

        let wall_before = Instant::now();
        let mut fbo = OpenGlFbo {
            fbo: 0,
            w: sw,
            h: sh,
            internal_format: 0,
        };
        let mut flip: c_int = 1;
        let mut block: c_int = 0;
        let mut params = [
            RenderParam {
                kind: MPV_RENDER_PARAM_OPENGL_FBO,
                data: &mut fbo as *mut OpenGlFbo as *mut c_void,
            },
            RenderParam {
                kind: MPV_RENDER_PARAM_FLIP_Y,
                data: &mut flip as *mut c_int as *mut c_void,
            },
            RenderParam {
                kind: MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
                data: &mut block as *mut c_int as *mut c_void,
            },
            RenderParam {
                kind: MPV_RENDER_PARAM_INVALID,
                data: std::ptr::null_mut(),
            },
        ];
        let rc = (self.api.render)(render_ctx.0, params.as_mut_ptr());
        if rc < 0 {
            self.shared.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        self.shared.offered.fetch_add(1, Ordering::Relaxed);
        self.shared.rendered.fetch_add(1, Ordering::Relaxed);
        let spent_ms = wall_before.elapsed().as_secs_f32() * 1000.0;
        {
            let mut smoothed = self.shared.render_ms.lock().unwrap();
            *smoothed = if *smoothed > 0.0 {
                *smoothed * 0.85 + spent_ms * 0.15
            } else {
                spent_ms
            };
        }
        (self.api.render_report_swap)(render_ctx.0);
        self.note_present((sw, sh));
        true
    }

    /// Say, every few seconds, what is actually reaching the screen.
    ///
    /// Frames on the glass per second, mpv's own count of what it threw away,
    /// and how long the render call is taking. Enough, in one line, to tell
    /// the three cases apart that this port kept confusing: a renderer that is
    /// slow, a renderer that is merely waiting, and a player dropping frames
    /// because the audio clock says they are late.
    fn note_present(&self, at: (i32, i32)) {
        let count = self.shared.presented.fetch_add(1, Ordering::Relaxed) + 1;
        let mut reported = self.shared.reported.lock().unwrap();
        let Some(since) = reported.as_ref().map(|t| t.elapsed().as_secs_f32()) else {
            *reported = Some(Instant::now());
            return;
        };
        if since < 5.0 {
            return;
        }
        *reported = Some(Instant::now());
        let painted = self.shared.rendered.load(Ordering::Relaxed);
        let before = self.shared.last_reported.swap(count, Ordering::Relaxed);
        let before_painted = self.shared.last_painted.swap(painted, Ordering::Relaxed);
        drop(reported);

        // Two rates, because they answer different questions and this file
        // spent a session confusing them. Frames are pictures mpv actually
        // drew, which is what playback looks like. Paints are times the
        // interface put something on screen, which is how responsive the
        // window is — and which counts a frame twice when the interface is
        // faster than the video.
        let frames = painted.saturating_sub(before_painted) as f32 / since;
        let paints = count.saturating_sub(before) as f32 / since;
        let render_ms = *self.shared.render_ms.lock().unwrap();
        let load = *self.shared.render_load.lock().unwrap();
        let dropped = self.get_i64("frame-drop-count").unwrap_or(-1);
        let late = self.get_i64("vo-delayed-frame-count").unwrap_or(-1);
        let decoder = self.get_f64("estimated-vf-fps").unwrap_or(0.0);
        let skipped = self.shared.dropped.load(Ordering::Relaxed);
        let offered = self.shared.offered.load(Ordering::Relaxed);
        crate::log::line(&format!(
            "[mpv] {frames:.1}fps drawn, {paints:.1}fps painted, at {}x{}; decoder \
             {decoder:.1}fps, mpv dropped {dropped}, late {late}; render \
             {render_ms:.1}ms at {load:.2} of a core, {skipped} of {offered} skipped",
            at.0, at.1
        ));
    }

    /// Release the OpenGL objects, on the thread that owns them.
    ///
    /// Must be called from a paint callback before the player is dropped.
    /// mpv's renderer holds shaders and textures of its own and destroys them
    /// in `render_context_free`, which is only lawful with the context current
    /// — and this application drops players from the interface thread while
    /// egui is between frames, where it is not.
    ///
    /// # Safety
    ///
    /// An OpenGL context must be current on the calling thread.
    pub unsafe fn release_gl(&self, gl: &GlFns) {
        let mut render_ctx = self.render_ctx.lock().unwrap();
        if !render_ctx.0.is_null() {
            (self.api.render_free)(render_ctx.0);
            *render_ctx = Ptr(std::ptr::null_mut());
        }
        if let Some(surface) = self.surface.lock().unwrap().take() {
            for (fbo, texture) in surface.bufs {
                gl.delete_framebuffers(1, &fbo);
                gl.delete_textures(1, &texture);
            }
        }
        // The interface's wrappers around the worker's textures. The worker's
        // own objects — its renderer included — are freed by the worker, on
        // the context that owns them, when Drop joins it.
        #[cfg(target_os = "linux")]
        {
            let mut cache = self.ui_fbos.lock().unwrap();
            for fbo in cache.1 {
                if fbo != 0 {
                    gl.delete_framebuffers(1, &fbo);
                }
            }
            *cache = (0, [0, 0]);
        }
    }

    /// Whether the render thread owns (or is about to own) the renderer.
    /// Spawns it on first ask, which needs the interface's context current —
    /// true at every call site, all of which sit inside a paint.
    #[cfg(target_os = "linux")]
    fn threaded_active(&self) -> bool {
        // On by default. It was parked once, over white flashes that turned
        // out to be the cross-context fence handoff — on the driver this
        // thread exists for, fences are the broken primitive — and the
        // handoff no longer uses them: the worker publishes only frames the
        // GPU has finished. CLICKER_RENDER_THREAD=0 is the off switch if a
        // machine ever needs the in-paint path back.
        if std::env::var("CLICKER_RENDER_THREAD")
            .map(|v| v == "0")
            .unwrap_or(false)
        {
            return false;
        }
        match self.threaded.get_or_init(|| worker::spawn(self)) {
            Some(link) => !link.dead.load(Ordering::SeqCst),
            None => false,
        }
    }

    /// Present by way of the render thread. `None` means there is no thread —
    /// it was tried and could not be had — and the in-paint path should run.
    #[cfg(target_os = "linux")]
    unsafe fn threaded_present(&self, gl: &GlFns, rect: [i32; 4]) -> Option<bool> {
        if !self.threaded_active() {
            // The worker cleaned up entirely before dying, so falling back to
            // the in-paint renderer is safe: no second render context exists.
            return None;
        }
        let link = self.threaded.get().and_then(|t| t.as_ref())?;

        let [x, bottom, w, h] = rect;
        let top = bottom + h;

        let (texs, front, gen, painted) = {
            let mut st = link.state.lock().unwrap();
            if st.want != (w, h) {
                st.want = (w, h);
                link.pending.store(true, Ordering::Release);
                link.wake.notify_one();
            }
            (st.tex, st.front, st.generation, st.painted)
        };
        if !painted {
            return Some(false);
        }

        // Nothing to wait on: the worker publishes only frames the GPU has
        // finished, so by the time a texture is visible here it is whole.
        let mut cache = self.ui_fbos.lock().unwrap();
        if cache.0 != gen {
            for fbo in cache.1 {
                if fbo != 0 {
                    gl.delete_framebuffers(1, &fbo);
                }
            }
            let mut pair = [0u32; 2];
            let previous = gl.draw_framebuffer();
            for (i, tex) in texs.iter().enumerate() {
                let mut fbo = 0u32;
                gl.gen_framebuffers(1, &mut fbo);
                gl.bind_framebuffer(GL_FRAMEBUFFER, fbo);
                gl.framebuffer_texture_2d(
                    GL_FRAMEBUFFER,
                    GL_COLOR_ATTACHMENT0,
                    GL_TEXTURE_2D,
                    *tex,
                    0,
                );
                pair[i] = fbo;
            }
            gl.bind_framebuffer(GL_FRAMEBUFFER, previous);
            *cache = (gen, pair);
        }
        let read_fbo = cache.1[front];
        drop(cache);

        let (sw, sh) = {
            let st = link.state.lock().unwrap();
            (st.width, st.height)
        };

        let target = gl.draw_framebuffer();
        let scissoring = (gl.is_enabled)(GL_SCISSOR_TEST) != 0;
        if scissoring {
            (gl.disable)(GL_SCISSOR_TEST);
        }
        gl.bind_framebuffer(GL_READ_FRAMEBUFFER, read_fbo);
        gl.bind_framebuffer(GL_DRAW_FRAMEBUFFER, target);
        (gl.blit_framebuffer)(
            0, 0, sw, sh,
            x, bottom, x + w, top,
            GL_COLOR_BUFFER_BIT,
            GL_LINEAR as u32,
        );
        gl.bind_framebuffer(GL_READ_FRAMEBUFFER, target);
        gl.bind_framebuffer(GL_DRAW_FRAMEBUFFER, target);
        if scissoring {
            (gl.enable)(GL_SCISSOR_TEST);
        }

        // No report_swap here, deliberately — two reasons. It exists to feed
        // display-resample's refresh estimate, and Linux paces against the
        // audio clock instead. And it takes mpv's internal render lock, which
        // is the lock the worker holds while rendering: the one call that
        // made the interface wait for the worker was this one, and waiting on
        // the worker is the entire thing this thread arrangement exists to
        // end.
        self.note_present((sw, sh));
        Some(true)
    }

    pub fn error(&self) -> Option<String> {
        self.shared.error.lock().unwrap().clone()
    }

    /// Not applicable: the timeshift buffer belongs to the other pipeline,
    /// which reads from a file it is writing. mpv reads the stream directly.
    pub fn set_discarded(&self, _fraction: f64) {}

    /// Whether this stream actually carries captions.
    ///
    /// Counted by walking the track list for subtitle tracks, rather than
    /// asking how many tracks there are at all — which was the old test, and
    /// was true of every file with a video stream, so the button was offered
    /// everywhere and did nothing almost everywhere.
    pub fn captions_available(&self) -> bool {
        let count = self.get_i64("track-list/count").unwrap_or(0);
        (0..count).any(|i| {
            self.get_string(&format!("track-list/{i}/type"))
                .is_some_and(|kind| kind == "sub")
        })
    }

    pub fn captions_on(&self) -> bool {
        self.get_flag("sub-visibility") && self.get_i64("sid").is_some()
    }

    pub fn set_captions(&self, on: bool) {
        self.set_flag("sub-visibility", on);
        if !on {
            self.command(&["set", "sid", "no"]);
            return;
        }

        // Pick the caption track by its own id rather than asking for "auto".
        //
        // "auto" chooses a track at the moment a file is loaded, and a closed
        // caption track does not exist then: mpv builds it only once the
        // decoder has actually seen caption data in the video, which is some
        // way into playback. By the time the button is pressed the track is
        // there and nothing has selected it, so "auto" resolves to nothing at
        // all and the button appears to do nothing.
        let count = self.get_i64("track-list/count").unwrap_or(0);
        let track = (0..count).find(|i| {
            self.get_string(&format!("track-list/{i}/type"))
                .is_some_and(|kind| kind == "sub")
        });
        match track.and_then(|i| self.get_i64(&format!("track-list/{i}/id"))) {
            Some(id) => {
                self.command(&["set", "sid", &id.to_string()]);
            }
            // Nothing to select yet. "auto" is still the right thing to ask
            // for: if a track appears later, mpv will take it.
            None => {
                self.command(&["set", "sid", "auto"]);
            }
        }
    }

    /// None, always: mpv draws captions into the frame rather than handing
    /// text back, so there is nothing here for the interface to render.
    pub fn caption(&self) -> Option<String> {
        None
    }

    pub fn set_paused(&self, paused: bool) {
        self.set_flag("pause", paused);
    }

    pub fn set_volume(&self, level: f64) {
        // mpv is 0 to 100 where the rest of this application is 0 to 1.
        self.set_f64("volume", (level.clamp(0.0, 1.0)) * 100.0);
    }

    pub fn position(&self) -> Option<f64> {
        self.get_f64("time-pos")
    }

    pub fn duration(&self) -> Option<f64> {
        self.get_f64("duration").filter(|d| *d > 0.0)
    }

    /// From zero, because mpv reports time from the start of the item whatever
    /// the container's timestamps are. The built-in pipeline reports raw
    /// stream time, which is why it needs an origin subtracted everywhere.
    pub fn seek_range(&self) -> Option<(f64, f64)> {
        self.duration().map(|d| (0.0, d))
    }

    pub fn is_seekable(&self) -> bool {
        self.get_flag("seekable")
    }

    pub fn seek_to(&self, secs: f64) -> bool {
        self.is_seekable() && self.command(&["seek", &format!("{secs:.3}"), "absolute"])
    }

    pub fn seek_by(&self, delta: f64) -> bool {
        self.is_seekable() && self.command(&["seek", &format!("{delta:.3}"), "relative"])
    }

    pub fn seek_to_live(&self) -> bool {
        // The end of what is available, which for a growing playlist is the
        // live edge.
        self.command(&["seek", "100", "absolute-percent"])
    }

    pub fn behind_live(&self) -> Option<f64> {
        let (_, end) = self.seek_range()?;
        let position = self.position()?;
        Some((end - position).max(0.0))
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // Before anything else. mpv's frame callback holds a pointer into the
        // `Shared` this player owns, and mpv calls it from its own thread; a
        // player torn down without `release_gl` having run would otherwise
        // leave that call aimed at freed memory.
        let render_ctx = *self.render_ctx.lock().unwrap();
        if !render_ctx.0.is_null() {
            unsafe {
                (self.api.render_set_update_callback)(
                    render_ctx.0,
                    noop_frame_ready,
                    std::ptr::null_mut(),
                )
            };
        }
        self.shared.quit.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }

        // The render thread next, and before the handle: the worker owns the
        // render context, and mpv forbids destroying a handle while one
        // exists. Joining it is what frees it, on the context that made it.
        #[cfg(target_os = "linux")]
        if let Some(Some(link)) = self.threaded.get() {
            link.stop.store(true, Ordering::SeqCst);
            link.wake.notify_all();
            if let Some(worker) = self.render_thread.lock().unwrap().take() {
                let _ = worker.join();
            }
            if !link.render_ctx.lock().unwrap().0.is_null() {
                crate::log::line("[mpv] the render thread left its renderer behind");
                return;
            }
        }

        // A last resort, and it should never fire.
        //
        // mpv forbids destroying a handle while a render context belonging to
        // it still exists, and doing so takes the process with it. Every path
        // that ends a player is supposed to call `release_gl` first, from the
        // interface thread where OpenGL is current. If one ever does not, this
        // is a leaked renderer and a warning in the log — which is a far better
        // outcome than a crash on the way out of a recording.
        let render_ctx = *self.render_ctx.lock().unwrap();
        if !render_ctx.0.is_null() {
            crate::log::line("[mpv] renderer outlived its player; not freeing it here");
        } else {
            unsafe { (self.api.terminate)(self.ctx.0) };
        }
    }
}

unsafe extern "C" fn noop_frame_ready(_data: *mut c_void) {}

/// The size mpv is actually producing, or zeros if it is not producing yet.
fn picture_size(api: &Api, ctx: *mut c_void) -> (usize, usize) {
    let read = |name: &str| -> usize {
        let Ok(name) = CString::new(name) else {
            return 0;
        };
        let mut value = 0i64;
        let rc = unsafe {
            (api.get_property)(
                ctx,
                name.as_ptr(),
                MPV_FORMAT_INT64,
                &mut value as *mut i64 as *mut c_void,
            )
        };
        if rc < 0 {
            0
        } else {
            value.max(0) as usize
        }
    };
    (read("dwidth"), read("dheight"))
}

fn event_loop(api: &'static Api, ctx: Ptr, shared: Arc<Shared>, resume_at: f64) {
    let mut resumed = resume_at <= 1.0;
    let mut announced = (0usize, 0usize);

    while !shared.quit.load(Ordering::SeqCst) {
        // A real wait, not a poll. Rendering happens on the interface thread
        // now, so there is nothing for this one to do between events except
        // block, and blocking is what keeps an idle player off the processor.
        let event = unsafe { (api.wait_event)(ctx.0, 0.25) };
        if event.is_null() {
            continue;
        }
        let id = unsafe { (*event).event_id };
        match id {
            MPV_EVENT_NONE => continue,
            MPV_EVENT_SHUTDOWN => shared.quit.store(true, Ordering::SeqCst),
            MPV_EVENT_END_FILE => {
                // keep-open holds the last frame, so this is not fatal on its
                // own; it becomes an error only if nothing ever played.
                crate::log::line("[mpv] end of file");
            }
            MPV_EVENT_VIDEO_RECONFIG => {
                // Where the size actually comes from. It is emphatically not
                // available at file-loaded: mpv has parsed the file by then but
                // has not configured a video output, so `dwidth` reads back as
                // zero while `dheight` already reads 1080. Believing that pair
                // meant a picture that never arrived, with audio playing over
                // it and nothing in the log to say why.
                let size = picture_size(api, ctx.0);
                if size.0 > 0 && size.1 > 0 {
                    shared
                        .size
                        .store((size.0 as u64) << 32 | size.1 as u64, Ordering::Relaxed);
                    if size != announced {
                        announced = size;
                        crate::log::line(&format!("[mpv] video {}x{}", size.0, size.1));
                    }
                }
            }
            MPV_EVENT_FILE_LOADED => {
                crate::log::line("[mpv] loaded");
                // Resuming, now that there is something to seek within.
                if !resumed {
                    resumed = true;
                    let seek = CString::new("seek").unwrap();
                    let to = CString::new(format!("{resume_at:.3}")).unwrap();
                    let mode = CString::new("absolute").unwrap();
                    let argv = [seek.as_ptr(), to.as_ptr(), mode.as_ptr(), std::ptr::null()];
                    unsafe { (api.command)(ctx.0, argv.as_ptr()) };
                }
            }
            MPV_EVENT_LOG_MESSAGE => {
                let message = unsafe { &*((*event).data as *const LogMessage) };
                let prefix = unsafe { CStr::from_ptr(message.prefix) }.to_string_lossy();
                let text = unsafe { CStr::from_ptr(message.text) }.to_string_lossy();
                crate::log::line(&format!("[mpv/{prefix}] {}", text.trim_end()));
            }
            _ => {}
        }

        // Belt as well as braces. video-reconfig is the event that carries the
        // size, and it does arrive — but a client that misses one would show a
        // spinner over playing audio forever, which is a bad enough failure to
        // be worth one property read on a thread that is otherwise asleep.
        if shared.size.load(Ordering::Relaxed) == 0 {
            let size = picture_size(api, ctx.0);
            if size.0 > 0 && size.1 > 0 {
                shared
                    .size
                    .store((size.0 as u64) << 32 | size.1 as u64, Ordering::Relaxed);
            }
        }

        (shared.repaint)();
    }
}

/// The render thread: mpv on its own OpenGL context, Linux only.
///
/// eframe owns the window's context and never hands out the native handle,
/// which is why mpv originally rented space inside egui's paint. But EGL will
/// name the *current* context if asked during a paint, and that handle is
/// enough to create a shared sibling. The worker renders into textures both
/// contexts can see; the interface waits on a GPU fence and blits. However
/// long the driver stalls the renderer — and through virtio it was measured
/// stalling it for most of every frame — it stalls a thread nobody is typing
/// at.
///
/// Everything here fails toward the in-paint path: no EGL, no current
/// context, no shared context, no renderer — each is a log line and a clean
/// fallback, never a broken player.
#[cfg(target_os = "linux")]
mod worker {
    use super::*;

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

    impl Egl {
        fn load() -> Option<Egl> {
            let lib = crate::platform::open_library("libEGL.so.1");
            if lib.is_null() {
                return None;
            }
            unsafe {
                macro_rules! sym {
                    ($name:literal) => {{
                        let p = crate::platform::library_symbol(lib, concat!($name, "\0").as_ptr());
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
        }
    }

    /// What the two threads share.
    pub struct Link {
        pub state: Mutex<Frames>,
        pub wake: std::sync::Condvar,
        pub stop: AtomicBool,
        /// The worker failed and cleaned up entirely; use the in-paint path.
        pub dead: AtomicBool,
        /// A frame or a resize is waiting. Latched, because a condvar notify
        /// with nobody waiting evaporates: mpv announces frames while the
        /// worker is mid-render, and without this the announcement was lost
        /// and the worker slept its whole backstop — sixty frames a second
        /// collapsing to the timeout rate, which looked like the fix making
        /// everything worse.
        pub pending: AtomicBool,
        /// mpv's renderer, owned by the worker. Here rather than on Player so
        /// the thread can hold it without holding the player.
        pub render_ctx: Mutex<Ptr>,
    }

    pub struct Frames {
        pub tex: [u32; 2],
        pub front: usize,
        pub width: i32,
        pub height: i32,
        pub want: (i32, i32),
        pub generation: u64,
        pub painted: bool,
    }

    /// Capture the paint thread's context and start the worker. Called once,
    /// from a paint, where that context is current.
    pub fn spawn(player: &Player) -> Option<Arc<Link>> {
        let egl = Egl::load()?;
        let (display, share) = unsafe {
            ((egl.get_current_display)(), (egl.get_current_context)())
        };
        if display.is_null() || share.is_null() {
            crate::log::line("[mpv] no EGL context is current; rendering stays on the paint thread");
            return None;
        }

        let link = Arc::new(Link {
            state: Mutex::new(Frames {
                tex: [0, 0],
                front: 0,
                width: 0,
                height: 0,
                want: (0, 0),
                generation: 0,
                painted: false,
            }),
            wake: std::sync::Condvar::new(),
            stop: AtomicBool::new(false),
            dead: AtomicBool::new(false),
            pending: AtomicBool::new(true),
            render_ctx: Mutex::new(Ptr(std::ptr::null_mut())),
        });

        let api = player.api;
        let mpv = player.ctx;
        let shared = player.shared.clone();
        let thread_link = link.clone();
        let (display, share) = (display as usize, share as usize);
        let handle = std::thread::Builder::new()
            .name("clicker-render".into())
            .spawn(move || {
                run(egl, display, share, api, mpv, shared, thread_link.clone());
                thread_link.dead.store(true, Ordering::SeqCst);
            })
            .ok()?;
        *player.render_thread.lock().unwrap() = Some(handle);
        crate::log::line("[mpv] rendering on its own thread and context");
        Some(link)
    }

    /// mpv's "a frame is ready", aimed at the worker's condvar.
    unsafe extern "C" fn frame_ready(data: *mut c_void) {
        let link = &*(data as *const Link);
        link.pending.store(true, Ordering::Release);
        link.wake.notify_all();
    }

    fn run(
        egl: Egl,
        display: usize,
        share: usize,
        api: &'static Api,
        mpv: Ptr,
        shared: Arc<Shared>,
        link: Arc<Link>,
    ) {
        let display = display as *mut c_void;
        let share = share as *mut c_void;
        unsafe {
            (egl.bind_api)(EGL_OPENGL_API);
            // A sibling of the interface's context: same share group, so
            // textures made here are visible there. No config and no surface,
            // which Mesa allows and which is all a renderer that only ever
            // draws into framebuffers needs.
            let attribs = [
                EGL_CONTEXT_MAJOR_VERSION, 3,
                EGL_CONTEXT_MINOR_VERSION, 0,
                EGL_NONE,
            ];
            let ctx = (egl.create_context)(display, std::ptr::null_mut(), share, attribs.as_ptr());
            if ctx.is_null() {
                crate::log::line("[mpv] could not create a shared context; rendering stays on the paint thread");
                return;
            }
            if (egl.make_current)(display, std::ptr::null_mut(), std::ptr::null_mut(), ctx) == 0 {
                crate::log::line("[mpv] could not make the shared context current; rendering stays on the paint thread");
                (egl.destroy_context)(display, ctx);
                return;
            }

            let gl = match GlFns::load() {
                Ok(gl) => gl,
                Err(e) => {
                    crate::log::line(&format!("[mpv] {e}; rendering stays on the paint thread"));
                    (egl.make_current)(display, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
                    (egl.destroy_context)(display, ctx);
                    return;
                }
            };

            // mpv's renderer, against this context.
            let api_type = CString::new("opengl").unwrap();
            let mut init = OpenGlInitParams {
                get_proc_address: gl_proc_address,
                get_proc_address_ctx: std::ptr::null_mut(),
            };
            let mut params = [
                RenderParam { kind: MPV_RENDER_PARAM_API_TYPE, data: api_type.as_ptr() as *mut c_void },
                RenderParam { kind: MPV_RENDER_PARAM_OPENGL_INIT_PARAMS, data: &mut init as *mut OpenGlInitParams as *mut c_void },
                RenderParam { kind: MPV_RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
            ];
            let mut created: *mut c_void = std::ptr::null_mut();
            let rc = (api.render_create)(&mut created, mpv.0, params.as_mut_ptr());
            if rc < 0 {
                let why = CStr::from_ptr((api.error_string)(rc)).to_string_lossy();
                crate::log::line(&format!("[mpv] renderer refused the shared context: {why}"));
                (egl.make_current)(display, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
                (egl.destroy_context)(display, ctx);
                return;
            }
            *link.render_ctx.lock().unwrap() = Ptr(created);
            (api.render_set_update_callback)(created, frame_ready, Arc::as_ptr(&link) as *mut c_void);

            let mut fbos = [0u32; 2];
            let mut texs = [0u32; 2];

            loop {
                if link.stop.load(Ordering::SeqCst) {
                    break;
                }
                // Work first, sleep only when the latch is clear. The latch is
                // what makes announcements made mid-render count; the timeout
                // is only a backstop against a wake lost to a race with it.
                if !link.pending.swap(false, Ordering::AcqRel) {
                    let st = link.state.lock().unwrap();
                    let _unused = link
                        .wake
                        .wait_timeout(st, std::time::Duration::from_millis(50))
                        .unwrap();
                    continue;
                }

                // The stream's own size, for the clamp; not configured yet
                // means nothing to draw.
                let packed = shared.size.load(Ordering::Relaxed);
                let (vw, vh) = ((packed >> 32) as i32, packed as i32);
                if vw <= 0 || vh <= 0 {
                    continue;
                }

                let want = link.state.lock().unwrap().want;
                if want.0 <= 0 || want.1 <= 0 {
                    continue;
                }
                // Same sizing as the in-paint path: the width quantized so a
                // drag does not reallocate per pixel, the height following by
                // the rectangle's exact ratio, the whole thing clamped to the
                // stream.
                let round32 = |n: i32| ((n + 31) / 32 * 32).max(32);
                let width = round32(want.0).min(vw);
                let height = ((((width as i64 * want.1 as i64) / want.0.max(1) as i64) as i32) & !1).max(2);

                let stale = {
                    let st = link.state.lock().unwrap();
                    st.width != width || st.height != height || st.tex[0] == 0
                };
                if stale {
                    for fbo in fbos {
                        if fbo != 0 { gl.delete_framebuffers(1, &fbo); }
                    }
                    for tex in texs {
                        if tex != 0 { gl.delete_textures(1, &tex); }
                    }
                    let previous = gl.draw_framebuffer();
                    let mut ok = true;
                    for i in 0..2 {
                        let mut tex = 0u32;
                        gl.gen_textures(1, &mut tex);
                        gl.bind_texture(GL_TEXTURE_2D, tex);
                        gl.tex_image_2d(GL_TEXTURE_2D, 0, GL_RGBA8, width, height, 0, GL_RGBA, GL_UNSIGNED_BYTE, std::ptr::null());
                        gl.tex_parameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
                        gl.tex_parameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
                        gl.tex_parameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
                        gl.tex_parameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
                        gl.bind_texture(GL_TEXTURE_2D, 0);
                        let mut fbo = 0u32;
                        gl.gen_framebuffers(1, &mut fbo);
                        gl.bind_framebuffer(GL_FRAMEBUFFER, fbo);
                        gl.framebuffer_texture_2d(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, tex, 0);
                        if gl.check_framebuffer_status(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE {
                            ok = false;
                        }
                        texs[i] = tex;
                        fbos[i] = fbo;
                    }
                    gl.bind_framebuffer(GL_FRAMEBUFFER, previous);
                    if !ok {
                        crate::log::line("[mpv] the driver refused the shared render target");
                        break;
                    }
                    // The textures exist for the other context once flushed.
                    (gl.flush)();
                    let mut st = link.state.lock().unwrap();
                    st.tex = texs;
                    st.front = 0;
                    st.width = width;
                    st.height = height;
                    st.generation += 1;
                    st.painted = false;
                }

                if (api.render_update)(created) & MPV_RENDER_UPDATE_FRAME == 0 {
                    continue;
                }

                let back = link.state.lock().unwrap().front ^ 1;
                let cpu_before = thread_cpu_ms();
                let wall_before = Instant::now();

                let mut fbo = OpenGlFbo {
                    fbo: fbos[back] as c_int,
                    w: width,
                    h: height,
                    internal_format: 0,
                };
                let mut flip: c_int = 1;
                // Render immediately, do not wait for the frame's due time.
                // Blocking sounded right for a thread built to wait, and froze
                // the interface instead: mpv holds its internal render lock
                // for the whole wait, the interface's report_swap wanted that
                // same lock once per paint, and the window sat behind the
                // worker's sleep. The latch above already paces this loop to
                // mpv's announcements; nothing needs the wait.
                let mut block: c_int = 0;
                let mut render_params = [
                    RenderParam { kind: MPV_RENDER_PARAM_OPENGL_FBO, data: &mut fbo as *mut OpenGlFbo as *mut c_void },
                    RenderParam { kind: MPV_RENDER_PARAM_FLIP_Y, data: &mut flip as *mut c_int as *mut c_void },
                    RenderParam { kind: MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME, data: &mut block as *mut c_int as *mut c_void },
                    RenderParam { kind: MPV_RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
                ];
                let rc = (api.render)(created, render_params.as_mut_ptr());
                if rc < 0 {
                    shared.dropped.fetch_add(1, Ordering::Relaxed);
                    continue;
                }

                // Wait here, on the thread built for waiting, until the GPU
                // has fully finished the frame — then publish. The first
                // version handed the interface a fence to wait on instead,
                // and on the one driver this thread exists for, cross-context
                // fences are precisely the broken primitive: the interface
                // sampled textures the fence had not really protected, which
                // was the white flashing that got this whole thread parked.
                // A published frame is now complete by construction; the
                // interface never synchronizes at all.
                (gl.finish)();
                {
                    let mut st = link.state.lock().unwrap();
                    st.front = back;
                    st.painted = true;
                }
                shared.rendered.fetch_add(1, Ordering::Relaxed);

                // The same numbers the in-paint path keeps, measured where the
                // work now happens. Wall time here costs nobody anything, but
                // it is still the honest record of what the driver is doing.
                let spent_ms = wall_before.elapsed().as_secs_f32() * 1000.0;
                {
                    let mut smoothed = shared.render_ms.lock().unwrap();
                    *smoothed = if *smoothed > 0.0 { *smoothed * 0.85 + spent_ms * 0.15 } else { spent_ms };
                }
                let cpu = thread_cpu_ms() - cpu_before;
                let mut last = shared.last_render.lock().unwrap();
                if let Some(previous) = last.replace(wall_before) {
                    let interval = wall_before.duration_since(previous).as_secs_f64() * 1000.0;
                    if interval > 0.0 {
                        let load = (cpu / interval) as f32;
                        let mut smoothed = shared.render_load.lock().unwrap();
                        *smoothed = if *smoothed > 0.0 { *smoothed * 0.9 + load * 0.1 } else { load };
                    }
                }
                drop(last);

                // A frame is ready; ask the interface to come and get it.
                (shared.repaint)();
            }

            // Tear down on the thread and context that own it all.
            (api.render_set_update_callback)(created, noop_frame_ready, std::ptr::null_mut());
            (api.render_free)(created);
            *link.render_ctx.lock().unwrap() = Ptr(std::ptr::null_mut());
            {
                let mut st = link.state.lock().unwrap();
                st.painted = false;
                st.tex = [0, 0];
            }
            for fbo in fbos {
                if fbo != 0 { gl.delete_framebuffers(1, &fbo); }
            }
            for tex in texs {
                if tex != 0 { gl.delete_textures(1, &tex); }
            }
            (egl.make_current)(display, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
            (egl.destroy_context)(display, ctx);
            (egl.release_thread)();
        }
    }
}

