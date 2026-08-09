//! Play a URL through libmpv and report what it did.
//!
//! The counterpart to `Player::self_test`, printing the same shape of numbers
//! so the two can be compared on the same source. That comparison is the whole
//! point: the question is not whether libmpv works, it is whether it handles
//! the files the built-in pipeline struggles with, and the only honest way to
//! answer that is to point both at one and read the counters.
//!
//! ```powershell
//! cargo run --release --example mpv_probe -- <url> [seconds]
//! ```
//!
//! libmpv is loaded at runtime rather than linked. The prebuilt LGPL package
//! ships a mingw import library, which an MSVC target cannot use, and loading
//! by name sidesteps that entirely — no second toolchain, no generated import
//! library, and a missing DLL is a message rather than a program that will not
//! start. `scripts\fetch-mpv.ps1` puts it in place.
//!
//! Rendering is libmpv's software path: mpv composites into a buffer we own
//! and we would upload that as a texture, which is exactly what the built-in
//! player does with the frames it decodes. The interface keeps drawing over
//! the picture in one pass, and nothing about the window changes.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::time::{Duration, Instant};

// --- the slice of the C API this needs ---------------------------------------

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

// --- loading it ---------------------------------------------------------------

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

macro_rules! api {
    ($($field:ident: $name:literal => fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        struct Api { $($field: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)* }
        impl Api {
            /// Every symbol resolved up front, so a package missing one fails
            /// here by name rather than at whatever moment it is first called.
            unsafe fn load(module: *mut c_void) -> Result<Self, String> {
                $(
                    let $field = GetProcAddress(module, concat!($name, "\0").as_ptr());
                    if $field.is_null() {
                        return Err(format!("{} is missing from the library", $name));
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
    request_log: "mpv_request_log_messages" => fn(*mut c_void, *const c_char) -> c_int;
    error_string: "mpv_error_string" => fn(c_int) -> *const c_char;
    render_create: "mpv_render_context_create" => fn(*mut *mut c_void, *mut c_void, *mut RenderParam) -> c_int;
    render_update: "mpv_render_context_update" => fn(*mut c_void) -> u64;
    render: "mpv_render_context_render" => fn(*mut c_void, *mut RenderParam) -> c_int;
    render_free: "mpv_render_context_free" => fn(*mut c_void);
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(url) = args.next() else {
        eprintln!("usage: mpv_probe <url> [seconds]");
        std::process::exit(2);
    };
    let seconds: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(30.0);

    // Beside the executable first, then the vendored copy, so a staged build
    // and a development tree both work without arguments.
    let candidates = [
        std::env::var("MPV_DLL").unwrap_or_default(),
        "libmpv-2.dll".into(),
        "third_party/mpv/libmpv-2.dll".into(),
    ];

    let module = candidates
        .iter()
        .filter(|path| !path.is_empty())
        .find_map(|path| {
            let handle = unsafe { LoadLibraryW(wide(path).as_ptr()) };
            (!handle.is_null()).then(|| {
                eprintln!("[mpv] loaded {path}");
                handle
            })
        });
    let Some(module) = module else {
        eprintln!("[mpv] could not load libmpv-2.dll; run scripts\\fetch-mpv.ps1");
        std::process::exit(1);
    };

    let api = match unsafe { Api::load(module) } {
        Ok(api) => api,
        Err(e) => {
            eprintln!("[mpv] {e}");
            std::process::exit(1);
        }
    };

    unsafe { run(&api, &url, seconds) }
}

unsafe fn run(api: &Api, url: &str, seconds: f64) {
    let check = |api: &Api, what: &str, rc: c_int| {
        if rc < 0 {
            let message = CStr::from_ptr((api.error_string)(rc)).to_string_lossy();
            eprintln!("[mpv] {what}: {message}");
            std::process::exit(1);
        }
    };

    let ctx = (api.create)();
    if ctx.is_null() {
        eprintln!("[mpv] mpv_create failed");
        std::process::exit(1);
    }

    // Software decoding, to compare like with like: the built-in pipeline
    // decodes on the CPU, so letting mpv reach for the GPU would measure two
    // different things.
    // Not fatal when one is unknown. A libmpv-only build has no command line
    // player and no scripting, so options belonging to those — `osc` is the
    // on-screen controller, drawn by a Lua script — simply do not exist in it,
    // and refusing to start because a thing we wanted off is already absent
    // would be daft.
    for (name, value) in [("vo", "libmpv"), ("hwdec", "no"), ("terminal", "no")] {
        let (name, value) = (CString::new(name).unwrap(), CString::new(value).unwrap());
        let rc = (api.set_option)(ctx, name.as_ptr(), value.as_ptr());
        if rc < 0 {
            let message = CStr::from_ptr((api.error_string)(rc)).to_string_lossy();
            eprintln!("[mpv] option {}: {message}", name.to_string_lossy());
        }
    }

    // What the built-in player cannot currently give a user: a running account
    // of what the decoder thinks is happening, on their machine, in a file
    // they can send back.
    let level = CString::new("info").unwrap();
    (api.request_log)(ctx, level.as_ptr());

    check(api, "initialize", (api.initialize)(ctx));

    let sw = CString::new("sw").unwrap();
    let mut render_ctx: *mut c_void = std::ptr::null_mut();
    let mut params = [
        RenderParam { kind: MPV_RENDER_PARAM_API_TYPE, data: sw.as_ptr() as *mut c_void },
        RenderParam { kind: MPV_RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
    ];
    check(api, "render_create", (api.render_create)(&mut render_ctx, ctx, params.as_mut_ptr()));

    let load = CString::new("loadfile").unwrap();
    let target = CString::new(url).unwrap();
    let argv = [load.as_ptr(), target.as_ptr(), std::ptr::null()];
    check(api, "loadfile", (api.command)(ctx, argv.as_ptr()));

    // A fixed surface, because this is measuring the pipeline rather than a
    // window. The real integration would use the size of the video rect.
    const W: usize = 1280;
    const H: usize = 720;
    let mut surface = vec![0u8; W * H * 4];

    let started = Instant::now();
    let mut last_report = Instant::now();
    let mut rendered = 0u64;
    let mut rendered_at_report = 0u64;
    let mut loaded = false;

    while started.elapsed().as_secs_f64() < seconds {
        // Events first: they carry the log lines and the end of the file.
        loop {
            let event = (api.wait_event)(ctx, 0.0);
            if event.is_null() || (*event).event_id == MPV_EVENT_NONE {
                break;
            }
            match (*event).event_id {
                MPV_EVENT_SHUTDOWN => {
                    eprintln!("[mpv] shutdown");
                    return cleanup(api, ctx, render_ctx);
                }
                MPV_EVENT_END_FILE => {
                    eprintln!("[mpv] end of file after {:.1}s", started.elapsed().as_secs_f64());
                    return cleanup(api, ctx, render_ctx);
                }
                MPV_EVENT_FILE_LOADED => {
                    loaded = true;
                    eprintln!("[mpv] file loaded in {:.2}s", started.elapsed().as_secs_f64());
                }
                MPV_EVENT_LOG_MESSAGE => {
                    let message = &*((*event).data as *const LogMessage);
                    let prefix = CStr::from_ptr(message.prefix).to_string_lossy();
                    let text = CStr::from_ptr(message.text).to_string_lossy();
                    eprint!("[mpv/{prefix}] {text}");
                }
                _ => {}
            }
        }

        if (api.render_update)(render_ctx) & MPV_RENDER_UPDATE_FRAME != 0 {
            let mut size = [W as c_int, H as c_int];
            let mut stride: usize = W * 4;
            let format = CString::new("rgb0").unwrap();
            let mut frame = [
                RenderParam { kind: MPV_RENDER_PARAM_SW_SIZE, data: size.as_mut_ptr() as *mut c_void },
                RenderParam { kind: MPV_RENDER_PARAM_SW_FORMAT, data: format.as_ptr() as *mut c_void },
                RenderParam { kind: MPV_RENDER_PARAM_SW_STRIDE, data: &mut stride as *mut _ as *mut c_void },
                RenderParam { kind: MPV_RENDER_PARAM_SW_POINTER, data: surface.as_mut_ptr() as *mut c_void },
                RenderParam { kind: MPV_RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
            ];
            let rc = (api.render)(render_ctx, frame.as_mut_ptr());
            if rc >= 0 {
                rendered += 1;
            }
        } else {
            std::thread::sleep(Duration::from_millis(2));
        }

        if loaded && last_report.elapsed() > Duration::from_secs(2) {
            let window = last_report.elapsed().as_secs_f64();
            last_report = Instant::now();

            let double = |name: &str| -> f64 {
                let name = CString::new(name).unwrap();
                let mut value = f64::NAN;
                (api.get_property)(ctx, name.as_ptr(), MPV_FORMAT_DOUBLE, &mut value as *mut _ as *mut c_void);
                value
            };
            let int = |name: &str| -> i64 {
                let name = CString::new(name).unwrap();
                let mut value = 0i64;
                (api.get_property)(ctx, name.as_ptr(), MPV_FORMAT_INT64, &mut value as *mut _ as *mut c_void);
                value
            };

            eprintln!(
                "[mpv] {:.1}s pos {:.1}s | rendered {:.2} fps | dropped {} (decoder {}) | cache {:.1}s | avsync {:+.0}ms",
                started.elapsed().as_secs_f64(),
                double("time-pos"),
                (rendered - rendered_at_report) as f64 / window,
                int("frame-drop-count"),
                int("decoder-frame-drop-count"),
                double("demuxer-cache-duration"),
                double("avsync") * 1000.0,
            );
            rendered_at_report = rendered;
        }
    }

    eprintln!(
        "[mpv] done: {} frames rendered in {:.1}s, {:.2} fps average",
        rendered,
        started.elapsed().as_secs_f64(),
        rendered as f64 / started.elapsed().as_secs_f64()
    );
    cleanup(api, ctx, render_ctx)
}

unsafe fn cleanup(api: &Api, ctx: *mut c_void, render_ctx: *mut c_void) {
    // The render context has to go before the core, or the core tears down a
    // renderer that is still attached to it.
    if !render_ctx.is_null() {
        (api.render_free)(render_ctx);
    }
    (api.terminate)(ctx);
}
