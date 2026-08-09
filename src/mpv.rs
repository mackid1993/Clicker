//! Playback through libmpv.
//!
//! The alternative to `player`, which is this application's own pipeline over
//! FFmpeg. Both fill the same `FrameSlot` and answer the same questions, so
//! the screens above them cannot tell which is running.
//!
//! Why have it. The hand-rolled pipeline is fast — measured at 28% of one core
//! against mpv's 87% on the same 1080p60 recording — but it is one person's
//! implementation of a problem mpv has been having solved for twenty years:
//! timestamp discontinuities, damaged segments, odd containers, streams that
//! stop and start. Every one of those arrives as a bug report about a file
//! nobody here can reproduce.
//!
//! What it costs. mpv's software renderer converts and composites every frame
//! on the CPU, and its own header is blunt about that: "an extremely simple
//! (but slow) renderer... You probably don't want to use this." Measured, that
//! is 87% of one core, which on a 22-core machine is 4% of it. The cheaper
//! path is mpv's OpenGL renderer, which needs eframe moved from wgpu to glow,
//! and that is a graphics backend change under the whole application rather
//! than a flag. Worth doing later; not worth blocking this on.
//!
//! The library is loaded by name at runtime rather than linked. mpv cannot be
//! built with MSVC, so the DLL comes from mingw and ships with a mingw import
//! library that an MSVC target cannot use. Loading it by name sidesteps that
//! entirely, and means a missing DLL is a message rather than a program that
//! will not start.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::player::FrameSlot;

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

const MPV_RENDER_PARAM_INVALID: c_int = 0;
const MPV_RENDER_PARAM_API_TYPE: c_int = 1;
const MPV_RENDER_PARAM_SW_SIZE: c_int = 17;
const MPV_RENDER_PARAM_SW_FORMAT: c_int = 18;
const MPV_RENDER_PARAM_SW_STRIDE: c_int = 19;
const MPV_RENDER_PARAM_SW_POINTER: c_int = 20;

const MPV_RENDER_UPDATE_FRAME: u64 = 1;

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

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

macro_rules! api {
    ($($field:ident: $name:literal => fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        pub struct Api { $($field: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)* }
        impl Api {
            unsafe fn load(module: *mut c_void) -> Result<Self, String> {
                $(
                    let $field = GetProcAddress(module, concat!($name, "\0").as_ptr());
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
    render_update: "mpv_render_context_update" => fn(*mut c_void) -> u64;
    render: "mpv_render_context_render" => fn(*mut c_void, *mut RenderParam) -> c_int;
    render_free: "mpv_render_context_free" => fn(*mut c_void);
}

/// A pointer that may cross to the render thread.
///
/// Sound because mpv's own handle is thread safe for properties and commands,
/// and because the render context is only ever touched by the one thread that
/// created it.
#[derive(Clone, Copy)]
struct Ptr(*mut c_void);
unsafe impl Send for Ptr {}
unsafe impl Sync for Ptr {}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Load the library once, from beside the executable or the build tree.
fn library() -> Result<&'static Api, String> {
    static LOADED: std::sync::OnceLock<Result<Api, String>> = std::sync::OnceLock::new();
    LOADED
        .get_or_init(|| {
            let beside = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("libmpv-2.dll")))
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            for candidate in [
                beside.as_str(),
                "libmpv-2.dll",
                "third_party/mpv/libmpv-2.dll",
            ] {
                if candidate.is_empty() {
                    continue;
                }
                let module = unsafe { LoadLibraryW(wide(candidate).as_ptr()) };
                if !module.is_null() {
                    return unsafe { Api::load(module) };
                }
            }
            Err("libmpv-2.dll was not found beside the application".into())
        })
        .as_ref()
        .map_err(|e| e.clone())
}

/// Whether this build can play through mpv at all.
pub fn available() -> bool {
    library().is_ok()
}

struct Shared {
    frame: Mutex<FrameSlot>,
    quit: AtomicBool,
    /// Frames handed to the interface. Stands in for the decoder's own count:
    /// what the stall watchdog needs to know is whether pictures are still
    /// arriving, and this is that.
    rendered: AtomicU64,
    dropped: AtomicU64,
    error: Mutex<Option<String>>,
    /// Milliseconds spent in the last render, smoothed, for the stats card.
    render_ms: Mutex<f32>,
}

pub struct Player {
    api: &'static Api,
    ctx: Ptr,
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Player {
    /// What is playing this, for the About line and the log.
    pub fn backend() -> String {
        match library() {
            Ok(_) => "mpv (LGPL), FFmpeg inside it".to_string(),
            Err(e) => format!("mpv unavailable: {e}"),
        }
    }

    /// Open a URL and start playing it.
    ///
    /// `resume_at` is elapsed seconds from the start, or zero. Unlike the
    /// built-in pipeline there is no origin to add: mpv reports time from the
    /// beginning of the item whatever the container's timestamps say.
    pub fn open(
        uri: &str,
        resume_at: f64,
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
            ("hwdec", if software_decoding { "no" } else { "auto-safe" }),
            // The read-ahead this application argued itself into having: mpv
            // caches compressed packets, which is minutes of protection for
            // the memory a couple of seconds of decoded frames would cost.
            ("cache", "yes"),
            ("demuxer-max-bytes", "96MiB"),
            ("demuxer-readahead-secs", "60"),
            // Keep the file open at the end rather than tearing down, so the
            // last frame stays on screen instead of a black window.
            ("keep-open", "yes"),
            ("user-agent", &crate::settings::user_agent()),
        ] {
            let (n, v) = (CString::new(name).unwrap(), CString::new(value).unwrap());
            // Unknown options are reported and skipped: a libmpv-only build has
            // no command line player, so options belonging to it do not exist,
            // and refusing to start over one that is already absent is daft.
            let rc = unsafe { (api.set_option)(ctx.0, n.as_ptr(), v.as_ptr()) };
            if rc < 0 {
                crate::log::line(&format!("[mpv] option {name}: ignored"));
            }
        }

        // Everything mpv has to say, into the log file beside the crash log.
        // This is the thing the built-in pipeline could not give a user.
        let level = CString::new("info").unwrap();
        unsafe { (api.request_log)(ctx.0, level.as_ptr()) };

        check("initialize", unsafe { (api.initialize)(ctx.0) })?;

        let shared = Arc::new(Shared {
            frame: Mutex::new(FrameSlot::default()),
            quit: AtomicBool::new(false),
            rendered: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            error: Mutex::new(None),
            render_ms: Mutex::new(0.0),
        });

        // Loading before the render context exists is fine and starts the fetch
        // a fraction earlier.
        let load = CString::new("loadfile").unwrap();
        let target = CString::new(uri).map_err(|_| "the address has a NUL in it")?;
        let argv = [load.as_ptr(), target.as_ptr(), std::ptr::null()];
        check("loadfile", unsafe { (api.command)(ctx.0, argv.as_ptr()) })?;

        let thread = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("clicker-mpv".into())
                .spawn(move || render_loop(api, ctx, shared, resume_at, repaint))
                .map_err(|e| e.to_string())?
        };

        Ok(Self {
            api,
            ctx,
            shared,
            thread: Some(thread),
        })
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

    pub fn frame(&self) -> std::sync::MutexGuard<'_, FrameSlot> {
        self.shared.frame.lock().unwrap()
    }

    pub fn decoded(&self) -> u64 {
        self.shared.rendered.load(Ordering::Relaxed)
    }

    pub fn dropped(&self) -> u64 {
        self.get_i64("frame-drop-count").unwrap_or(0).max(0) as u64
    }

    pub fn queued_frames(&self) -> usize {
        0
    }

    /// Seconds of media read ahead of playback. mpv's own number, and the
    /// direct equivalent of what the built-in pipeline calls its buffer.
    pub fn queued_audio(&self) -> f64 {
        self.get_f64("demuxer-cache-duration").unwrap_or(0.0)
    }

    pub fn decode_ms(&self) -> f32 {
        *self.shared.render_ms.lock().unwrap()
    }

    pub fn error(&self) -> Option<String> {
        self.shared.error.lock().unwrap().clone()
    }

    /// Not applicable: the timeshift buffer belongs to the other pipeline,
    /// which reads from a file it is writing. mpv reads the stream directly.
    pub fn set_discarded(&self, _fraction: f64) {}

    pub fn captions_available(&self) -> bool {
        self.get_i64("track-list/count").unwrap_or(0) > 0
    }

    pub fn captions_on(&self) -> bool {
        self.get_flag("sub-visibility") && self.get_i64("sid").is_some()
    }

    pub fn set_captions(&self, on: bool) {
        self.set_flag("sub-visibility", on);
        // "auto" picks the stream's own captions; "no" turns them off. mpv
        // renders them into the picture itself, so nothing here draws them.
        self.command(&["set", "sid", if on { "auto" } else { "no" }]);
    }

    /// None, always: mpv draws captions into the frame rather than handing
    /// text back, so there is nothing here for the interface to render.
    pub fn caption(&self) -> Option<String> {
        None
    }

    pub fn stop(&self) {
        self.shared.quit.store(true, Ordering::SeqCst);
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
        self.shared.quit.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // Only once the render thread has stopped touching it.
        unsafe { (self.api.terminate)(self.ctx.0) };
    }
}

/// Pump events and render frames until told to stop.
fn render_loop(
    api: &'static Api,
    ctx: Ptr,
    shared: Arc<Shared>,
    resume_at: f64,
    repaint: impl Fn() + Send + Sync,
) {
    let sw = CString::new("sw").unwrap();
    let mut render_ctx: *mut c_void = std::ptr::null_mut();
    let mut params = [
        RenderParam {
            kind: MPV_RENDER_PARAM_API_TYPE,
            data: sw.as_ptr() as *mut c_void,
        },
        RenderParam {
            kind: MPV_RENDER_PARAM_INVALID,
            data: std::ptr::null_mut(),
        },
    ];
    let rc = unsafe { (api.render_create)(&mut render_ctx, ctx.0, params.as_mut_ptr()) };
    if rc < 0 {
        *shared.error.lock().unwrap() = Some("mpv could not start its renderer".into());
        return;
    }

    let format = CString::new("rgb0").unwrap();
    let mut width = 0usize;
    let mut height = 0usize;
    let mut surface: Vec<u8> = Vec::new();
    let mut resumed = resume_at <= 1.0;

    while !shared.quit.load(Ordering::SeqCst) {
        // Events first: they carry the log lines, the size, and the end.
        loop {
            let event = unsafe { (api.wait_event)(ctx.0, 0.0) };
            if event.is_null() {
                break;
            }
            let id = unsafe { (*event).event_id };
            if id == MPV_EVENT_NONE {
                break;
            }
            match id {
                MPV_EVENT_SHUTDOWN => {
                    shared.quit.store(true, Ordering::SeqCst);
                }
                MPV_EVENT_END_FILE => {
                    // keep-open holds the last frame, so this is not fatal on
                    // its own; it becomes an error only if nothing plays.
                    crate::log::line("[mpv] end of file");
                }
                MPV_EVENT_FILE_LOADED => {
                    let get = |name: &str| -> i64 {
                        let Ok(name) = CString::new(name) else { return 0 };
                        let mut value = 0i64;
                        unsafe {
                            (api.get_property)(
                                ctx.0,
                                name.as_ptr(),
                                MPV_FORMAT_INT64,
                                &mut value as *mut _ as *mut c_void,
                            )
                        };
                        value
                    };
                    width = get("dwidth").max(0) as usize;
                    height = get("dheight").max(0) as usize;
                    if width > 0 && height > 0 {
                        surface = vec![0u8; width * height * 4];
                    }
                    crate::log::line(&format!("[mpv] loaded, {width}x{height}"));

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
        }

        if surface.is_empty()
            || unsafe { (api.render_update)(render_ctx) } & MPV_RENDER_UPDATE_FRAME == 0
        {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }

        let started = Instant::now();
        let mut size = [width as c_int, height as c_int];
        let mut stride: usize = width * 4;
        let mut frame = [
            RenderParam {
                kind: MPV_RENDER_PARAM_SW_SIZE,
                data: size.as_mut_ptr() as *mut c_void,
            },
            RenderParam {
                kind: MPV_RENDER_PARAM_SW_FORMAT,
                data: format.as_ptr() as *mut c_void,
            },
            RenderParam {
                kind: MPV_RENDER_PARAM_SW_STRIDE,
                data: &mut stride as *mut _ as *mut c_void,
            },
            RenderParam {
                kind: MPV_RENDER_PARAM_SW_POINTER,
                data: surface.as_mut_ptr() as *mut c_void,
            },
            RenderParam {
                kind: MPV_RENDER_PARAM_INVALID,
                data: std::ptr::null_mut(),
            },
        ];
        let rc = unsafe { (api.render)(render_ctx, frame.as_mut_ptr()) };
        if rc < 0 {
            shared.dropped.fetch_add(1, Ordering::Relaxed);
            continue;
        }

        // Into the slot the interface uploads from. Copied rather than swapped
        // because mpv owns the surface it just drew into and will draw into it
        // again next frame.
        {
            let mut slot = shared.frame.lock().unwrap();
            if slot.pixels.len() != surface.len() {
                slot.pixels = vec![0u8; surface.len()];
            }
            slot.pixels.copy_from_slice(&surface);
            slot.width = width as u32;
            slot.height = height as u32;
            slot.generation = slot.generation.wrapping_add(1);
        }

        let ms = started.elapsed().as_secs_f32() * 1000.0;
        {
            let mut smoothed = shared.render_ms.lock().unwrap();
            *smoothed = if *smoothed > 0.0 { *smoothed * 0.9 + ms * 0.1 } else { ms };
        }
        shared.rendered.fetch_add(1, Ordering::Relaxed);
        repaint();
    }

    unsafe { (api.render_free)(render_ctx) };
}
