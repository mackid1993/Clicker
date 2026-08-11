// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native Windows client for Channels DVR
// Copyright (c) 2026 David Brustein

//! Clicker — a native Channels DVR client.
//!
//! Video is decoded by FFmpeg and drawn as a texture, so the picture and the
//! interface share one render pass. That is what makes a media app pleasant to
//! build: controls float over the picture with no second window, no z-order, no
//! transparency tricks and no hit-testing to get wrong.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod backdrop;
mod downloads;
mod guide;
mod images;
mod keys;
mod library;
mod log;
mod mpv;
mod paths;
mod platform;
mod settings;
mod stream;
mod theme;
mod timeshift;
mod tray;
mod ui;
mod ui_guide;
mod ui_downloads;
mod ui_library;
mod ui_record;
mod ui_setup;

use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;
use theme::{Fluent, RADIUS_SURFACE, SPACE_L, SPACE_M, SPACE_S, SPACE_XS, TITLEBAR_HEIGHT};
use ui::Screen;

/// What the program calls itself on screen: the title bar, the tray, the
/// welcome card, the About line. One place, so it cannot be half-renamed.
pub const APP_NAME: &str = "Clicker";


/// Width of the navigation rail, collapsed and expanded. Both are Fluent's own
/// values, so it lines up with every other Windows application that uses one.
const RAIL_COLLAPSED: f32 = 48.0;
const RAIL_EXPANDED: f32 = 200.0;

/// How many hours of listings to ask for.
///
/// A full day, so tomorrow evening is reachable rather than just tonight. This
/// is the *requested* duration; what arrives reaches further, because the
/// server returns whole programs and the last of them run past the end of the
/// window. Measured against a real server with 446 channels:
///
/// | asked | airings | listings reach | request |
/// |-------|---------|----------------|---------|
/// | 12h   |   6,454 |          +21h  |   1.5s  |
/// | 24h   |  13,042 |          +32h  |   3.3s  |
/// | 48h   |  25,750 |          +55h  |   6.7s  |
///
/// Twenty-four is the knee: it doubles the reach of twelve for two seconds
/// that nobody waits through, because the guide loads in the background while
/// the home screen is already up. Forty-eight costs another three and a half
/// seconds and four times the memory of twelve to reach a day nobody is
/// planning yet.
const GUIDE_HOURS: i64 = 24;

/// Where to fetch a live channel.
///
/// Two decisions are folded in here, and both matter.
///
/// **HLS rather than `stream.mpg`.** The direct endpoint has the lowest
/// latency, but it is one long HTTP response with no ranges and no index:
/// there is nothing to seek within, and asking it to seek tears the pipeline
/// down and takes the audio with it. The DVR's HLS output keeps every segment
/// from the moment the channel was tuned, so it rewinds to the start of the
/// session with no local storage at all. A DVR that cannot rewind is not a
/// DVR.
///
/// **`vcodec=copy&acodec=copy`.** Without it Channels defaults to
/// `vcodec=h264` with a hardware deinterlacer and a 5,739kbps ceiling, which
/// is a full server-side re-encode of a stream it already had, and a
/// noticeable quality loss: passthrough measured 9,915kbps on the same
/// channel. Segmenting for HLS does not require re-encoding, so there is no
/// reason to accept it. Transcoding remains available, but as a choice.
/// One live-stream quality, picked either globally in Settings or per session
/// from the player.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum QualityChoice {
    /// The broadcast untouched: `vcodec=copy&acodec=copy`.
    Original,
    /// A server-side transcode capped at this height.
    Height(u32),
}

impl QualityChoice {
    const MENU: [QualityChoice; 5] = [
        QualityChoice::Original,
        QualityChoice::Height(1080),
        QualityChoice::Height(720),
        QualityChoice::Height(540),
        QualityChoice::Height(360),
    ];

    fn label(self) -> String {
        match self {
            QualityChoice::Original => "Original".into(),
            QualityChoice::Height(h) => format!("{h}p"),
        }
    }

    /// Bitrates that suit each size. Chosen once here so the player menu and
    /// the settings screen cannot drift apart.
    fn kbps(self) -> u32 {
        match self {
            QualityChoice::Original => 0,
            QualityChoice::Height(1080) => 8000,
            QualityChoice::Height(720) => 4000,
            QualityChoice::Height(540) => 2500,
            QualityChoice::Height(_) => 1200,
        }
    }
}

fn stream_uri(server: &str, channel: &str, quality: QualityChoice) -> String {
    match quality {
        // The tuner's own transport stream, straight through.
        //
        // Not `hls/master.m3u8?vcodec=copy`, which is what this used to ask
        // for. `vcodec=copy` means "do not re-encode", not "do not touch": the
        // HLS endpoint still stands a pipeline up to cut the stream into
        // segments. Checked against a real DVR — asking for this endpoint
        // produced no server activity at all, while the copy playlist on the
        // same channel reported "Remux Starting". Original is supposed to mean
        // the broadcast untouched, and only one of those two is.
        //
        // This is one long HTTP response with no index, so there is nothing to
        // seek within it as it stands. Timeshift is bought back by writing it
        // to disk on the way past and playing the file — see `tune`, and
        // `timeshift.rs` for why FFmpeg's own `cache:` protocol does not do
        // the job.
        QualityChoice::Original => {
            format!("{server}/devices/ANY/channels/{channel}/stream.mpg")
        }
        QualityChoice::Height(h) => format!(
            "{server}/devices/ANY/channels/{channel}/hls/master.m3u8\
             ?vcodec=h264&acodec=copy&resolution={h}&bitrate={}",
            quality.kbps()
        ),
    }
}

/// Fetch the home screen's data.
fn spawn_home(
    runtime: &tokio::runtime::Runtime,
    tx: &Sender<Msg>,
    ctx: egui::Context,
    server: &str,
) {
    if server.is_empty() {
        return;
    }
    let lib = library::Library::new(server);
    let tx = tx.clone();
    runtime.spawn(async move {
        let message = match lib.home().await {
            Ok(home) => Msg::Home(home),
            Err(e) => Msg::Failed(format!("Could not read the library: {e:#}")),
        };
        let _ = tx.send(message);
        ctx.request_repaint();
    });
}

/// Fetch listings, collections, sources and the current schedule.
fn spawn_guide(
    runtime: &tokio::runtime::Runtime,
    tx: &Sender<Msg>,
    ctx: egui::Context,
    server: &str,
) {
    if server.is_empty() {
        return;
    }
    let api = guide::GuideApi::new(server);
    let tx = tx.clone();
    runtime.spawn(async move {
        let message = match api.load(now_unix(), GUIDE_HOURS).await {
            Ok(data) => Msg::Guide(Box::new(data)),
            Err(e) => Msg::Failed(format!("Could not read the guide: {e:#}")),
        };
        let _ = tx.send(message);
        ctx.request_repaint();
    });
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Ask for the integrated GPU.
///
/// A laptop with switchable graphics will hand an application the discrete GPU
/// by default, which for a video player is a straight trade of battery life for
/// nothing: drawing one video frame and some flat panels does not need it.
///
/// Set before the graphics context exists, because the adapter is chosen during
/// initialization and cannot be changed afterwards. The two vendor symbols are
/// the opposite lever, exported by applications that want the discrete GPU;
/// they are deliberately left unset.
fn prefer_integrated_gpu() {
    if std::env::var_os("WGPU_POWER_PREF").is_none() {
        std::env::set_var("WGPU_POWER_PREF", "low");
    }
}

fn main() -> eframe::Result<()> {
    install_panic_log();
    // Buffers left by a process that did not live to clean up after itself.
    // Before anything can make a request, so every one of them carries the
    // device name from the first.
    let saved = settings::Settings::load();

    // Before anything writes, reads or logs.
    //
    // `paths::data_dir` remembers its answer the first time it is asked, and
    // the logger and the crash handler ask early by design, because they have
    // to work before anything else does. So the redirection has to happen
    // here, above the first line that could touch any of it. A configured
    // folder that cannot be created is reported and ignored rather than
    // silently leaving the caches somewhere the setting says they are not.
    if let Some(dir) = saved.cache_path() {
        if !paths::set_data_dir(dir.clone()) {
            log::logline!("[clicker] cannot use {} for caches; using the default", dir.display());
        }
    }

    // Swept from wherever the buffer is configured to live, which is not
    // necessarily where it lived last time somebody changed the setting.
    timeshift::sweep(&saved.buffer_path());
    settings::set_user_agent(&saved.client_name);
    prefer_integrated_gpu();

    // The build's own license, from the binary rather than from a claim in a
    // text file. This application may only be distributed if FFmpeg was built
    // without GPL components, so it is worth being able to check.
    log::logline!("[clicker] {}", mpv::Player::backend());

    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 760.0])
        .with_min_inner_size([420.0, 280.0])
        .with_title(APP_NAME)
        // Opaque, whatever the platform does about the frame.
        //
        // The window used to be transparent, because Mica is drawn by the
        // desktop compositor *behind* the window and only shows through one
        // that lets it. That is what made this Windows 11 only. The material
        // is painted here now (see `backdrop`), which needs a surface to paint
        // on rather than a hole to look through.
        .with_transparent(false);
    // Whether the frame is the system's or ours is the platform's call: no
    // caption and self-drawn everything on Windows and Linux, native traffic
    // lights floating over the same surface on macOS.
    let mut viewport = platform::shape_window(viewport);
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(icon);
    }

    // Reopen where it was left, at the size it was left, maximized if it was
    // maximized. The position is only honored while it still lands on a
    // desktop that exists — see `Window::is_reachable`.
    if let Some(window) = saved.window.filter(settings::Window::is_reachable) {
        viewport = viewport.with_inner_size([window.width, window.height]);
        viewport = viewport.with_position([window.x, window.y]);
        if window.maximized {
            viewport = viewport.with_maximized(true);
        }
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(App::new(cc)))
        }),
    )
}

/// Record panics to a file as well as to stderr.
///
/// A panic on the UI thread takes the whole process with it, and a windowed
/// build has no console attached — so the one piece of information that says
/// where it happened was being written to a handle nobody was reading. The
/// release profile strips symbols, so a backtrace would be addresses, but the
/// panic *location* is compiled in regardless and it is the part that names the
/// bug.
///
/// The log is appended to, so a fault that only shows up occasionally still
/// leaves a trail rather than overwriting the evidence of the last one.
fn install_panic_log() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let where_ = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "an unknown location".to_string());
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panicked".to_string());
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("unnamed").to_string();
        let line = format!("panic on the {thread} thread at {where_}: {what}\n");

        eprint!("[clicker] {line}");
        // Into the player log as well as the crash log. A panic is the last
        // thing that happened before the numbers above it stopped, and reading
        // them together is what makes either of them mean anything.
        log::line(&format!("[panic] {}", line.trim_end()));
        if let Some(path) = crash_log_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                use std::io::Write;
                let _ = write!(file, "{line}");
            }
        }
        previous(info);
    }));
}

fn crash_log_path() -> Option<std::path::PathBuf> {
    Some(paths::data_dir()?.join("crash.log"))
}

/// Something destructive, waiting to be confirmed.
///
/// One dialog for all three, because they are the same question, but each
/// carries what it is about: "delete this?" means something different for a
/// recording on the server and a copy on this disk, and a dialog that cannot
/// tell them apart is one that gets clicked through.
#[derive(Clone)]
enum Confirm {
    /// Delete the recording from the DVR.
    Recording(library::Recording),
    /// Delete the local copy of one download.
    Download { id: String, title: String },
    /// Delete the local copies of everything that has finished.
    Finished(usize),
}

/// The icon the running window carries.
///
/// This is a separate thing from the Win32 resource `build.rs` compiles in.
/// That one is what Explorer and the shell read off the file; the window
/// itself takes its small and large icons from here, and with nothing supplied
/// the title bar, the taskbar button and Alt+Tab all fall back to a blank
/// default the whole time the program is running — which is exactly what
/// "the icon isn't embedded" looked like, even though it was.
///
/// The PNG rather than the .ico because the .ico is a container of several
/// sizes and `image` would only hand back one of them anyway.
fn app_icon() -> Option<egui::IconData> {
    const PNG: &[u8] = include_bytes!("../assets/clicker.png");
    let image = image::load_from_memory(PNG).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

/// Work that happened off the UI thread and needs folding back in.
enum Msg {
    /// What is on this channel, the server's padding, and whether the DVR
    /// already has a job for it.
    Program(api::Airing, api::Padding, Option<String>),
    JobCreated(String),
    JobDeleted,
    Failed(String),
    /// The periodic health check answered: whether the DVR is reachable, and
    /// why not when it is not.
    ServerHealth(bool, String),
    /// The home screen's data, assembled from the recordings and series lists.
    Home(library::Home),
    /// The guide, with its listings and the current schedule.
    Guide(Box<guide::GuideData>),
    /// A player finished opening off the UI thread. The generation says which
    /// request it answers; a stale one is dropped, which also stops its
    /// threads and releases its stream.
    PlayerOpened {
        result: Result<Arc<mpv::Player>, String>,
        resume_at: f64,
        generation: u64,
    },
    /// A candidate DVR answered, or did not.
    Probed(String, Result<settings::ServerInfo, String>),
    /// A season pass was created or removed; the guide needs refreshing.
    ScheduleChanged(String),
    /// A recording was deleted or its watched state changed; the library and
    /// home screen need rereading.
    LibraryChanged(String),
    /// A folder was chosen in the system picker.
    FolderPicked(ui_setup::Folder, std::path::PathBuf),
}

struct App {
    /// Behind an `Arc` because the paint callback that draws the picture runs
    /// later, inside egui's renderer, and has to still have a player then.
    player: Option<Arc<mpv::Player>>,

    paused: bool,
    volume: f32,
    show_stats: bool,

    // Scrubbing is held locally while the pointer is down so the bar tracks the
    // finger, and the seek only happens on release. Seeking on every pointer
    // move would flush the pipeline dozens of times a second.
    scrubbing: bool,
    scrub_target: Option<f64>,

    /// When the pointer last did anything. The controls are an overlay on the
    /// picture, so they have to get out of the way when they are not being
    /// used, the way every other video player does it.
    last_activity: Instant,

    // Recording state lives on the server. `job_id` is the DVR's id for the
    // scheduled job, so an empty value genuinely means "not recording" rather
    // than "this session has not pressed the button yet".
    dvr: api::Dvr,
    runtime: tokio::runtime::Runtime,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    airing: Option<api::Airing>,
    padding: api::Padding,
    job_id: Option<String>,
    job_pending: bool,
    toast: Option<(String, Instant)>,

    frame_times: Vec<f32>,
    last_frame: Instant,
    last_decoded: u64,
    last_fps_sample: Instant,
    decode_fps: f32,

    // ── Navigation and screens ──────────────────────────────────────────
    screen: Screen,
    rail_expanded: bool,
    /// Full screen hides the caption and the rail entirely: the picture is the
    /// whole point of full screen, so nothing else should be on it.
    fullscreen: bool,
    /// Whether the window was maximized before full screen, so leaving it
    /// returns to what was there rather than to a restored-size window.
    was_maximized: bool,
    /// Set when the player is showing rather than a browsing screen. Live TV
    /// and a recording are both this; what differs is the source.
    watching: bool,

    lib: library::Library,
    images: images::Images,
    downloads: downloads::Downloads,
    home: library::Home,
    home_loading: bool,

    settings: settings::Settings,
    setup: ui_setup::SetupState,
    guide: guide::GuideData,
    guide_state: ui_guide::GuideState,
    guide_loading: bool,
    library_state: ui_library::LibraryState,
    /// Actions the menu bar asked for, waiting for `handle_keys` to act on
    /// them exactly as it would the keys of the same name.
    menu_actions: std::collections::HashSet<String>,
    recordings_tab: ui_library::RecordingsTab,
    /// For the crossfade between screens: which one is on show, and when it
    /// arrived.
    shown_screen: Screen,
    screen_changed: Instant,
    /// Open while choosing padding and pass options for a program.
    record_dialog: Option<ui_record::RecordDialog>,
    /// A recording awaiting delete confirmation. Deleting is the one action in
    /// this application that cannot be undone, so it asks.
    confirm_delete: Option<Confirm>,
    /// What the spinner should say. Live TV really is tuning something;
    /// a recording is a file and is not.
    loading: Loading,
    /// The channel currently being watched, if this is live rather than a
    /// recording.
    live_channel: Option<String>,
    /// Commercial breaks in the playing recording, as (start, end) seconds.
    /// The DVR's comskip pass found these; they arrive with the recording and
    /// cost nothing to use. Empty for live TV, which has no markers until the
    /// recording has been processed.
    commercials: Vec<(f64, f64)>,
    /// A quality chosen from the player for this session. None means the
    /// global default from Settings. Per-session on purpose: dropping to 540p
    /// on hotel wifi tonight should not quietly degrade the living room
    /// forever.
    quality_override: Option<QualityChoice>,
    /// The in-player quality menu, open above the transport.
    show_quality: bool,
    /// The recording being streamed, kept so a quality change can reopen the
    /// same one at the same position.
    current_recording: Option<library::Recording>,
    /// Playing a downloaded local file. Quality does not apply there: the file
    /// is the file, and downloads always fetch the original.
    playing_local: bool,
    /// How the current source is being carried, for the stats card. Known here
    /// rather than asked of the player: mpv opens the address and does not
    /// report back which of these it decided the address was.
    transport: Option<stream::Transport>,
    /// Which open request is current. Tuning channel B while A is still
    /// opening must not end with A's picture arriving late and winning.
    open_generation: u64,

    /// The live buffer behind direct playback, when there is one. Dropping it
    /// stops its writer and deletes the file, so it is held exactly as long as
    /// something is playing from it.
    timeshift: Option<timeshift::Timeshift>,
    /// The notification-area icon, present only while the setting is on.
    tray: Option<tray::Tray>,
    /// This window, so the tray thread can bring it back without going through
    /// egui — which cannot help, because a hidden window draws no frames.
    hwnd: Option<isize>,

    // ── Server health ───────────────────────────────────────────────────
    //
    // The player had stall detection, but only while something was playing.
    // Left sitting on the home screen while the laptop slept, nothing ever
    // noticed the DVR had gone: the guide's half-hourly refresh reset its own
    // timer before making the request, so one failure meant half an hour of
    // silence, and no screen said anything was wrong. Relaunching was the only
    // way back, which is not a state an application should be able to reach.
    /// Whether the DVR answered the last health check.
    online: bool,
    /// A health check is in flight; do not start another.
    probing_server: bool,
    /// When the next health check is due.
    next_health_check: Instant,
    /// Consecutive failures, for backoff.
    health_failures: u32,
    /// Wall-clock seconds at the last housekeeping pass. Wall clock, not
    /// `Instant`: `Instant` is `QueryPerformanceCounter`, which is not
    /// guaranteed to advance while the machine is asleep — and sleeping is
    /// exactly the case this exists to notice.
    last_tick_unix: i64,
    /// When the first frame of the current source arrived, for the entrance
    /// animation.
    first_frame_at: Option<Instant>,
    /// When the guide was last loaded. Listings go stale as the evening moves
    /// on, and a guide showing what was on an hour ago is worse than useless.
    guide_loaded_at: Instant,
    /// Set when playback has stalled long enough to look like a dropped
    /// connection — a closed laptop lid, or wifi going away.
    stalled_since: Option<Instant>,
    /// Frame count at the last stall check, to tell "stopped" from "slow".
    stall_watch_frames: u64,
    /// When the position was last reported to the server.
    position_reported_at: Instant,
    /// Kept so a player opened later can still ask for repaints. The context is
    /// cheap to clone and is the only handle the decoder threads need.
    repaint: egui::Context,
    /// The Mica-alike the window is painted on.
    backdrop: backdrop::Backdrop,
    /// Which card the home screen's shuffle is showing.
    ///
    /// Bumped on every arrival at Home rather than once per launch, so leaving
    /// the screen and coming back deals another one. It holds still while the
    /// screen is up: a hero that changed under a click would be a trap rather
    /// than a surprise.
    hero_pick: u64,
    /// What was on screen last frame, for noticing arrivals at home. None
    /// while something is playing, because the player is not a screen.
    last_view: Option<Screen>,
    /// The window's geometry as last written to settings, and when it last
    /// moved. Dragging a window produces a position every frame, and writing
    /// the settings file at sixty hertz for the length of a drag is not
    /// something to do to someone's disk — so it is written once the window
    /// has been still for a moment. See `remember_window`.
    window_settled: Option<Instant>,

    error: Option<String>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(handle) = platform::window_handle(cc) {
            platform::apply_chrome(handle, true);
        }

        // Where a platform gates the local network behind a question, have it
        // asked now, while the first screen is still being read — not after
        // the first attempt to reach the DVR has already failed on it.
        platform::request_local_network();

        // The menu bar, where the platform keeps one above the window rather
        // than inside it. Here rather than before the window exists: it is
        // handed to the running application object, which by now there is.
        platform::install_menu_bar();

        // Maximize by command rather than by `ViewportBuilder::with_maximized`.
        //
        // The builder is told to maximize *and* given the restored position and
        // size, and on Windows the position wins: the window came back the
        // right size, in the right place, and not maximized. The size and
        // position still have to be set — they are where un-maximizing returns
        // to — so the maximize is asked for separately, here, where nothing
        // undoes it. Queued before the first frame, so there is no flash of an
        // unmaximized window.
        if settings::Settings::load().window.is_some_and(|w| w.maximized) {
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }

        // Four workers rather than two: artwork downloads and decodes run here
        // alongside the API calls, and a home screen asks for a dozen images at
        // once. Two threads made the first paint visibly crawl in.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("could not start the async runtime");
        let runtime_handle = runtime.handle().clone();

        let (tx, rx) = std::sync::mpsc::channel();
        let settings = settings::Settings::load();
        // The menu bar's accelerators, from the bindings actually in force
        // rather than the empty ones it was built with a moment ago.
        platform::sync_menu_shortcuts(&settings);
        let server = settings.server_url();
        let dvr = api::Dvr::new(&server);
        let lib = library::Library::new(&server);

        // Nothing is tuned at startup, deliberately. Opening a tuner before
        // being asked to watch anything holds a scarce resource — on a
        // two-tuner box it is half of them — for someone who may only have
        // opened the app to check what is recording tonight. The home screen
        // needs no tuner at all.
        let configured = settings.configured();
        if configured {
            spawn_home(&runtime, &tx, cc.egui_ctx.clone(), &server);
            spawn_guide(&runtime, &tx, cc.egui_ctx.clone(), &server);
        }

        Self {
            player: None,
            paused: false,
            volume: 1.0,
            show_stats: false,
            scrubbing: false,
            scrub_target: None,
            last_activity: Instant::now(),
            dvr,
            runtime,
            tx,
            rx,
            airing: None,
            padding: api::Padding::default(),
            job_id: None,
            job_pending: false,
            toast: None,
            frame_times: Vec::with_capacity(180),
            last_frame: Instant::now(),
            last_decoded: 0,
            last_fps_sample: Instant::now(),
            decode_fps: 0.0,
            screen: Screen::Home,
            rail_expanded: false,
            fullscreen: false,
            was_maximized: false,
            watching: false,
            backdrop: backdrop::Backdrop::new(&cc.egui_ctx),
            // Seeded from the clock so the first card of a session is not the
            // same one every time the program opens.
            hero_pick: now_unix() as u64,
            last_view: Some(Screen::Home),
            window_settled: None,
            images: images::Images::new(runtime_handle.clone()),
            downloads: downloads::Downloads::new(runtime_handle, settings.download_path()),
            lib,
            // Last session's library, so downloaded recordings have a title
            // and a poster from the first frame, server or no server. It is
            // replaced the moment a live load arrives.
            home: library::Home::load_cache().unwrap_or_default(),
            home_loading: configured,
            setup: ui_setup::SetupState::default(),
            guide: guide::GuideData::default(),
            // The guide reopens exactly as it was left: picking a collection
            // IS picking the default, with no separate setting to configure.
            guide_state: ui_guide::GuideState {
                collection: settings.last_collection.clone(),
                source: settings.last_source.clone(),
                ..ui_guide::GuideState::default()
            },
            settings,
            guide_loading: configured,
            library_state: ui_library::LibraryState::default(),
            menu_actions: std::collections::HashSet::new(),
            recordings_tab: ui_library::RecordingsTab::default(),
            shown_screen: Screen::Home,
            screen_changed: Instant::now(),
            record_dialog: None,
            confirm_delete: None,
            loading: Loading::recording(""),
            live_channel: None,
            commercials: Vec::new(),
            quality_override: None,
            show_quality: false,
            current_recording: None,
            playing_local: false,
            transport: None,
            open_generation: 0,
            timeshift: None,
            tray: None,
            hwnd: platform::window_handle(cc),
            online: true,
            probing_server: false,
            next_health_check: Instant::now() + Duration::from_secs(30),
            health_failures: 0,
            last_tick_unix: now_unix(),
            first_frame_at: None,
            guide_loaded_at: Instant::now(),
            stalled_since: None,
            stall_watch_frames: 0,
            position_reported_at: Instant::now(),
            repaint: cc.egui_ctx.clone(),
            error: None,
        }
    }

    /// Reload everything that depends on which server is selected.
    /// Re-fetch everything the screens are built from, without disturbing
    /// playback or which screen is showing.
    ///
    /// Distinct from `reconnect`, which is for changing server and throws the
    /// player away with the data. After an outage the server is the same one
    /// and whatever is playing may well still be playing; only the listings
    /// are stale.
    fn refresh_data(&mut self) {
        if !self.settings.configured() {
            return;
        }
        let server = self.settings.server_url();
        self.home_loading = true;
        self.guide_loading = true;
        self.guide_loaded_at = Instant::now();
        spawn_home(&self.runtime, &self.tx, self.repaint.clone(), &server);
        spawn_guide(&self.runtime, &self.tx, self.repaint.clone(), &server);
    }

    /// Whatever the menu bar was asked for, on the platforms that have one.
    ///
    /// The system's own items — Quit, Hide, Full Screen, the clipboard verbs
    /// — never arrive here; macOS acts on those itself. What does arrive is
    /// an action id from `keys::ACTIONS`, which is put in a set for
    /// `handle_keys` to read as though the key had been pressed. That is the
    /// whole point of using the same ids: one implementation of "skip
    /// forward", reachable two ways, with no chance of the menu and the
    /// keyboard drifting apart.
    ///
    /// The two exceptions are things no key has: refreshing, and the link in
    /// the Help menu.
    fn pump_menu(&mut self) {
        while let Some(id) = platform::menu_command() {
            match id.as_str() {
                "refresh" => self.refresh_data(),
                "github" => platform::open_url("https://github.com/mackid1993/Clicker"),
                _ => {
                    self.menu_actions.insert(id);
                }
            }
        }
    }

    fn reconnect(&mut self) {
        let server = self.settings.server_url();
        self.dvr = api::Dvr::new(&server);
        self.lib = library::Library::new(&server);
        self.home = library::Home::default();
        self.guide = guide::GuideData::default();
        self.home_loading = true;
        self.guide_loading = true;

        // Whatever was playing came from the old server.
        self.retire_texture();
        self.player = None;
        self.watching = false;
        self.live_channel = None;

        spawn_home(&self.runtime, &self.tx, self.repaint.clone(), &server);
        spawn_guide(&self.runtime, &self.tx, self.repaint.clone(), &server);
    }

    fn drain_messages(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                Msg::Program(airing, padding, existing) => {
                    self.padding = padding;
                    self.job_id = existing;
                    self.airing = Some(airing);
                }
                Msg::JobCreated(id) => {
                    self.job_pending = false;
                    let name = self
                        .airing
                        .as_ref()
                        .map(|a| a.title.clone())
                        .unwrap_or_else(|| "this program".into());
                    self.job_id = Some(id);
                    self.announce(format!("Recording {name}"));
                }
                Msg::JobDeleted => {
                    self.job_pending = false;
                    self.job_id = None;
                    self.announce("Recording canceled".into());
                }
                Msg::Failed(reason) => {
                    self.job_pending = false;
                    self.home_loading = false;
                    self.announce(reason);
                }
                Msg::ServerHealth(reachable, why) => {
                    self.probing_server = false;
                    if reachable {
                        // Coming back is the interesting half. Everything on
                        // screen was fetched before the server went away, so
                        // it is however stale the outage was long.
                        if !self.online {
                            self.announce("Reconnected to the DVR".into());
                            self.refresh_data();
                        }
                        self.online = true;
                        self.health_failures = 0;
                        self.next_health_check = Instant::now() + Duration::from_secs(30);
                    } else {
                        if self.online {
                            self.announce(format!("Lost the DVR: {why}"));
                        }
                        self.online = false;
                        self.health_failures = self.health_failures.saturating_add(1);
                        // Back off, but not far. A DVR that went away because
                        // the laptop moved rooms is usually back within
                        // seconds, and a half-minute ceiling means the wait is
                        // never long enough to be worth relaunching over.
                        let wait = match self.health_failures {
                            1 => 3,
                            2 => 6,
                            3 => 12,
                            _ => 30,
                        };
                        self.next_health_check = Instant::now() + Duration::from_secs(wait);
                    }
                }
                Msg::Home(home) => {
                    // Kept on disk so the next launch has titles and artwork
                    // for downloaded recordings even with no server to ask.
                    home.save_cache();
                    self.home = home;
                    self.home_loading = false;
                }
                Msg::Guide(data) => {
                    self.guide = *data;
                    self.guide_loading = false;
                    self.guide_loaded_at = Instant::now();
                }
                Msg::PlayerOpened {
                    result,
                    resume_at,
                    generation,
                } => {
                    // A stale open: the user has already tuned something else.
                    // Dropping the player stops its threads and its stream.
                    if generation != self.open_generation {
                        continue;
                    }
                    match result {
                        Ok(player) => {
                            // `resume_at` is elapsed seconds from the start of
                            // the item; the player seeks in stream time, whose
                            // origin is whatever the file begins at. Adding
                            // the origin is what turns one into the other —
                            // without it, a resume asks for a position tens of
                            // thousands of seconds past the end.
                            if resume_at > 5.0 {
                                let origin = player
                                    .seek_range()
                                    .map(|(start, _)| start)
                                    .unwrap_or(0.0);
                                player.seek_to(origin + resume_at);
                            }

                            self.player = Some(player);
                            self.error = None;
                            self.paused = false;
                            self.first_frame_at = None;
                        }
                        Err(e) => {
                            self.watching = false;
                            self.announce(format!("Could not play: {e}"));
                        }
                    }
                }
                Msg::FolderPicked(which, path) => {
                    let text = path.display().to_string();
                    // Checked the same way a typed path is, because a folder
                    // that exists and was chosen from a dialog can still be
                    // one this account cannot write to.
                    let refused = settings::writable(&path).err().map(|e| format!("{e:#}"));
                    match which {
                        ui_setup::Folder::Downloads => {
                            self.settings.download_dir = text;
                            self.setup.download_dir_error = refused;
                        }
                        ui_setup::Folder::Buffer => {
                            self.settings.buffer_dir = text;
                            self.setup.buffer_dir_error = refused;
                        }
                        ui_setup::Folder::Cache => {
                            self.settings.cache_dir = text;
                            self.setup.cache_dir_error = refused;
                        }
                    }
                    self.handle_setup(ui_setup::SetupAction::Save);
                }
                Msg::LibraryChanged(note) => {
                    self.announce(note);
                    // Reload rather than patching the local copy, so what is
                    // on screen is what the server actually has.
                    let server = self.settings.server_url();
                    spawn_home(&self.runtime, &self.tx, self.repaint.clone(), &server);
                }
                Msg::ScheduleChanged(note) => {
                    self.announce(note);
                    // Reload so the guide's dots match the server rather than
                    // an optimistic guess made here.
                    let server = self.settings.server_url();
                    spawn_guide(&self.runtime, &self.tx, self.repaint.clone(), &server);
                }
                Msg::Probed(url, result) => {
                    self.setup.probing = false;
                    match result {
                        Ok(info) => {
                            self.setup.message =
                                Some((format!("Connected to {}", info.name), true));
                            self.setup.address.clear();
                            self.settings
                                .add_server(ui_setup::server_from_probe(url, info));
                            if let Err(e) = self.settings.save() {
                                self.announce(format!("Could not save settings: {e:#}"));
                            }
                            self.reconnect();
                            self.screen = Screen::Home;
                        }
                        Err(e) => {
                            // The platform gets a word in: on macOS the first
                            // probe usually loses a race with the local
                            // network permission prompt, and the failure
                            // should say so rather than read as a bad address.
                            let hint = platform::LOCAL_NETWORK_HINT;
                            self.setup.message = Some((format!("{e}{hint}"), false));
                        }
                    }
                }
            }
        }
    }

    fn announce(&mut self, text: String) {
        self.toast = Some((text, Instant::now()));
    }

    /// Ask the DVR to schedule this program, or to drop the job it already
    /// has. Recording is the server's job, so the button reports what the
    /// server said rather than lighting up optimistically.
    fn toggle_record(&mut self, ctx: &egui::Context) {
        if self.job_pending {
            return;
        }

        let dvr = self.dvr.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        match self.job_id.clone() {
            Some(id) => {
                self.job_pending = true;
                self.runtime.spawn(async move {
                    let message = match dvr.delete_job(&id).await {
                        Ok(()) => Msg::JobDeleted,
                        Err(e) => Msg::Failed(format!("Could not cancel: {e:#}")),
                    };
                    let _ = tx.send(message);
                    ctx.request_repaint();
                });
            }
            None => {
                let Some(airing) = self.airing.clone() else {
                    self.announce("Still loading the guide for this channel".into());
                    return;
                };
                self.job_pending = true;
                let padding = self.padding;
                self.runtime.spawn(async move {
                    let message = match dvr.create_job(&airing, padding).await {
                        Ok(id) => Msg::JobCreated(id),
                        Err(e) => Msg::Failed(format!("Could not record: {e:#}")),
                    };
                    let _ = tx.send(message);
                    ctx.request_repaint();
                });
            }
        }
    }

    /// Stop showing the video texture without freeing it yet.
    ///
    /// Every caller of this runs part-way through a frame — tuning a channel,
    /// switching quality, stopping playback, changing server — and the picture
    /// has already been painted by the time they do. Freeing the texture there
    /// leaves the frame referring to something that no longer exists.
    /// Let go of the picture, and of the OpenGL objects behind it.
    ///
    /// mpv's renderer owns shaders and textures and frees them itself, but only
    /// lawfully while the context is current — which, between frames, it is
    /// not. So this happens here, at the points where a source is being
    /// replaced, rather than in a `Drop` that could run anywhere.
    ///
    /// Call this *before* dropping the player, not after. Every one of these
    /// call sites originally cleared `self.player` first, which left nothing to
    /// release and leaked mpv's whole renderer on every channel change.
    fn retire_texture(&mut self) {
        if let (Some(player), Some(gl)) = (self.player.as_ref(), gl_fns()) {
            unsafe { player.release_gl(gl) };
        }
    }

    /// Put the picture on screen.
    ///
    /// mpv draws it, inside a paint callback, because that is the only moment
    /// the OpenGL context is current on this thread. Nothing is uploaded and
    /// nothing is copied: the frame is decoded, converted and composited on the
    /// graphics chip and never crosses back into system memory. The old path
    /// read every frame back, converted it on the processor, allocated eight
    /// megabytes of `Color32` for it and uploaded that again — three crossings
    /// of the bus for a picture that was already on the right side of it.
    fn draw_video(
        &self,
        ui: &egui::Ui,
        ctx: &egui::Context,
        rect: egui::Rect,
        entrance: f32,
    ) {
        let Some(player) = &self.player else { return };
        let (vw, vh) = player.video_size();
        if vw == 0 || vh == 0 {
            return;
        }

        // Where the picture goes, letterboxed inside the content area, with the
        // entrance rise applied.
        let size = egui::vec2(vw as f32, vh as f32);
        let scale = (rect.width() / size.x).min(rect.height() / size.y);
        let rise = (1.0 - entrance) * 42.0;
        let target =
            egui::Rect::from_center_size(rect.center() + egui::vec2(0.0, rise), size * scale);
        let target = target.intersect(rect);
        if !target.is_positive() {
            return;
        }

        // Weak, emphatically not a clone of the `Arc`.
        //
        // This callback runs after `update` has returned, during egui's paint.
        // The transport — including its back arrow — is drawn over the picture,
        // so stopping playback happens *after* this callback is already queued:
        // teardown frees mpv's renderer and drops the player, and then the
        // renderer runs this. Holding a strong reference kept the player alive
        // to that moment and ran its `Drop` from inside egui, destroying an mpv
        // handle whose render context had just been freed under it. A weak one
        // simply finds nothing and skips the frame, which is the truth: there is
        // no longer anything to draw.
        let player = Arc::downgrade(player);
        let callback = egui::PaintCallback {
            rect: target,
            callback: std::sync::Arc::new(eframe::egui_glow::CallbackFn::new(
                move |info, _painter| {
                    let Some(player) = player.upgrade() else { return };
                    // egui's own conversion of this callback's rect into
                    // physical pixels, rather than one computed up in the
                    // interface from points and a scale factor. It is the
                    // renderer's view of where the picture goes, which is the
                    // only one that has to be right.
                    let at = info.viewport_in_pixels();
                    if let Some(gl) = gl_fns() {
                        unsafe {
                            player.present(
                                gl,
                                [at.left_px, at.from_bottom_px, at.width_px, at.height_px],
                            )
                        };
                    }
                },
            )),
        };
        ui.painter().with_clip_rect(rect).add(callback);

        // Keep asking for frames while something is playing.
        //
        // mpv's ready-callback alone is not enough to hold a steady rate: it
        // wakes the event loop, which then has to reach a paint, and a wake
        // that lands just after the compositor's deadline waits out the whole
        // refresh and the frame it was carrying is dropped. Asking for the
        // next frame now means the interface is already at the vsync boundary
        // when it arrives. Vsync caps this, so it is the display's rate and
        // not a spin.
        if !self.paused {
            ctx.request_repaint();
        }

        // The entrance fade, painted over the picture rather than tinted into
        // it: a blit cannot blend, and this is one rectangle for a third of a
        // second.
        if entrance < 1.0 {
            let veil = ((1.0 - entrance) * 255.0) as u8;
            ui.painter()
                .with_clip_rect(rect)
                .rect_filled(target, 0.0, egui::Color32::from_black_alpha(veil));
        }
    }


    fn sample_rates(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;
        if dt > 0.0 && dt < 1.0 {
            self.frame_times.push(dt);
            if self.frame_times.len() > 180 {
                self.frame_times.remove(0);
            }
        }

        // Sampled over a longer window than feels necessary, because a short
        // one quantizes a 59.94fps source into a readout that jitters between
        // 59 and 62 and looks like a fault when nothing is wrong.
        let since = now.duration_since(self.last_fps_sample).as_secs_f32();
        if since >= 2.0 {
            if let Some(player) = &self.player {
                let count = player.decoded();
                self.decode_fps = count.saturating_sub(self.last_decoded) as f32 / since;
                self.last_decoded = count;

                // A decode error arrives asynchronously, so it is picked up
                // here rather than at open time.
                if self.error.is_none() {
                    self.error = player.error();
                }
            }
            self.last_fps_sample = now;
        }
    }

    fn ui_fps(&self) -> f32 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        let mean: f32 = self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32;
        if mean > 0.0 { 1.0 / mean } else { 0.0 }
    }
}

impl eframe::App for App {
    /// The base the backdrop is painted over. Only ever visible for the first
    /// frame, before there is a texture to draw, so it is the same solid the
    /// material is mixed down to.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        Fluent::SOLID.to_normalized_gamma_f32()
    }

    /// Take the player down while there is still a graphics context to take it
    /// down with.
    ///
    /// mpv requires its renderer be freed before the handle that owns it, and
    /// freeing the renderer touches OpenGL. Left to `Drop`, neither condition
    /// holds: the window and its context are gone by then, and closing the
    /// application mid-playback crashed on the way out. eframe calls this with
    /// the context still current, which is the one moment both are true.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.retire_texture();
        self.player = None;
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump_tray(ctx);
        self.pump_menu();
        self.drain_messages();

        // Bring the renderer up, and only then let mpv open the file.
        //
        // Here rather than where the player is created, because this is the
        // thread that holds the OpenGL context. mpv initializes its video
        // output while loading, and a load that happens before the renderer
        // exists gets video switched off for the rest of the file — audio
        // playing over a spinner, with one line in the log to say so.
        //
        // Every frame, not once: it is cheap after the first, and a renderer
        // that failed to come up gets another attempt rather than a player
        // that is permanently mute about it.
        if let (Some(player), Some(gl)) = (self.player.as_ref(), gl_fns()) {
            unsafe { player.start(gl) };
        }

        self.sample_rates();
        self.images.pump(ctx);
        self.handle_keys(ctx);
        self.housekeeping();
        self.remember_window(ctx);

        // A new card every time the home screen is arrived at.
        //
        // "Arrived at" has to include coming back from something playing, not
        // just from another screen. Watching does not change `screen` — the
        // player draws over whatever was showing — so a check on the screen
        // alone missed the most common way of leaving and returning, which is
        // to watch something and stop.
        //
        // Noticed here rather than at each of the several places that set the
        // screen or start playback, so a new way of getting home cannot forget
        // to deal one.
        let view = (!self.watching).then_some(self.screen);
        if view == Some(Screen::Home) && self.last_view != Some(Screen::Home) {
            self.hero_pick = self.hero_pick.wrapping_add(1);
        }
        self.last_view = view;

        // The material, before anything else is drawn over it. Every surface
        // in the theme is translucent by design and needs something behind it;
        // this is that something, and it used to come from the compositor.
        self.backdrop.paint(ctx, ctx.screen_rect());

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Fluent::LAYER_BASE))
            .show(ctx, |ui| {
                let full = ctx.screen_rect();

                // Full screen means full screen: no caption, no rail, nothing
                // but the picture and the controls that fade away on their own.
                let chrome = !self.fullscreen;
                let caption_h = if chrome { TITLEBAR_HEIGHT } else { 0.0 };
                // The rail slides away while something is playing. Watching is
                // one thing at a time and a column of somewhere-else is just a
                // strip of the picture spent on navigation nobody is using.
                //
                // It used to stay up, because hiding it the moment something
                // was clicked in the guide left a blank window with a spinner
                // and no visible way back — the picture had not arrived and
                // everything pressable had gone. That objection is answered
                // now: the transport is drawn over the tuning indicator as
                // well as over the picture, so its back arrow is there from
                // the first frame of the wait.
                //
                // The width animates between its sizes, which is what makes
                // the hamburger feel like it slides a surface open rather than
                // teleporting the whole layout — and what makes this look like
                // the picture opening out rather than the rail vanishing.
                let rail_target = if chrome && self.settings.configured() && !self.watching {
                    if self.rail_expanded { RAIL_EXPANDED } else { RAIL_COLLAPSED }
                } else {
                    0.0
                };
                let rail_w = ctx.animate_value_with_time(
                    egui::Id::new("rail-width"),
                    rail_target,
                    theme::ANIM_SURFACE,
                );

                if chrome {
                    title_bar(
                        ui,
                        ctx,
                        egui::Rect::from_min_size(full.min, egui::vec2(full.width(), caption_h)),
                        self.online,
                    );
                }

                if rail_w > 0.0 {
                    let rail = egui::Rect::from_min_size(
                        egui::pos2(full.min.x, full.min.y + caption_h),
                        egui::vec2(rail_w, full.height() - caption_h),
                    );
                    let mut screen = self.screen;
                    if ui::nav_rail(ui, rail, &mut screen, &mut self.rail_expanded) {
                        self.screen = screen;
                        // Navigating away ends playback, same as Escape. The
                        // alternative — sound continuing under a screen with
                        // no picture — reads as a bug every time.
                        if self.watching {
                            self.stop_playback();
                        }
                    }
                }

                let content = egui::Rect::from_min_max(
                    egui::pos2(full.min.x + rail_w, full.min.y + caption_h),
                    full.max,
                );

                // Nothing works without a server, so nothing else is offered
                // until there is one. Showing a navigation rail over five
                // empty screens would only invite someone to explore a broken
                // application.
                if !self.settings.configured() {
                    let action = ui_setup::onboarding(
                        ui,
                        content,
                        &mut self.settings,
                        &mut self.setup,
                    );
                    self.handle_setup(action);
                } else if self.watching {
                    self.watch_view(ui, ctx, full, content);
                } else {
                    self.browse_view(ui, content);
                    self.offline_banner(ui, content);
                }

                self.toast_banner(ui, content);

                // Last, so the borders sit above everything they overlap —
                // notably the caption, whose drag-to-move would otherwise
                // swallow the top edge.
                resize_borders(ui, ctx, full, self.fullscreen);
            });

        self.record_dialog_frame(ctx);
        self.delete_dialog_frame(ctx);

        // The decoder asks for a repaint per frame, so the UI presents in step
        // with the video. This slow tick keeps the clock and the progress bar
        // moving when nothing is decoding.
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}

impl App {
    /// Keep the tray in step with the setting, act on it, and decide what the
    /// window's close button means.
    ///
    /// Order matters. The icon has to exist *before* a close is turned into a
    /// hide, because a hidden window with no tray icon is an application that
    /// has vanished with no way back — so if the notification area refuses the
    /// icon, closing goes on meaning close.
    fn pump_tray(&mut self, ctx: &egui::Context) {
        if self.settings.minimize_to_tray {
            if self.tray.is_none() {
                let hwnd = self.hwnd;
                self.tray = app_icon().and_then(|icon| {
                    tray::Tray::new(
                        icon.rgba,
                        icon.width,
                        icon.height,
                        &format!("{APP_NAME} — downloads keep running"),
                        hwnd,
                    )
                });
            }
        } else {
            // Dropping it takes the icon out of the notification area and
            // stops its watcher thread, so turning the setting off is visible
            // immediately rather than at the next launch.
            self.tray = None;
        }

        // Nothing is polled here. The tray watches itself on its own thread,
        // because this function only runs when a frame is drawn and a hidden
        // window is never asked to draw one — which is why the tray menu used
        // to do nothing at all once the window had gone.

        // Turn the close into a hide, but only when there is somewhere to hide
        // to.
        if ctx.input(|i| i.viewport().close_requested())
            && self.settings.minimize_to_tray
            && self.tray.is_some()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            // Playback is not a background activity. Downloads are the reason
            // this exists; a tuner held open by a window nobody can see is
            // just a tuner nobody else can use.
            if self.watching {
                self.stop_playback();
            }
        }
    }

    /// Keyboard shortcuts.
    ///
    /// The ones a media application is expected to have. F11 and Escape are the
    /// Windows conventions for full screen; space for play/pause and the arrow
    /// keys for skipping are what every player does, and doing something else
    /// would only be surprising.
    fn handle_keys(&mut self, ctx: &egui::Context) {
        // Whether the keyboard belongs to something else this frame.
        //
        // A focused text field owns it: without this, typing a space into the
        // guide's search box also toggles pause and the arrow keys seek
        // instead of moving the caret. So does a shortcut being rebound —
        // binding an action to G would otherwise switch to the guide on the
        // way past, which is startling and hard to undo.
        //
        // This used to return early. It cannot now, because a menu click
        // arrives through this same function and a menu is perfectly usable
        // while a search box has the caret: the flag suppresses the *keys*
        // and leaves everything else standing.
        let typing =
            ctx.memory(|m| m.focused().is_some()) || self.setup.capturing.is_some();

        // Every binding sampled once, before anything acts on any of them.
        //
        // Two reasons, and both matter. Acting mutates `self`, so a closure
        // holding the settings open to ask "was this pressed" would forbid it.
        // And the toggle below changes whether the rest count at all, which
        // would otherwise mean a key read before it and a key read after it
        // disagreeing about the same frame.
        let fired: std::collections::HashMap<&'static str, bool> = keys::ACTIONS
            .iter()
            .map(|action| {
                (
                    action.id,
                    !typing && keys::pressed(ctx, &self.settings, action.id),
                )
            })
            .collect();

        // The menu bar's clicks, taken for this frame and cleared. An action
        // asked for from a menu is not subject to `typing` — the pointer went
        // to the menu bar, so nothing is being typed — nor to the shortcuts
        // master switch, which is about the keyboard and not about menus.
        let clicked = std::mem::take(&mut self.menu_actions);
        let bound =
            |id: &str| fired.get(id).copied().unwrap_or(false) || clicked.contains(id);

        // The master switch, before anything else and regardless of it, so it
        // can be pressed again to undo itself.
        if bound(keys::TOGGLE) {
            self.settings.shortcuts_enabled = !self.settings.shortcuts_enabled;
            let state = if self.settings.shortcuts_enabled { "on" } else { "off" };
            self.announce(format!("Keyboard shortcuts {state}"));
            self.handle_setup(ui_setup::SetupAction::Save);
        }

        // Escape is not reboundable and not disableable. It is the key every
        // application on this desktop uses to get out of something, and a
        // full-screen window with shortcuts turned off and no way back is a
        // trap worth refusing to build.
        let escape = !typing && ctx.input(|i| i.key_pressed(egui::Key::Escape));

        let (space, left, right, home) = (
            bound("play"),
            bound("back"),
            bound("forward"),
            bound("stop"),
        );

        if bound("fullscreen") {
            self.set_fullscreen(ctx, !self.fullscreen);
        }
        if escape {
            if self.fullscreen {
                self.set_fullscreen(ctx, false);
            } else if self.watching {
                // Escape stops playback outright. An earlier version kept the
                // stream running in the background so returning was instant,
                // but that is not what Escape means anywhere else, and it
                // silently held a tuner — half of them, on a two-tuner box —
                // for a program nobody was watching.
                self.stop_playback();
            }
        }
        if home && self.watching && !self.fullscreen {
            self.stop_playback();
        }

        // Letters, for driving this from across a room without a mouse.
        //
        // Only while not watching. A letter that switched screens mid-programme
        // would tear the picture away on a mistyped key, and the transport is
        // what the keyboard should be reaching during playback.
        if !self.watching {
            let screen = [
                ("home", Screen::Home),
                ("guide", Screen::Guide),
                ("library", Screen::Library),
                ("recordings", Screen::Recordings),
                ("downloads", Screen::Downloads),
                ("settings", Screen::Settings),
            ]
            .into_iter()
            .find(|(id, _)| bound(id))
            .map(|(_, screen)| screen);

            if let Some(screen) = screen {
                if self.screen != screen {
                    self.screen = screen;
                    self.screen_changed = Instant::now();
                }
            }
            if bound("rail") {
                self.rail_expanded = !self.rail_expanded;
            }
            return;
        }

        // Volume, and muting, which is the one control a remote always has.
        let (up_arrow, down_arrow, mute) =
            (bound("volume_up"), bound("volume_down"), bound("mute"));
        if up_arrow || down_arrow {
            let step = if up_arrow { 0.05 } else { -0.05 };
            self.volume = (self.volume + step).clamp(0.0, 1.0);
            if let Some(p) = &self.player {
                p.set_volume(self.volume as f64);
            }
            self.announce(format!("Volume {:.0}%", self.volume * 100.0));
        }
        if mute {
            self.volume = if self.volume <= 0.001 { 1.0 } else { 0.0 };
            if let Some(p) = &self.player {
                p.set_volume(self.volume as f64);
            }
            self.announce(
                if self.volume <= 0.001 { "Muted" } else { "Unmuted" }.into(),
            );
        }

        if space {
            self.paused = !self.paused;
            if let Some(p) = &self.player {
                p.set_paused(self.paused);
            }
        }
        if left {
            let back = self.settings.skip_back_secs as f64;
            if let Some(p) = &self.player {
                if !p.seek_by(-back) {
                    self.announce("This source cannot be rewound".into());
                }
            }
        }
        if right {
            let forward = self.settings.skip_forward_secs as f64;
            if let Some(p) = &self.player {
                p.seek_by(forward);
            }
        }

        // Channel surfing. Steps through the guide as it is currently
        // filtered, so a collection picked in the guide is also the lineup
        // being surfed rather than the whole thousand-channel list.
        let (up, down) = (bound("channel_up"), bound("channel_down"));
        if (up || down) && self.live_channel.is_some() {
            self.surf(if up { -1 } else { 1 });
        }
    }

    /// Tune the next or previous channel in the filtered guide order.
    fn surf(&mut self, delta: isize) {
        let Some(current) = self.live_channel.clone() else { return };
        let rows = guide::filter(
            &self.guide,
            self.guide_state.collection.as_deref(),
            self.guide_state.source.as_deref(),
            "",
        );
        if rows.is_empty() {
            return;
        }

        let index = rows
            .iter()
            .position(|r| r.channel.number == current)
            .unwrap_or(0) as isize;
        // Wrapping, because reaching the end of the lineup and stopping dead
        // is not what a channel button does.
        let count = rows.len() as isize;
        let next = ((index + delta) % count + count) % count;
        let channel = rows[next as usize].channel.number.clone();
        let name = rows[next as usize].channel.name.clone();

        self.watch_channel(&channel);
        self.announce(format!("{channel}  {name}"));
    }

    /// The quality flyout, opened from the transport's quality badge.
    fn quality_menu(&mut self, ctx: &egui::Context) {
        if !self.show_quality || self.playing_local {
            return;
        }

        let mut chosen: Option<QualityChoice> = None;
        let current = self.effective_quality();

        egui::Area::new(egui::Id::new("quality-menu"))
            .anchor(
                egui::Align2::RIGHT_BOTTOM,
                egui::vec2(-SPACE_L, -(96.0 + SPACE_S)),
            )
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(with_alpha(Fluent::SOLID, 240))
                    .rounding(RADIUS_SURFACE)
                    .stroke(egui::Stroke::new(1.0, Fluent::STROKE_SURFACE))
                    .inner_margin(egui::Margin::same(SPACE_S))
                    .show(ui, |ui| {
                        ui.set_min_width(150.0);
                        ui.label(
                            egui::RichText::new("QUALITY")
                                .size(10.0)
                                .color(Fluent::TEXT_TERTIARY),
                        );
                        ui.add_space(SPACE_XS);
                        for choice in QualityChoice::MENU {
                            let selected = choice == current;
                            let label = choice.label();
                            if ui.selectable_label(selected, label).clicked() {
                                chosen = Some(choice);
                            }
                        }
                    });
            });

        if let Some(choice) = chosen {
            self.show_quality = false;
            if choice == current {
                return;
            }
            self.quality_override = Some(choice);

            // Retune the same source in place, carrying the position across.
            // Live rejoins at the live edge, which is where live belongs; a
            // recording comes back exactly where it was.
            if let Some(channel) = self.live_channel.clone() {
                // Rejoin near the live edge, not at the head of the playlist.
                //
                // Channels keeps every segment since the channel was tuned, so
                // by the time anyone changes quality the head of that playlist
                // is however long they have been watching — and joining there
                // restarts the channel from whenever they tuned in. This cannot
                // be corrected with a seek afterwards: a live stream states no
                // duration, so the seekable window is measured from what this
                // player has decoded, and for the first second there is no
                // window at all.
                self.tune(&channel, stream::JoinAt::LiveEdge);
                self.announce(format!("Switched to {}", choice.label()));
            } else if let Some(recording) = self.current_recording.clone() {
                // Elapsed, not the raw clock. Stream timestamps do not start
                // at zero — these recordings begin around 81,876 seconds — so
                // handing the position straight back reopened the file and
                // seeked twenty-two hours past the end of it.
                let position = self.elapsed_position().unwrap_or(0.0);
                self.open_recording(recording, position);
                self.announce(format!("Switched to {}", choice.label()));
            }
        }
    }

    /// How far into the current item playback has reached, in seconds from its
    /// beginning.
    ///
    /// `Player::position` reports the stream's own timeline, whose origin is
    /// whatever the file happens to start at. Everything outside the player —
    /// resume points, commercial markers, what the server records — counts
    /// from zero, and confusing the two has now caused two separate bugs.
    fn elapsed_position(&self) -> Option<f64> {
        let player = self.player.as_ref()?;
        let position = player.position()?;
        let origin = player.seek_range().map(|(start, _)| start).unwrap_or(0.0);
        Some((position - origin).max(0.0))
    }

    /// Stop playback completely: tear the player down, release the tuner, and
    /// return to whatever screen the rail has selected.
    fn stop_playback(&mut self) {
        // One last position report before the player goes away, so stopping
        // two minutes into something records those two minutes.
        self.report_position();
        self.retire_texture();
        self.player = None;
        // Gigabytes, and worthless the moment nothing is reading them.
        self.timeshift = None;
        self.watching = false;
        self.paused = false;
        self.live_channel = None;
        self.commercials.clear();
        self.current_recording = None;
        self.playing_local = false;
        self.show_quality = false;
    }

    fn set_fullscreen(&mut self, ctx: &egui::Context, on: bool) {
        if self.fullscreen == on {
            return;
        }
        self.fullscreen = on;

        // Un-maximize on the way in, and put it back on the way out.
        //
        // This window has no decorations, and Windows constrains a maximized
        // borderless window to the *work area* — the screen less the taskbar.
        // Going fullscreen from that state left it exactly there, so full
        // screen came up with a strip of desktop along the bottom where the
        // taskbar had been. Dropping the maximize first means the fullscreen
        // is applied to an ordinary window and covers the monitor.
        if on {
            self.was_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            if self.was_maximized {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            }
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
        if !on && self.was_maximized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
        // Coming out of full screen with the controls hidden would leave no
        // visible way back, so wake them.
        self.last_activity = Instant::now();
    }

    /// The player.
    fn watch_view(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        full: egui::Rect,
        content: egui::Rect,
    ) {
        if let Some(error) = &self.error {
            ui.painter().text(
                content.center(),
                egui::Align2::CENTER_CENTER,
                error,
                egui::FontId::proportional(14.0),
                Fluent::LIVE,
            );
            return;
        }

        let playing = self
            .player
            .as_ref()
            .map(|p| p.video_size().0 > 0)
            .unwrap_or(false);
        match playing {
            true => {
                // The swoop: the picture rises from below and fades in over a
                // third of a second, easing out. Tuning takes long enough that
                // the arrival deserves to feel like one.
                let entrance = self
                    .first_frame_at
                    .map(|at| {
                        (at.elapsed().as_secs_f32() / (theme::ANIM_SURFACE * 1.6)).min(1.0)
                    })
                    .unwrap_or(1.0);
                let eased = 1.0 - (1.0 - entrance) * (1.0 - entrance) * (1.0 - entrance);
                if entrance < 1.0 {
                    ctx.request_repaint();
                }
                self.draw_video(ui, ctx, content, eased);
            }
            false => tuning_indicator(ui, content, &self.loading),
        }

        // Double click anywhere on the picture toggles full screen, as it does
        // in every other video player.
        let surface = ui.interact(
            content,
            egui::Id::new("video-surface"),
            egui::Sense::click(),
        );
        if surface.double_clicked() {
            self.set_fullscreen(ctx, !self.fullscreen);
        }

        self.captions(ui, content);
        self.fullscreen_button(ui, ctx, content);
        self.transport(ui, ctx, full);
        self.skip_pill(ctx);
        self.quality_menu(ctx);

        if self.show_stats {
            self.stats_card(ui, content);
        }
    }

    /// The break currently playing, as stream-time `(start, end)`.
    fn current_break(&self) -> Option<(f64, f64)> {
        let player = self.player.as_ref()?;
        let origin = player.seek_range().map(|(start, _)| start).unwrap_or(0.0);
        let elapsed = self.elapsed_position()?;
        self.commercials
            .iter()
            .find(|(start, end)| elapsed >= *start && elapsed < end - 0.5)
            .map(|(start, end)| (origin + start, origin + end))
    }

    /// Jump past the current break, or forward to the end of the next one.
    fn skip_break(&mut self) {
        let Some(player) = &self.player else { return };
        let origin = player.seek_range().map(|(start, _)| start).unwrap_or(0.0);
        let Some(elapsed) = self.elapsed_position() else { return };

        let target = self
            .commercials
            .iter()
            // Inside a break: its end. Otherwise the end of the next one
            // ahead, which is what "skip the adverts" means when pressed
            // during the program.
            .find(|(start, end)| (elapsed >= *start && elapsed < end - 0.5) || *start > elapsed)
            .map(|(_, end)| origin + *end);

        match target {
            Some(t) => {
                player.seek_to(t);
            }
            None => self.announce("No commercial breaks left".into()),
        }
    }

    /// "Skip break", shown only while playback is inside one.
    ///
    /// The button is the whole comskip feature from the viewer's side: the
    /// bar's amber bands say where the breaks are, and this takes you past the
    /// one you are in with a single press. It slides in rather than popping,
    /// and disappears the moment it is no longer true.
    fn skip_pill(&mut self, ctx: &egui::Context) {
        let Some(player) = &self.player else { return };
        if self.commercials.is_empty() {
            return;
        }
        let Some(position) = player.position() else { return };

        // Marker times are offsets into the recording; the clock is in stream
        // PTS, which does not start at zero. Comparing the two directly means
        // the pill never appears. See the same correction in `scrub_bar`.
        let origin = player.seek_range().map(|(start, _)| start).unwrap_or(0.0);
        let elapsed = position - origin;

        // A small lead-out: offering "skip" for the last half second of a
        // break would seek past nothing.
        let target = self
            .commercials
            .iter()
            .find(|(start, end)| elapsed >= *start && elapsed < end - 0.5)
            .map(|(_, end)| origin + *end);

        let visible = ctx.animate_bool_with_time(
            egui::Id::new("skip-pill"),
            target.is_some(),
            theme::ANIM_NORMAL,
        );
        if visible <= 0.01 {
            return;
        }

        egui::Area::new(egui::Id::new("skip-pill-area"))
            .anchor(
                egui::Align2::RIGHT_BOTTOM,
                egui::vec2(-SPACE_L * 2.0, -(96.0 + SPACE_L) + (1.0 - visible) * 14.0),
            )
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_opacity(visible);

                // Two sections, because the glyph has to be laid out in the
                // icon family explicitly. It was part of the button's plain
                // RichText, which uses the proportional face, and Segoe UI
                // Variable has nothing in the Private Use Area — so the one
                // control that most needs to look deliberate was rendering a
                // blank box. Every other glyph in the app names the family;
                // this was the only one that did not.
                let ink = egui::Color32::from_rgb(12, 14, 18);
                let mut label = egui::text::LayoutJob::default();
                label.append(
                    "Skip break  ",
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::proportional(14.0),
                        color: ink,
                        valign: egui::Align::Center,
                        ..Default::default()
                    },
                );
                label.append(
                    theme::icon::SKIP_BREAK,
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::new(
                            12.0,
                            egui::FontFamily::Name(theme::ICON_FONT.into()),
                        ),
                        color: ink,
                        valign: egui::Align::Center,
                        ..Default::default()
                    },
                );

                let button = egui::Button::new(label)
                    .fill(Fluent::ACCENT)
                .rounding(18.0)
                .min_size(egui::vec2(128.0, 36.0));

                if ui.add(button).clicked() {
                    if let (Some(end), Some(p)) = (target, &self.player) {
                        p.seek_to(end);
                    }
                }
            });
    }

    /// Whichever browsing screen is selected.
    fn browse_view(&mut self, ui: &mut egui::Ui, content: egui::Rect) {
        // Screens crossfade in rather than cutting. Tracked by hand instead of
        // egui's animate_bool, because that helper snaps to its target the
        // first time an id is seen — which is precisely the moment the fade is
        // supposed to happen.
        if self.shown_screen != self.screen {
            self.shown_screen = self.screen;
            self.screen_changed = Instant::now();
        }
        let t = (self.screen_changed.elapsed().as_secs_f32() / theme::ANIM_SURFACE).min(1.0);
        ui.set_opacity(t * t * (3.0 - 2.0 * t));
        if t < 1.0 {
            ui.ctx().request_repaint();
        }

        match self.screen {
            Screen::Home => {
                let action = ui::home(
                    ui,
                    content,
                    &self.home,
                    &mut self.images,
                    self.home_loading,
                    self.hero_pick,
                );
                self.handle_item(action);
            }
            Screen::Guide => {
                let (action, settings_changed) = ui_guide::guide(
                    ui,
                    content,
                    &self.guide,
                    &mut self.guide_state,
                    &mut self.images,
                    &mut self.settings,
                    now_unix(),
                    self.guide_loading,
                );
                if settings_changed {
                    if let Err(e) = self.settings.save() {
                        self.announce(format!("Could not save settings: {e:#}"));
                    }
                }
                self.handle_guide(action);
            }
            Screen::Downloads => {
                // Local only. Nothing on this screen reaches the DVR: removing
                // a download deletes a file from this machine and leaves the
                // recording on the server untouched.
                let (action, settings_changed) = ui_downloads::downloads_screen(
                    ui,
                    content,
                    &self.home,
                    &mut self.images,
                    &self.downloads,
                    &mut self.settings,
                );
                if settings_changed {
                    if let Err(e) = self.settings.save() {
                        self.announce(format!("Could not save settings: {e:#}"));
                    }
                }
                match action {
                    ui_downloads::DownloadAction::None => {}
                    ui_downloads::DownloadAction::Play(id) => {
                        // `open_recording` prefers a finished download over the
                        // server, so this plays the local file with the network
                        // unplugged — which is the entire point of having it.
                        match self.home.all.iter().find(|r| r.id == id).cloned() {
                            Some(recording) => self.play_recording(&recording),
                            None => self.announce(
                                "That recording is no longer in the library".into(),
                            ),
                        }
                    }
                    ui_downloads::DownloadAction::Pause(id) => {
                        self.downloads.pause(&id);
                    }
                    ui_downloads::DownloadAction::Resume(id) => {
                        // The same call that starts one. Whether it begins or
                        // continues is decided by what is on disk, not here.
                        let url = self.lib.stream_url(&id);
                        let repaint = self.repaint.clone();
                        self.downloads
                            .start(&id, url, move || repaint.request_repaint());
                    }
                    ui_downloads::DownloadAction::Remove(id) => {
                        // Asked first. This deletes a file, and the row it is
                        // on has the remove button next to pause and resume,
                        // which are not destructive at all.
                        let title = self
                            .home
                            .all
                            .iter()
                            .find(|r| r.id == id)
                            .map(|r| r.title.clone())
                            .unwrap_or_else(|| format!("Recording {id}"));
                        self.confirm_delete = Some(Confirm::Download { id, title });
                    }
                    ui_downloads::DownloadAction::ClearFinished => {
                        // The most destructive button on the screen: one click
                        // deletes every finished copy. It said what it would do
                        // in a tooltip and then did it.
                        let finished = self
                            .downloads
                            .entries()
                            .iter()
                            .filter(|(_, status)| status.is_finished())
                            .count();
                        if finished > 0 {
                            self.confirm_delete = Some(Confirm::Finished(finished));
                        }
                    }
                }
            }
            Screen::Library => {
                let (action, settings_changed) = ui_library::library_screen(
                    ui,
                    content,
                    &self.home,
                    &mut self.library_state,
                    &mut self.images,
                    &self.downloads,
                    &mut self.settings,
                    self.home_loading,
                );
                if settings_changed {
                    if let Err(e) = self.settings.save() {
                        self.announce(format!("Could not save settings: {e:#}"));
                    }
                }
                self.handle_item(action);
            }
            Screen::Recordings => {
                let (action, settings_changed) = ui_library::recordings_screen(
                    ui,
                    content,
                    &self.home,
                    &mut self.recordings_tab,
                    &mut self.images,
                    &self.downloads,
                    &mut self.settings,
                    now_unix(),
                    self.home_loading,
                );
                if settings_changed {
                    if let Err(e) = self.settings.save() {
                        self.announce(format!("Could not save settings: {e:#}"));
                    }
                }
                match action {
                    ui_library::RecordingsAction::Cancel(id) => self.cancel_job(id),
                    ui_library::RecordingsAction::Item(item) => self.handle_item(item),
                    ui_library::RecordingsAction::None => {}
                }
            }
            Screen::Settings => {
                let action = ui_setup::settings_screen(
                    ui,
                    content,
                    &mut self.settings,
                    &mut self.setup,
                );
                self.handle_setup(action);
            }
        }
    }

    /// Act on something asked of a recording, from whichever screen asked it.
    fn handle_item(&mut self, action: ui::Action) {
        match action {
            ui::Action::Play(recording) => self.play_recording(&recording),
            ui::Action::WatchLive => self.screen = Screen::Guide,
            ui::Action::Download(recording) => self.start_download(&recording),
            ui::Action::RemoveDownload(id) => self.downloads.remove(&id),
            ui::Action::SetWatched(id, watched) => self.set_watched(id, watched),
            ui::Action::Delete(recording) => {
                self.confirm_delete = Some(Confirm::Recording(recording))
            }
            ui::Action::None => {}
        }
    }

    fn handle_setup(&mut self, action: ui_setup::SetupAction) {
        match action {
            ui_setup::SetupAction::None => {}
            ui_setup::SetupAction::Save => {
                if let Err(e) = self.settings.save() {
                    self.announce(format!("Could not save settings: {e:#}"));
                }
                // A rebinding may have just happened, and the menu bar has to
                // agree with it. Cheap enough to do on every save rather than
                // trying to work out whether this one touched the keyboard.
                platform::sync_menu_shortcuts(&self.settings);
            }
            ui_setup::SetupAction::PickFolder(which) => {
                // On its own thread. The picker runs its own message loop and
                // does not return until it is dismissed, so calling it from
                // here would stop this window drawing — including the dialog's
                // own parent, which is the thing behind it.
                let tx = self.tx.clone();
                let ctx = self.repaint.clone();
                let start = match which {
                    ui_setup::Folder::Downloads => self.settings.download_path(),
                    ui_setup::Folder::Buffer => self.settings.buffer_path(),
                    ui_setup::Folder::Cache => self
                        .settings
                        .cache_path()
                        .or_else(paths::data_dir)
                        .unwrap_or_else(std::env::temp_dir),
                };
                std::thread::spawn(move || {
                    let picked = rfd::FileDialog::new()
                        .set_title(match which {
                            ui_setup::Folder::Downloads => "Where downloads are kept",
                            ui_setup::Folder::Buffer => "Where the live buffer is written",
                            ui_setup::Folder::Cache => "Where caches and logs are kept",
                        })
                        // Opening where the current setting points, so changing
                        // it starts from where it is rather than from wherever
                        // the shell last happened to be.
                        .set_directory(&start)
                        .pick_folder();
                    if let Some(path) = picked {
                        let _ = tx.send(Msg::FolderPicked(which, path));
                        ctx.request_repaint();
                    }
                });
            }
            ui_setup::SetupAction::Probe(input) => {
                let url = settings::normalize(&input);
                if url.is_empty() {
                    return;
                }
                self.setup.probing = true;
                self.setup.message = None;
                let tx = self.tx.clone();
                let ctx = self.repaint.clone();
                self.runtime.spawn(async move {
                    let result = settings::probe(&url)
                        .await
                        .map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Msg::Probed(url, result));
                    ctx.request_repaint();
                });
            }
            ui_setup::SetupAction::Select(index) => {
                self.settings.active = index;
                let _ = self.settings.save();
                self.reconnect();
            }
            ui_setup::SetupAction::Remove(index) => {
                self.settings.remove_server(index);
                let _ = self.settings.save();
                self.reconnect();
            }
        }
    }

    /// Act on a click in the guide.
    fn handle_guide(&mut self, action: ui_guide::GuideAction) {
        use ui_guide::GuideAction as G;
        match action {
            G::None => {}
            G::Watch(channel) => self.watch_channel(&channel),
            G::Record(airing) => self.schedule(airing, false),
            G::RecordSeries(airing) => self.schedule(airing, true),
            G::CancelJob(id, _) => self.cancel_job(id),
            G::CancelSeries(airing) => {
                let Some(rule) = self.guide.schedule.rule_for(&airing).cloned() else {
                    self.announce("No series pass found for this program".into());
                    return;
                };
                let dvr = self.dvr.clone();
                let tx = self.tx.clone();
                let ctx = self.repaint.clone();
                let title = airing.title.clone();
                self.runtime.spawn(async move {
                    let note = match dvr.delete_rule(&rule).await {
                        Ok(()) => format!("Stopped recording {title}"),
                        Err(e) => format!("Could not cancel the pass: {e:#}"),
                    };
                    let _ = tx.send(Msg::ScheduleChanged(note));
                    ctx.request_repaint();
                });
            }
            G::OpenRecord(airing) => {
                self.record_dialog = Some(ui_record::RecordDialog::new(
                    airing,
                    self.padding.start,
                    self.padding.end,
                ));
            }
        }
    }

    fn cancel_job(&mut self, id: String) {
        let dvr = self.dvr.clone();
        let tx = self.tx.clone();
        let ctx = self.repaint.clone();
        self.runtime.spawn(async move {
            let note = match dvr.delete_job(&id).await {
                Ok(()) => "Recording canceled".to_string(),
                Err(e) => format!("Could not cancel: {e:#}"),
            };
            let _ = tx.send(Msg::ScheduleChanged(note));
            ctx.request_repaint();
        });
    }

    /// The record dialog, when one is open.
    fn record_dialog_frame(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.record_dialog else { return };
        match dialog.show(ctx) {
            ui_record::RecordChoice::Pending => {}
            ui_record::RecordChoice::Canceled => self.record_dialog = None,
            ui_record::RecordChoice::Once { start_pad, end_pad } => {
                let airing = dialog.airing.clone();
                self.record_dialog = None;
                self.schedule_with(airing, false, start_pad, end_pad, true, 0);
            }
            ui_record::RecordChoice::Series {
                start_pad,
                end_pad,
                new_only,
                keep,
            } => {
                let airing = dialog.airing.clone();
                self.record_dialog = None;
                self.schedule_with(airing, true, start_pad, end_pad, new_only, keep);
            }
        }
    }

    /// Confirmation before deleting a recording.
    ///
    /// The file is gone from the DVR afterwards, so this asks first and names
    /// what will go. The destructive button is the one that has to be aimed
    /// at, not the one under the cursor.
    fn delete_dialog_frame(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.confirm_delete.clone() else { return };
        let mut open = true;
        let mut decision: Option<bool> = None;

        // What is about to happen, said in the terms of the thing itself. A
        // dialog that says "delete this?" for both a recording on the server
        // and a copy on this disk is asking two very different questions with
        // one sentence.
        let (window, heading, detail, warning) = match &pending {
            Confirm::Recording(recording) => (
                "Delete recording",
                recording.title.clone(),
                recording.subtitle(),
                "This deletes the file from the DVR. It cannot be undone.".to_string(),
            ),
            Confirm::Download { title, .. } => (
                "Delete download",
                title.clone(),
                String::new(),
                "This deletes the copy on this machine. The recording stays on the DVR."
                    .to_string(),
            ),
            Confirm::Finished(count) => (
                "Delete finished downloads",
                format!("{count} finished download{}", if *count == 1 { "" } else { "s" }),
                String::new(),
                "This deletes their copies on this machine. The recordings stay on the DVR."
                    .to_string(),
            ),
        };

        egui::Window::new(window)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("Are you sure?")
                        .size(13.0)
                        .color(Fluent::TEXT_SECONDARY),
                );
                ui.add_space(SPACE_S);
                ui.label(
                    egui::RichText::new(&heading)
                        .size(16.0)
                        .color(Fluent::TEXT_PRIMARY),
                );
                if !detail.is_empty() {
                    ui.label(
                        egui::RichText::new(detail)
                            .size(12.0)
                            .color(Fluent::TEXT_SECONDARY),
                    );
                }
                ui.add_space(SPACE_M);
                ui.label(
                    egui::RichText::new(warning)
                        .size(12.0)
                        .color(Fluent::CAUTION),
                );
                ui.add_space(SPACE_L);
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new("Cancel").min_size(egui::vec2(96.0, 32.0)))
                        .clicked()
                    {
                        decision = Some(false);
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Delete")
                                    .color(egui::Color32::from_rgb(16, 16, 18)),
                            )
                            .fill(Fluent::LIVE)
                            .min_size(egui::vec2(96.0, 32.0)),
                        )
                        .clicked()
                    {
                        decision = Some(true);
                    }
                });
            });

        if !open {
            self.confirm_delete = None;
            return;
        }
        match decision {
            Some(true) => {
                self.confirm_delete = None;
                match pending {
                    Confirm::Recording(recording) => self.delete_recording(&recording),
                    Confirm::Download { id, .. } => {
                        self.downloads.remove(&id);
                        self.announce("Removed the local copy".into());
                    }
                    Confirm::Finished(_) => {
                        self.downloads.clear_finished();
                        self.announce("Cleared finished downloads".into());
                    }
                }
            }
            Some(false) => self.confirm_delete = None,
            None => {}
        }
    }

    fn delete_recording(&mut self, recording: &library::Recording) {
        // The local copy goes too. Leaving a download behind for a recording
        // the server no longer has would show it in the library forever with
        // nothing to refresh it from.
        self.downloads.remove(&recording.id);

        let lib = library::Library::new(self.settings.server_url());
        let tx = self.tx.clone();
        let ctx = self.repaint.clone();
        let id = recording.id.clone();
        let title = recording.title.clone();
        self.runtime.spawn(async move {
            let note = match lib.delete(&id).await {
                Ok(()) => format!("Deleted {title}"),
                Err(e) => format!("Could not delete: {e:#}"),
            };
            let _ = tx.send(Msg::LibraryChanged(note));
            ctx.request_repaint();
        });
    }

    fn set_watched(&mut self, id: String, watched: bool) {
        let lib = library::Library::new(self.settings.server_url());
        let tx = self.tx.clone();
        let ctx = self.repaint.clone();
        self.runtime.spawn(async move {
            let note = match lib.set_watched(&id, watched).await {
                Ok(()) => {
                    if watched { "Marked watched" } else { "Marked unwatched" }.to_string()
                }
                Err(e) => format!("Could not update: {e:#}"),
            };
            let _ = tx.send(Msg::LibraryChanged(note));
            ctx.request_repaint();
        });
    }

    /// Schedule an airing using the server's default padding.
    fn schedule(&mut self, airing: guide::Airing, series: bool) {
        let (start, end) = (self.padding.start, self.padding.end);
        self.schedule_with(airing, series, start, end, true, 0);
    }

    /// Schedule an airing, as one recording or as a whole series.
    fn schedule_with(
        &mut self,
        airing: guide::Airing,
        series: bool,
        start_pad: i64,
        end_pad: i64,
        new_only: bool,
        keep: i64,
    ) {
        let dvr = self.dvr.clone();
        let tx = self.tx.clone();
        let ctx = self.repaint.clone();
        let title = airing.title.clone();

        // Channel and start are enough: the server's own object for this
        // airing is looked up from the guide's cache on disk when the job is
        // built, rather than carried through every airing in memory.
        let job = api::Airing {
            title: airing.title.clone(),
            subtitle: airing.episode_title.clone(),
            start: airing.start,
            duration: airing.duration,
            channel: airing.channel.clone(),
        };
        let padding = api::Padding {
            start: start_pad,
            end: end_pad,
        };

        self.runtime.spawn(async move {
            let note = if series {
                let options = api::PassOptions {
                    padding,
                    new_only,
                    keep,
                };
                match dvr
                    .create_series_rule(&job, airing.series_id.as_str(), options)
                    .await
                {
                    Ok(()) => format!("Recording every episode of {title}"),
                    Err(e) => format!("Could not create the pass: {e:#}"),
                }
            } else {
                match dvr.create_job(&job, padding).await {
                    Ok(_) => format!("Recording {title}"),
                    Err(e) => format!("Could not record: {e:#}"),
                }
            };
            let _ = tx.send(Msg::ScheduleChanged(note));
            ctx.request_repaint();
        });
    }

    /// Everything that has to happen on a clock rather than on a click:
    /// telling the server where playback is, keeping the guide from going
    /// stale, and noticing that the stream has died.
    /// Keep the settings file's idea of the window in step with the real one.
    ///
    /// Sampled every frame, written rarely. A drag reports a new position on
    /// every one of them, and a settings file rewritten sixty times a second
    /// for the length of a drag is a lot of disk for a rectangle — so a change
    /// starts a short timer and the write happens once the window has been
    /// still for it. Closing the window is not a special case: the last move
    /// before it settles for good is the one that gets written.
    ///
    /// Maximizing is deliberately not a move. The rectangle stored is always
    /// the restored one, so un-maximizing after a restart gives back the window
    /// someone actually sized, not the full screen they last saw.
    fn remember_window(&mut self, ctx: &egui::Context) {
        const SETTLE: Duration = Duration::from_millis(700);

        let (rect, maximized) = ctx.input(|i| {
            let info = i.viewport();
            (info.outer_rect, info.maximized.unwrap_or(false))
        });

        let mut wanted = self.settings.window;
        if maximized {
            // Only the flag. Where a maximized window "is" is a property of the
            // monitor, not of anything worth remembering.
            match wanted.as_mut() {
                Some(window) => window.maximized = true,
                // Maximized before it was ever restored: nothing to keep but
                // the flag, and the default size to come back to.
                None => {
                    wanted = Some(settings::Window {
                        x: f32::NAN,
                        y: f32::NAN,
                        width: 1280.0,
                        height: 760.0,
                        maximized: true,
                    })
                }
            }
        } else if let Some(rect) = rect {
            wanted = Some(settings::Window {
                x: rect.min.x,
                y: rect.min.y,
                width: rect.width(),
                height: rect.height(),
                maximized: false,
            });
        }

        let changed = match (wanted, self.settings.window) {
            (Some(new), Some(old)) => {
                // A pixel of jitter is not a move. Without this, a window that
                // reports 900.0001 forever rewrites the file forever.
                new.maximized != old.maximized
                    || (new.x - old.x).abs() > 1.0
                    || (new.y - old.y).abs() > 1.0
                    || (new.width - old.width).abs() > 1.0
                    || (new.height - old.height).abs() > 1.0
            }
            (Some(_), None) => true,
            _ => false,
        };

        if changed {
            self.settings.window = wanted;
            self.window_settled = Some(Instant::now());
            // Nothing else asks for a repaint while the window sits still, so
            // without this the write would wait for whatever redraws next.
            ctx.request_repaint_after(SETTLE);
        } else if self
            .window_settled
            .is_some_and(|since| since.elapsed() >= SETTLE)
        {
            self.window_settled = None;
            if let Err(e) = self.settings.save() {
                eprintln!("[clicker] could not save the window position: {e:#}");
            }
        }
    }

    fn housekeeping(&mut self) {
        // ── Position, every 20 seconds ──────────────────────────────────
        //
        // Without this, everything watched here is invisible to Continue
        // Watching, Up Next, and every other Channels client. Live has no
        // position to report, and a local file has no server to report to.
        if self.watching && !self.paused && !self.playing_local {
            if self.position_reported_at.elapsed() > Duration::from_secs(20) {
                self.position_reported_at = Instant::now();
                self.report_position();
            }
        }

        // ── Guide freshness, every 30 minutes ───────────────────────────
        //
        // Listings are a moving window. Half an hour in, the leftmost column
        // is describing programs that already finished.
        if self.settings.configured()
            && !self.guide_loading
            && self.guide_loaded_at.elapsed() > Duration::from_secs(1800)
        {
            self.guide_loaded_at = Instant::now();
            let server = self.settings.server_url();
            spawn_guide(&self.runtime, &self.tx, self.repaint.clone(), &server);
        }

        // ── Live buffer ─────────────────────────────────────────────────
        //
        // Keep the seekable window in step with what the buffer still holds.
        // It recycles as it runs, so the earliest thing that can be rewound to
        // moves forward all the while.
        if let (Some(buffer), Some(player)) = (&self.timeshift, &self.player) {
            player.set_discarded(buffer.discarded_fraction());
        }

        // ── Server health ───────────────────────────────────────────────
        //
        // A single cheap request, on a timer, rather than threading failure
        // reporting through every call site. It catches a sleeping laptop, a
        // changed network, a rebooted DVR and a pulled cable identically,
        // because from here they are all the same thing: the server stopped
        // answering and has to be waited for.
        let now_secs = now_unix();
        let asleep_for = now_secs - self.last_tick_unix;
        self.last_tick_unix = now_secs;

        // A gap far larger than the repaint interval means this process was
        // not running — the machine slept, or the window was hidden long
        // enough to stop being redrawn. Whatever was true about the network
        // before that gap is not evidence of anything now.
        if asleep_for > 45 {
            self.next_health_check = Instant::now();
            if self.watching {
                self.announce("Reconnecting after sleep…".into());
                self.reopen_current();
            }
        }

        if self.settings.configured()
            && !self.probing_server
            && Instant::now() >= self.next_health_check
        {
            self.probing_server = true;
            let server = self.settings.server_url();
            let tx = self.tx.clone();
            let notify = self.repaint.clone();
            self.runtime.spawn(async move {
                let message = match settings::probe(&server).await {
                    Ok(_) => Msg::ServerHealth(true, String::new()),
                    Err(e) => Msg::ServerHealth(false, format!("{e:#}")),
                };
                let _ = tx.send(message);
                notify.request_repaint();
            });
        }

        // ── Stall detection ─────────────────────────────────────────────
        //
        // A closed laptop lid or a dropped wifi leaves the HTTP connection
        // dead and the player sitting on a frozen frame forever. Frames
        // stopping while unpaused is the symptom; reopening the same source
        // is the cure, and it is the same one for both causes.
        if self.watching && !self.paused {
            if let Some(player) = &self.player {
                let frames = player.decoded();
                if frames != self.stall_watch_frames {
                    self.stall_watch_frames = frames;
                    self.stalled_since = None;
                } else {
                    let since = *self.stalled_since.get_or_insert_with(Instant::now);
                    // Generous: a slow segment fetch is not a dead connection,
                    // and reopening a stream that was merely thinking would be
                    // worse than waiting.
                    if since.elapsed() > Duration::from_secs(12) {
                        self.stalled_since = None;
                        self.announce("Reconnecting…".into());
                        self.reopen_current();
                    }
                }
            }
        } else {
            self.stalled_since = None;
        }
    }

    /// Send the current position to the server.
    fn report_position(&self) {
        let Some(recording) = &self.current_recording else { return };
        // Elapsed from the start of the recording, which is what the server
        // stores and every other client expects.
        let Some(position) = self.elapsed_position() else { return };
        if position <= 1.0 {
            return;
        }

        let lib = library::Library::new(self.settings.server_url());
        let id = recording.id.clone();
        let duration = recording.duration;
        self.runtime.spawn(async move {
            let _ = lib.report_position(&id, position).await;
            // Past 95% it is finished in every sense that matters, and that is
            // the same threshold the server uses to decide what is unwatched.
            if duration > 0.0 && position / duration >= 0.95 {
                let _ = lib.set_watched(&id, true).await;
            }
        });
    }

    /// Reopen whatever is playing, at the position it had reached.
    fn reopen_current(&mut self) {
        if let Some(channel) = self.live_channel.clone() {
            self.watch_channel(&channel);
        } else if let Some(recording) = self.current_recording.clone() {
            let position = self
                .elapsed_position()
                .unwrap_or(recording.playback_time);
            self.open_recording(recording, position);
        }
    }

    /// Open a source off the UI thread.
    ///
    /// `Player::open` blocks on the network — a connection, then a probe of
    /// several megabytes — which is over a second on a live tune. Doing it on
    /// the UI thread froze the window at exactly the moment the tuning
    /// animation was supposed to be showing that work was happening.
    fn spawn_open(
        &mut self,
        uri: String,
        transport: stream::Transport,
        resume_at: f64,
        join: stream::JoinAt,
    ) {
        self.retire_texture();
        self.player = None;
        self.transport = Some(transport);
        self.first_frame_at = None;
        self.watching = true;
        self.paused = false;
        self.open_generation += 1;

        let generation = self.open_generation;
        let tx = self.tx.clone();
        let notify = self.repaint.clone();
        let frame_repaint = self.repaint.clone();
        // Read here rather than on the thread: the settings belong to the
        // interface and the thread must not reach back into them.
        let software_decoding = self.settings.software_decoding;
        std::thread::spawn(move || {
            // A live buffer is created empty and the tuner takes several
            // seconds to produce its first byte, so opening straight away only
            // ever gets "invalid data". The demuxer needs enough of a
            // transport stream to find the programs and probe the codecs.
            //
            // Waited for here rather than before the thread is spawned: this
            // is exactly the blocking the interface must not do, and the
            // tuning animation is already on screen while it happens.
            if transport == stream::Transport::Timeshift {
                // Enough transport stream for the demuxer to find a program,
                // and no more. This used to wait for a megabyte, because a
                // demuxer that ran out of file while probing gave up and
                // reported invalid data. It no longer runs out: the buffer is
                // opened in follow mode, so a short read waits for the writer
                // instead of ending. The wait that remains is only so there is
                // something rather than nothing, and at broadcast rates it is
                // about a third of a second instead of two.
                const ENOUGH: u64 = 192 * 1024;
                let deadline = Instant::now() + Duration::from_secs(30);
                while Instant::now() < deadline {
                    let size = std::fs::metadata(&uri).map(|m| m.len()).unwrap_or(0);
                    if size >= ENOUGH {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }

            let result = mpv::Player::open(
                &uri,
                resume_at,
                join,
                transport,
                software_decoding,
                move || frame_repaint.request_repaint(),
            )
            .map(Arc::new);
            let _ = tx.send(Msg::PlayerOpened {
                result,
                resume_at,
                generation,
            });
            notify.request_repaint();
        });
    }

    /// The quality in effect: the session's pick, or the global default.
    fn effective_quality(&self) -> QualityChoice {
        self.quality_override.unwrap_or({
            if self.settings.original_quality {
                QualityChoice::Original
            } else {
                QualityChoice::Height(self.settings.transcode_height)
            }
        })
    }

    /// Tune a live channel.
    ///
    /// A fresh tune joins where the server's playlist begins, which for a
    /// channel being tuned now is a few seconds back — a buffer held on the
    /// server rather than in memory.
    fn watch_channel(&mut self, channel: &str) {
        self.tune(channel, stream::JoinAt::Start);
    }

    /// Tune a channel, choosing where a live playlist is joined.
    fn tune(&mut self, channel: &str, join: stream::JoinAt) {
        let server = self.settings.server_url();
        let quality = self.effective_quality();
        let uri = stream_uri(&server, channel, quality);

        self.loading = Loading::live(channel);
        self.live_channel = Some(channel.to_string());
        self.commercials.clear();
        self.current_recording = None;
        self.playing_local = false;

        // Direct has no timeshift of its own — one endless HTTP body with no
        // ranges — so the stream is written to disk here and the player opens
        // the file instead. Pause and rewind are not optional on live TV, and
        // this is what buys them back without asking the server to do anything.
        //
        // Dropping the previous buffer stops its writer and deletes it, which
        // is why the new one is built into a variable first: doing it the
        // other way round would leave two writers on the same channel for as
        // long as the assignment took.
        let keep = self.settings.live_buffer_gb as u64 * 1024 * 1024 * 1024;
        let buffer = if quality == QualityChoice::Original && keep > 0 {
            let http = reqwest::Client::builder()
                .user_agent(settings::user_agent())
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            match timeshift::Timeshift::start(
                self.runtime.handle(),
                http,
                uri.clone(),
                channel,
                keep,
                self.settings.buffer_path(),
            ) {
                Ok(buffer) => Some(buffer),
                Err(e) => {
                    // Not fatal: without a buffer the picture still plays,
                    // just without rewind, which is better than not tuning.
                    self.announce(format!("No live buffer: {e}"));
                    None
                }
            }
        } else {
            None
        };

        let (uri, transport) = match &buffer {
            Some(buffer) => (
                buffer.path().to_string_lossy().into_owned(),
                stream::Transport::Timeshift,
            ),
            None => {
                let transport = stream::Transport::of(&uri);
                (uri, transport)
            }
        };
        self.timeshift = buffer;

        self.spawn_open(uri, transport, 0.0, join);
    }

    /// Begin fetching a recording to local disk.
    fn start_download(&mut self, recording: &library::Recording) {
        let url = self.lib.stream_url(&recording.id);
        let repaint = self.repaint.clone();
        self.downloads
            .start(&recording.id, url, move || repaint.request_repaint());
        self.announce(format!("Downloading {}", recording.title));
    }

    /// Where to stream a recording from, at a given quality.
    ///
    /// Original goes straight to the file, which serves with byte ranges and
    /// seeks perfectly. A transcode goes through the recording's own HLS
    /// endpoint, which accepts the same parameters as live — verified against
    /// the server: `resolution=720` on a recording comes back honored.
    fn recording_uri(&self, id: &str, quality: QualityChoice) -> String {
        let server = self.settings.server_url();
        match quality {
            QualityChoice::Original => format!("{server}/dvr/files/{id}/stream.mpg"),
            QualityChoice::Height(h) => format!(
                "{server}/dvr/files/{id}/hls/master.m3u8?vcodec=h264&acodec=copy&resolution={h}&bitrate={}",
                quality.kbps()
            ),
        }
    }

    /// Open a recording, resuming where it was left.
    fn play_recording(&mut self, recording: &library::Recording) {
        self.open_recording(recording.clone(), recording.playback_time);
    }

    /// Open a recording at a given position.
    ///
    /// A finished download is preferred over the server: it plays with the
    /// network unplugged, and at home it is byte-identical to what the server
    /// would have sent anyway. Downloads always hold the original file, so
    /// quality selection does not apply to them.
    fn open_recording(&mut self, recording: library::Recording, resume_at: f64) {
        let (uri, local) = match self.downloads.local_path(&recording.id) {
            Some(path) => (path.to_string_lossy().into_owned(), true),
            None => (
                self.recording_uri(&recording.id, self.effective_quality()),
                false,
            ),
        };
        let transport = stream::Transport::of(&uri);

        self.loading = Loading::recording(&recording.title);
        self.live_channel = None;
        self.playing_local = local;
        // The boundary list alternates break-start, break-end.
        self.commercials = recording
            .commercials
            .chunks_exact(2)
            .map(|pair| (pair[0], pair[1]))
            .filter(|(start, end)| end > start)
            .collect();
        self.current_recording = Some(recording);

        self.spawn_open(uri, transport, resume_at, stream::JoinAt::Start);
    }

    /// How visible the controls should be, 0 to 1.
    ///
    /// They fade out after a few seconds of a still pointer and come straight
    /// back on any movement. Anything mid-interaction pins them open: fading
    /// out from under a drag, or while the stats card is being read, would be
    /// the overlay fighting the person using it.
    fn controls_opacity(&mut self, ctx: &egui::Context) -> f32 {
        let active = ctx.input(|i| {
            i.pointer.velocity().length() > 1.0
                || i.pointer.any_down()
                || i.pointer.any_click()
                || !i.raw.events.is_empty()
        });
        if active {
            self.last_activity = Instant::now();
        }
        if self.scrubbing || self.show_stats || self.job_pending {
            return 1.0;
        }

        const HOLD: f32 = 2.75;
        const FADE: f32 = 0.45;
        let idle = self.last_activity.elapsed().as_secs_f32();
        let opacity = 1.0 - ((idle - HOLD) / FADE).clamp(0.0, 1.0);

        // Repaint through the fade, otherwise it freezes part way whenever the
        // decoder happens not to deliver a frame.
        if opacity > 0.0 && idle > HOLD {
            ctx.request_repaint();
        }
        opacity
    }

    /// Says the server has gone, and points at what can still be watched.
    ///
    /// Sitting on a browsing screen with a dead DVR, everything on it is a
    /// promise that cannot be kept: the artwork is cached, the titles are
    /// cached, and clicking any of them fails. Downloads are the exception —
    /// they are on this disk — so the thing to do is say so and offer them,
    /// rather than leave a library that looks fine and is not.
    ///
    /// Shown only where it helps: not on the Downloads screen, which is
    /// already the answer, and not when there is nothing downloaded, when it
    /// would be an offer of nothing.
    fn offline_banner(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        if self.online || self.screen == Screen::Downloads {
            return;
        }
        let ready = self
            .downloads
            .entries()
            .iter()
            .filter(|(_, status)| matches!(status, downloads::Status::Done(_)))
            .count();

        let (title, detail) = if ready > 0 {
            (
                format!("Offline — {ready} downloaded to watch"),
                "The DVR is not answering. Downloads play from this machine.",
            )
        } else {
            (
                "Offline".to_string(),
                "The DVR is not answering. Reconnecting automatically.",
            )
        };

        let width = 430.0_f32.min(rect.width() - SPACE_L * 2.0);
        let height = 66.0;
        let card = egui::Rect::from_min_size(
            egui::pos2(
                rect.center().x - width / 2.0,
                rect.max.y - height - SPACE_L * 2.0,
            ),
            egui::vec2(width, height),
        );

        let response = ui.interact(
            card,
            egui::Id::new("offline-banner"),
            if ready > 0 { egui::Sense::click() } else { egui::Sense::hover() },
        );
        let hover = ui.ctx().animate_bool_with_time(
            egui::Id::new("offline-banner-h"),
            ready > 0 && response.hovered(),
            theme::ANIM_FAST,
        );

        ui.painter().rect_filled(
            card,
            RADIUS_SURFACE,
            theme::mix(with_alpha(Fluent::SOLID, 245), Fluent::CONTROL_HOVER, hover),
        );
        ui.painter().rect_stroke(
            card,
            RADIUS_SURFACE,
            egui::Stroke::new(1.0, with_alpha(Fluent::LIVE, 110)),
        );
        ui.painter().circle_filled(
            egui::pos2(card.min.x + SPACE_L + 4.0, card.center().y),
            4.0,
            Fluent::LIVE,
        );

        let text_x = card.min.x + SPACE_L + 18.0;
        ui.painter().text(
            egui::pos2(text_x, card.center().y - 10.0),
            egui::Align2::LEFT_CENTER,
            title,
            egui::FontId::proportional(13.5),
            Fluent::TEXT_PRIMARY,
        );
        ui.painter().text(
            egui::pos2(text_x, card.center().y + 11.0),
            egui::Align2::LEFT_CENTER,
            detail,
            egui::FontId::proportional(11.5),
            Fluent::TEXT_TERTIARY,
        );

        if ready > 0 {
            ui.painter().text(
                egui::pos2(card.max.x - SPACE_M, card.center().y),
                egui::Align2::RIGHT_CENTER,
                theme::icon::FORWARD,
                egui::FontId::new(12.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
                Fluent::TEXT_SECONDARY,
            );
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if response.clicked() {
                self.screen = Screen::Downloads;
            }
        }
    }

    /// Full screen, at the top right of the picture.
    ///
    /// Not in the transport: it belongs at the corner of the thing it makes
    /// bigger, which is where every video player puts it and where the pointer
    /// already goes. It appears and fades with the rest of the controls, so it
    /// is there when the pointer moves and gone while watching.
    ///
    /// Below the caption bar, not over it — the window's own buttons live in
    /// that corner and two things to click in one place is how the X on the
    /// player came to be mistaken for quit.
    fn fullscreen_button(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, rect: egui::Rect) {
        let opacity = self.controls_opacity(ctx);
        if opacity <= 0.001 {
            return;
        }
        let fade = |c: egui::Color32, a: u8| with_alpha(c, (a as f32 * opacity) as u8);

        const SIZE: f32 = 38.0;
        let at = egui::Rect::from_min_size(
            egui::pos2(rect.max.x - SIZE - SPACE_L, rect.min.y + SPACE_L),
            egui::vec2(SIZE, SIZE),
        );

        let response = ui.interact(at, egui::Id::new("fullscreen-corner"), egui::Sense::click());
        let hover = ctx.animate_bool_with_time(
            egui::Id::new("fullscreen-corner-hover"),
            response.hovered(),
            theme::ANIM_FAST,
        );

        let painter = ui.painter();
        painter.rect_filled(
            at,
            theme::RADIUS_CONTROL,
            fade(
                theme::mix(Fluent::LAYER_CARD, Fluent::CONTROL_HOVER, hover),
                215,
            ),
        );
        painter.rect_stroke(
            at,
            theme::RADIUS_CONTROL,
            egui::Stroke::new(1.0, fade(Fluent::STROKE_CONTROL, 255)),
        );
        painter.text(
            at.center(),
            egui::Align2::CENTER_CENTER,
            if self.fullscreen {
                theme::icon::EXIT_FULLSCREEN
            } else {
                theme::icon::FULLSCREEN
            },
            egui::FontId::new(14.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
            fade(Fluent::TEXT_PRIMARY, 255),
        );

        if response.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let response = response.on_hover_text(if self.fullscreen {
            "Leave full screen (F11)"
        } else {
            "Full screen (F11)"
        });
        if response.clicked() {
            self.set_fullscreen(ctx, !self.fullscreen);
        }
    }

    /// Closed captions, drawn over the picture.
    ///
    /// Sat above the transport rather than behind it: a caption hidden under
    /// the controls is worse than one that moves, so it rides up when they are
    /// showing and drops back down when they fade.
    ///
    /// Drawn on a plate rather than with an outline. Broadcast captions land
    /// on whatever the picture happens to be, and white-on-white is a caption
    /// that is not there — which is the whole failure this is meant to fix.
    fn captions(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let Some(player) = &self.player else { return };
        let Some(text) = player.caption() else { return };
        if text.is_empty() {
            return;
        }

        let controls_up = self.last_activity.elapsed().as_secs_f32() < 3.2;
        let lift = if controls_up { 112.0 } else { 40.0 };

        // Scaled to the window. A fixed size is most of the picture on a small
        // window and a whisper on a large one.
        let size = (rect.width() / 46.0).clamp(13.0, 26.0);
        // Monospaced and upper case, which is the look a caption decoder has:
        // CEA-608 is a fixed 32-column grid of capitals, and rendering it in a
        // proportional face reads as a film subtitle rather than as captions.
        let font = egui::FontId::monospace(size);
        let upper = text.to_uppercase();

        let lines: Vec<&str> = upper
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        if lines.is_empty() {
            return;
        }

        // Laid out first, because the block is drawn upwards from the bottom
        // and its height is not known until every line has been measured.
        let max_width = rect.width() * 0.86;
        let galleys: Vec<_> = lines
            .iter()
            .map(|line| {
                ui.painter()
                    .layout((*line).to_string(), font.clone(), Fluent::TEXT_PRIMARY, max_width)
            })
            .collect();

        let pad = egui::vec2(size * 0.5, size * 0.16);
        let block: f32 = galleys.iter().map(|g| g.size().y + pad.y * 2.0).sum();
        let mut y = rect.max.y - lift - block;

        for galley in galleys {
            let line_size = galley.size();
            let origin = egui::pos2(rect.center().x - line_size.x / 2.0, y + pad.y);
            // A box per line, tight to the text, with square corners. One
            // rounded box around the whole block is a subtitle; the ragged
            // per-line box is what makes it read as a caption.
            let plate = egui::Rect::from_min_size(
                egui::pos2(origin.x - pad.x, y),
                egui::vec2(line_size.x + pad.x * 2.0, line_size.y + pad.y * 2.0),
            );
            ui.painter()
                .rect_filled(plate, 0.0, egui::Color32::from_rgba_unmultiplied(0, 0, 0, 235));
            ui.painter().galley(origin, galley, Fluent::TEXT_PRIMARY);
            y += line_size.y + pad.y * 2.0;
        }
    }

    /// The transport, drawn over the picture as a single Fluent surface.
    fn transport(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, full: egui::Rect) {
        let opacity = self.controls_opacity(ctx);
        if opacity <= 0.001 {
            ctx.set_cursor_icon(egui::CursorIcon::None);
            return;
        }

        // Anchored to the bottom of the window rather than positioned at a
        // computed y. An Area sizes itself to its contents, so placing it by
        // its top-left left a sliver of picture showing beneath it whenever the
        // contents came out shorter than the rect reserved for them. Anchoring
        // the bottom edge makes it flush by construction, whatever it contains.
        egui::Area::new(egui::Id::new("transport"))
            .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_width(full.width());
                ui.set_opacity(opacity);

                egui::Frame::none()
                    .fill(with_alpha(Fluent::SOLID, 216))
                    .inner_margin(egui::Margin {
                        left: SPACE_M,
                        right: SPACE_M,
                        top: SPACE_S,
                        bottom: SPACE_S,
                    })
                    .show(ui, |ui| {
                        ui.set_width(full.width() - SPACE_M * 2.0);
                        ui.spacing_mut().item_spacing.y = SPACE_XS;
                        self.scrub_bar(ui);
                        self.transport_row(ui);
                    });
            });
        let _ = ui;
    }

    /// The progress bar: where playback is inside the timeshift window.
    ///
    /// The window is the DVR's, not this client's. Its left edge is the moment
    /// the channel was tuned and its right edge is the live edge, and both move
    /// as the session goes on, so the bar is drawn from the seek range the
    /// pipeline reports rather than from a fixed duration.
    fn scrub_bar(&mut self, ui: &mut egui::Ui) {
        let range = self.player.as_ref().and_then(|p| p.seek_range());
        let live_position = self.player.as_ref().and_then(|p| p.position());

        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 20.0),
            egui::Sense::click_and_drag(),
        );

        // A gutter at each end for the times, with the track between them.
        //
        // On the bar rather than in the row of buttons underneath: they
        // describe the bar, and that row sheds controls as the window narrows
        // — where you are in a program is not something to shed.
        const GUTTER: f32 = 64.0;
        let inner = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + GUTTER, rect.min.y),
            egui::pos2(
                (rect.max.x - GUTTER).max(rect.min.x + GUTTER + 20.0),
                rect.max.y,
            ),
        );

        let track = egui::Rect::from_center_size(inner.center(), egui::vec2(inner.width(), 4.0));
        let painter = ui.painter();
        painter.rect_filled(track, 2.0, with_alpha(Fluent::TEXT_PRIMARY, 40));

        // Nothing to scrub through on a stream that cannot seek. The track is
        // left dead and the explanation moved to a tooltip: a line of text
        // floating in the middle of a four pixel bar reads as a rendering
        // fault rather than as an explanation.
        let Some((start, end)) = range else {
            response.on_hover_text("This source is live only and cannot be rewound");
            return;
        };

        let span = (end - start).max(1.0);
        let position = self.scrub_target.or(live_position).unwrap_or(start);
        let fraction = (((position - start) / span) as f32).clamp(0.0, 1.0);

        // Pointer handling first, so a drag reads this frame instead of next.
        let mut commit: Option<f64> = None;
        if response.drag_started() {
            self.scrubbing = true;
        }
        if self.scrubbing || response.clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let t = ((pointer.x - track.min.x) / track.width().max(1.0)).clamp(0.0, 1.0);
                self.scrub_target = Some(start + t as f64 * span);
            }
        }
        if response.drag_stopped() || response.clicked() {
            commit = self.scrub_target.take();
            self.scrubbing = false;
        }

        let filled = egui::Rect::from_min_max(
            track.min,
            egui::pos2(track.min.x + track.width() * fraction, track.max.y),
        );
        ui.painter().rect_filled(filled, 2.0, Fluent::ACCENT);

        // Commercial breaks, marked on the bar itself so scrubbing past them
        // needs no guesswork. Amber, not red: they are information, not a
        // warning.
        //
        // The marker times are measured from the start of the recording, but
        // `start` here is the stream's own origin — which for these recordings
        // is around 81,876 seconds, not zero. Subtracting it turned every
        // break negative, clamped them all to the left edge, and made them
        // invisible. They are offsets into the span, not absolute positions.
        for (break_start, break_end) in &self.commercials {
            let x0 = track.min.x + track.width() * ((break_start / span) as f32).clamp(0.0, 1.0);
            let x1 = track.min.x + track.width() * ((break_end / span) as f32).clamp(0.0, 1.0);
            if x1 - x0 >= 1.0 {
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x0, track.min.y),
                        egui::pos2(x1, track.max.y),
                    ),
                    2.0,
                    with_alpha(Fluent::CAUTION, 150),
                );
            }
        }

        // Fluent shows the handle on hover and while dragging, not at rest, so
        // the bar stays a thin line until it is being used.
        if response.hovered() || self.scrubbing {
            let knob = egui::pos2(filled.max.x, track.center().y);
            ui.painter().circle_filled(knob, 7.0, Fluent::TEXT_PRIMARY);
            ui.painter().circle_filled(knob, 4.0, Fluent::ACCENT);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        // Left: how far in. Right: how much there is, or how far behind live.
        //
        // The left one follows the handle while scrubbing rather than the
        // picture, because the question being asked mid-drag is "where am I
        // about to land", not "where am I now" — `position` already prefers
        // the scrub target for exactly that reason.
        let behind = self.player.as_ref().and_then(|p| p.behind_live());
        let trailing = if self.live_channel.is_some() {
            match behind {
                Some(behind) if behind > 8.0 => format!("-{}", clock(behind)),
                _ => "LIVE".to_string(),
            }
        } else {
            clock(span)
        };
        let trailing_tint = if trailing == "LIVE" {
            Fluent::LIVE
        } else {
            Fluent::TEXT_SECONDARY
        };

        ui.painter().text(
            egui::pos2(inner.min.x - SPACE_S, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            clock((position - start).max(0.0)),
            egui::FontId::proportional(11.5),
            Fluent::TEXT_SECONDARY,
        );
        ui.painter().text(
            egui::pos2(inner.max.x + SPACE_S, rect.center().y),
            egui::Align2::LEFT_CENTER,
            trailing,
            egui::FontId::proportional(11.5),
            trailing_tint,
        );

        if let Some(target) = commit {
            if let Some(player) = &self.player {
                player.seek_to(target);
            }
        }
    }

    fn transport_row(&mut self, ui: &mut egui::Ui) {
        let seekable = self.player.as_ref().map(|p| p.is_seekable()).unwrap_or(false);
        let behind = self.player.as_ref().and_then(|p| p.behind_live());

        // What fits.
        //
        // Every control here used to be drawn unconditionally, so a narrow
        // window laid the same eight of them out past its own edge and they
        // simply overlapped: the LIVE pill on top of the quality button, the
        // quality button on top of the volume slider. Nothing in an
        // immediate-mode row negotiates for space, so the row has to decide.
        //
        // Dropped in the order of how little is lost by dropping each — the
        // volume slider first, because the keyboard and the system mixer both
        // still work without it. Play, the live pill and the way out are never
        // dropped, whatever the width.
        let width = ui.available_width();
        let show_volume = width > 860.0;
        let show_quality = width > 700.0;
        // Recording is a live action. A recording already exists on the server,
        // so offering to record it is either meaningless or a way to schedule
        // something the viewer did not ask for; the button was gated on window
        // width alone and appeared over every recording in the library.
        let show_record = width > 520.0 && self.live_channel.is_some();
        let show_skips = width > 440.0;

        ui.horizontal(|ui| {
            let play_glyph = if self.paused { theme::icon::PLAY } else { theme::icon::PAUSE };
            if subtle_button(ui, play_glyph, 40.0, false).clicked() {
                self.paused = !self.paused;
                if let Some(p) = &self.player {
                    p.set_paused(self.paused);
                }
            }

            let back = self.settings.skip_back_secs as f64;
            let forward = self.settings.skip_forward_secs as f64;
            if show_skips {
                if subtle_button(ui, theme::icon::SKIP_BACK, 40.0, false)
                    .on_hover_text(format!("Back {back:.0} seconds"))
                    .clicked()
                {
                    if let Some(p) = &self.player {
                        if !p.seek_by(-back) {
                            self.announce("This source cannot be rewound".into());
                        }
                    }
                }
                if subtle_button(ui, theme::icon::SKIP_FORWARD, 40.0, false)
                    .on_hover_text(format!("Forward {forward:.0} seconds"))
                    .clicked()
                {
                    if let Some(p) = &self.player {
                        p.seek_by(forward);
                    }
                }
            }

            // Recording the channel being watched is a first-class action, so
            // it sits in the bar rather than behind the overflow menu.
            if show_record {
                let recording = self.job_id.is_some();
                let record = subtle_button(ui, theme::icon::RECORD, 40.0, recording);
                let record = if recording {
                    record.on_hover_text("Stop recording this program")
                } else {
                    record.on_hover_text("Record this program")
                };
                if record.clicked() {
                    let ctx = ui.ctx().clone();
                    self.toggle_record(&ctx);
                }
            }

            ui.add_space(SPACE_S);

            // Live controls belong to live. A recording has no live edge to be
            // behind and nothing to jump back to, so a LIVE pill on one is
            // just wrong.
            if self.live_channel.is_some() {
                // Red at the live edge because that is where the picture
                // already is, green when behind because the button then has
                // somewhere to take you. Clicking returns to live.
                if live_pill(ui, behind, seekable).clicked() {
                    if let Some(p) = &self.player {
                        p.seek_to_live();
                    }
                }
            }

            // Skip-commercial, in the transport row where controls belong.
            // The floating pill over the picture appears only while a break is
            // actually playing; this one is always present on a recording that
            // has markers, so the feature is discoverable rather than
            // something you have to be mid-advert to find out about.
            if !self.commercials.is_empty() {
                ui.add_space(SPACE_XS);
                let inside = self.current_break().is_some();
                let button = subtle_button(ui, theme::icon::SKIP_BREAK, 40.0, inside);
                let button = if inside {
                    button.on_hover_text("Skip this commercial break")
                } else {
                    button.on_hover_text("Skip to the end of the next break")
                };
                if button.clicked() {
                    self.skip_break();
                }
            }

            // The clock used to be here as well. It is on the scrub bar now,
            // at the ends of the thing it describes, where it does not compete
            // with the buttons for room on a narrow window.

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Leaving the player is a back arrow, not an X.
                //
                // An X here was mistaken for the window's close button — which
                // is also an X, also on the right edge of the same window —
                // and people quit the program when they meant to stop
                // watching. An arrow cannot be confused with it.
                if subtle_button(ui, theme::icon::BACK, 40.0, false)
                    .on_hover_text("Stop watching (Esc)")
                    .clicked()
                {
                    self.stop_playback();
                    return;
                }
                ui.add_space(SPACE_XS);

                if subtle_button(ui, theme::icon::MORE, 40.0, self.show_stats).clicked() {
                    self.show_stats = !self.show_stats;
                }


                // Captions, offered only where they exist. Broadcast carries
                // them and imported files generally do not, and a permanently
                // dead CC button would say nothing except that it is broken.
                let cc = self
                    .player
                    .as_ref()
                    .map(|p| (p.captions_available(), p.captions_on()));
                if let Some((true, on)) = cc {
                    ui.add_space(SPACE_XS);
                    if subtle_text_button(ui, "CC", on)
                        .on_hover_text(if on {
                            "Turn closed captions off"
                        } else {
                            "Turn closed captions on"
                        })
                        .clicked()
                    {
                        if let Some(player) = &self.player {
                            player.set_captions(!on);
                        }
                    }
                }

                // Quality, for anything streamed from the server — live or a
                // recording, both of which the DVR will transcode on request.
                // Hidden for a downloaded file, which is already whatever it
                // is and always the original.
                //
                // The button says what it opens, not what is selected. It used
                // to show an abbreviation of the current pick — "SRC" for the
                // untranscoded original — which read as an acronym nobody owed
                // an explanation for. The flyout already marks the selection,
                // and the hover text carries it for anyone who wants it
                // without opening the menu.
                if !self.playing_local && show_quality {
                    ui.add_space(SPACE_XS);
                    let current = self.effective_quality();
                    if subtle_text_button(ui, "Quality", self.show_quality)
                        .on_hover_text(format!("Stream quality — {}", current.label()))
                        .clicked()
                    {
                        self.show_quality = !self.show_quality;
                    }
                }

                // The slider is the first thing to go on a narrow window. The
                // mute button beside it stays, so there is always a way to
                // silence it without the keyboard.
                if show_volume {
                    ui.add_space(SPACE_XS);
                    let mut volume = self.volume;
                    if ui
                        .add_sized(
                            [96.0, 18.0],
                            egui::Slider::new(&mut volume, 0.0..=1.0).show_value(false),
                        )
                        .changed()
                    {
                        self.volume = volume;
                        if let Some(p) = &self.player {
                            p.set_volume(volume as f64);
                        }
                    }
                }
                let vol_glyph = if self.volume <= 0.001 {
                    theme::icon::MUTE
                } else {
                    theme::icon::VOLUME
                };
                if subtle_button(ui, vol_glyph, 36.0, false).clicked() {
                    self.volume = if self.volume <= 0.001 { 1.0 } else { 0.0 };
                    if let Some(p) = &self.player {
                        p.set_volume(self.volume as f64);
                    }
                }
            });
        });
    }

    /// A short-lived message, for things the user asked for and the server
    /// answered: scheduled, canceled, or refused.
    fn toast_banner(&mut self, ui: &egui::Ui, area: egui::Rect) {
        let Some((text, shown)) = &self.toast else { return };
        let age = shown.elapsed().as_secs_f32();
        if age > 4.0 {
            self.toast = None;
            return;
        }

        // Slides down and fades in, holds, then fades away. A message that
        // pops into existence startles; one that arrives has somewhere to have
        // come from.
        let entrance = (age / 0.18).min(1.0);
        let entrance = entrance * entrance * (3.0 - 2.0 * entrance);
        let alpha = ((1.0 - ((age - 3.0) / 1.0).clamp(0.0, 1.0)) * entrance * 255.0) as u8;
        let slide = (1.0 - entrance) * -10.0;

        let galley = ui.painter().layout_no_wrap(
            text.clone(),
            egui::FontId::proportional(13.0),
            with_alpha(Fluent::TEXT_PRIMARY, alpha),
        );
        let size = galley.size() + egui::vec2(SPACE_L * 2.0, SPACE_M * 1.5);
        let card = egui::Rect::from_center_size(
            egui::pos2(area.center().x, area.min.y + SPACE_L + size.y / 2.0 + slide),
            size,
        );

        ui.painter().rect_filled(card, RADIUS_SURFACE, with_alpha(Fluent::SOLID, alpha.saturating_sub(20)));
        ui.painter().rect_stroke(
            card,
            RADIUS_SURFACE,
            egui::Stroke::new(1.0, with_alpha(egui::Color32::WHITE, alpha / 10)),
        );
        ui.painter().galley(
            egui::pos2(card.center().x - galley.size().x / 2.0, card.center().y - galley.size().y / 2.0),
            galley,
            Fluent::TEXT_PRIMARY,
        );

        ui.ctx().request_repaint();
    }

    /// Stream stats, as a Fluent card.
    fn stats_card(&self, ui: &egui::Ui, area: egui::Rect) {
        let card = egui::Rect::from_min_size(
            area.min + egui::vec2(SPACE_L, SPACE_L),
            egui::vec2(276.0, 312.0),
        );
        let painter = ui.painter();
        painter.rect_filled(card, RADIUS_SURFACE, Fluent::LAYER_CARD);
        painter.rect_stroke(card, RADIUS_SURFACE, egui::Stroke::new(1.0, Fluent::STROKE_SURFACE));

        let mut y = card.min.y + SPACE_M;
        painter.text(
            egui::pos2(card.min.x + SPACE_M, y),
            egui::Align2::LEFT_TOP,
            "Stream",
            egui::FontId::proportional(12.0),
            Fluent::TEXT_TERTIARY,
        );
        y += 22.0;

        let mut row = |label: &str, value: String, tint: egui::Color32| {
            painter.text(
                egui::pos2(card.min.x + SPACE_M, y),
                egui::Align2::LEFT_TOP,
                label,
                egui::FontId::proportional(13.0),
                Fluent::TEXT_SECONDARY,
            );
            painter.text(
                egui::pos2(card.max.x - SPACE_M, y),
                egui::Align2::RIGHT_TOP,
                value,
                egui::FontId::proportional(13.0),
                tint,
            );
            y += 21.0;
        };

        let health = if self.ui_fps() >= 50.0 { Fluent::SUCCESS } else { Fluent::CAUTION };
        row("Display", format!("{:.0} fps", self.ui_fps()), health);
        row("Decode", format!("{:.0} fps", self.decode_fps), Fluent::TEXT_PRIMARY);

        // Frames decoded but never shown, because the clock had already passed
        // them. A number that climbs steadily is the honest signal that
        // playback is not keeping up.
        let dropped = self.player.as_ref().map(|p| p.dropped()).unwrap_or(0);
        row(
            "Dropped",
            dropped.to_string(),
            if dropped == 0 { Fluent::SUCCESS } else { Fluent::CAUTION },
        );

        // Frames the decoder abandoned before output. A different fault from
        // the row above, with a different cause: too slow to decode at all,
        // rather than too slow to present on time.
        let behind = self.player.as_ref().map(|p| p.decoder_dropped()).unwrap_or(0);
        row(
            "Decoder drops",
            behind.to_string(),
            if behind == 0 { Fluent::SUCCESS } else { Fluent::LIVE },
        );

        // The cost of the software renderer, as a share of one core.
        //
        // Not milliseconds per frame. That row used to read 16.6ms on every
        // machine and every stream, which looked like a player scraping past
        // its 16.7ms budget and was nothing of the sort: the render call
        // returns when the frame is *due*, so a clock around it measures the
        // frame interval and never the work. This is processor time over
        // elapsed time, which is the actual cost.
        //
        // It falls with the size of the window, because that is the size being
        // rendered at — a 1080p stream in a small window is genuinely less
        // work, not the same work drawn smaller. Measured on one 1080p60
        // recording: 0.82 of a core at full size, 0.70 at 1280x720.
        let load = self.player.as_ref().map(|p| p.render_load()).unwrap_or(0.0);
        row(
            "Render load",
            format!("{:.0}% of a core", load * 100.0),
            if load < 0.6 {
                Fluent::SUCCESS
            } else if load < 0.95 {
                Fluent::CAUTION
            } else {
                Fluent::LIVE
            },
        );

        // How far the picture is from the sound. The one number that says
        // whether playback is actually correct rather than merely running.
        let avsync = self.player.as_ref().map(|p| p.avsync()).unwrap_or(0.0);
        row(
            "A/V sync",
            format!("{:+.0} ms", avsync * 1000.0),
            if avsync.abs() < 0.040 { Fluent::SUCCESS } else { Fluent::CAUTION },
        );

        let buffered = self.player.as_ref().map(|p| p.buffered()).unwrap_or(0.0);
        row(
            "Buffered",
            if buffered >= 1.0 {
                format!("{buffered:.1} s")
            } else {
                format!("{:.0} ms", buffered * 1000.0)
            },
            if buffered > 0.5 { Fluent::SUCCESS } else { Fluent::CAUTION },
        );
        // The stream's size, then what is actually being converted, which is
        // smaller whenever the window is. Both, because one without the other
        // reads as a fault: "1280x720" alone looks like the wrong stream, and
        // "1920x1080" alone hides where the render time went.
        let stream = self.player.as_ref().map(|p| p.video_size()).unwrap_or((0, 0));
        row(
            "Resolution",
            if stream.0 > 0 {
                format!("{}x{}", stream.0, stream.1)
            } else {
                "—".into()
            },
            Fluent::TEXT_PRIMARY,
        );
        // The raw timeline numbers, not a tidied summary. A live HLS playlist
        // is where seeking goes wrong, and it goes wrong by the pipeline
        // reporting a position or a range that does not mean what it looks
        // like, so the three values have to be visible separately to tell
        // which one is at fault.
        let player = self.player.as_ref();
        row(
            "Position",
            player
                .and_then(|p| p.position())
                .map(clock)
                .unwrap_or_else(|| "—".into()),
            Fluent::TEXT_PRIMARY,
        );
        row(
            "Seek range",
            player
                .and_then(|p| p.seek_range())
                .map(|(s, e)| format!("{} → {}", clock(s), clock(e)))
                .unwrap_or_else(|| "not seekable".into()),
            Fluent::TEXT_PRIMARY,
        );
        row(
            "Duration",
            player
                .and_then(|p| p.duration())
                .map(clock)
                .unwrap_or_else(|| "—".into()),
            Fluent::TEXT_PRIMARY,
        );
        row(
            "Source",
            self.transport.map(|t| t.label().to_string()).unwrap_or_else(|| "—".into()),
            Fluent::TEXT_PRIMARY,
        );
    }
}

/// Drag-to-resize handles along the window's edges and corners.
///
/// An undecorated window has no system frame, and the frame is what Windows
/// hit-tests for resizing — so `with_decorations(false)` silently made the
/// window fixed-size no matter what `resizable` said. There is nothing to grab
/// because the operating system is no longer providing anything to grab, and
/// the only way back is to hit-test the edges ourselves and ask the backend to
/// take over the drag.
///
/// Nothing is painted. These are eight invisible strips whose whole job is to
/// set the right cursor and hand the drag to the window manager, which then
/// does the actual resizing at the compositor's frame rate rather than ours.
fn resize_borders(ui: &mut egui::Ui, ctx: &egui::Context, full: egui::Rect, fullscreen: bool) {
    use egui::viewport::ResizeDirection as Dir;
    use egui::CursorIcon as Cursor;

    // A native frame resizes itself; a second set of grab zones inside the
    // window would only fight the system's own.
    if platform::NATIVE_FRAME {
        return;
    }

    // A maximized or full-screen window has no edges to drag, and offering
    // them would resize it out of its own maximized state on a stray click.
    if fullscreen || ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
        return;
    }

    // Wide enough to hit without aiming, narrow enough not to steal clicks
    // from the controls that sit near the edges. The corners are larger,
    // because a corner is what people reach for and it is the smaller target.
    const EDGE: f32 = 6.0;
    const CORNER: f32 = 14.0;

    let (l, r, t, b) = (full.min.x, full.max.x, full.min.y, full.max.y);
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32| {
        egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1))
    };

    // Corners first: they overlap the edges, and whichever is registered last
    // wins the hit test, so the edges must come after to lose it.
    let zones = [
        (rect(l, t, l + CORNER, t + CORNER), Dir::NorthWest, Cursor::ResizeNwSe),
        (rect(r - CORNER, t, r, t + CORNER), Dir::NorthEast, Cursor::ResizeNeSw),
        (rect(l, b - CORNER, l + CORNER, b), Dir::SouthWest, Cursor::ResizeNeSw),
        (rect(r - CORNER, b - CORNER, r, b), Dir::SouthEast, Cursor::ResizeNwSe),
        (rect(l, t, r, t + EDGE), Dir::North, Cursor::ResizeVertical),
        (rect(l, b - EDGE, r, b), Dir::South, Cursor::ResizeVertical),
        (rect(l, t, l + EDGE, b), Dir::West, Cursor::ResizeHorizontal),
        (rect(r - EDGE, t, r, b), Dir::East, Cursor::ResizeHorizontal),
    ];

    // Registered in reverse so the corners, listed first, end up on top.
    for (zone, direction, cursor) in zones.into_iter().rev() {
        let response = ui.interact(
            zone,
            egui::Id::new(("resize", direction as u8)),
            egui::Sense::drag(),
        );
        if response.hovered() || response.dragged() {
            ctx.set_cursor_icon(cursor);
        }
        if response.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
    }
}

/// The custom caption. Undecorated windows have to provide their own, which is
/// what lets the material run edge to edge instead of stopping below a system
/// title bar.
fn title_bar(ui: &mut egui::Ui, ctx: &egui::Context, rect: egui::Rect, online: bool) {
    // Past the traffic lights, where a platform has put some. On the others
    // the inset is zero and this is the same left margin it always was.
    let left = rect.min.x + SPACE_L + platform::CAPTION_INSET;
    let painter = ui.painter();
    painter.text(
        egui::pos2(left, rect.center().y),
        egui::Align2::LEFT_CENTER,
        APP_NAME,
        egui::FontId::proportional(13.0),
        Fluent::TEXT_SECONDARY,
    );

    // Say so when the server has gone.
    //
    // Everything on screen was fetched while it was still there, so without
    // this an outage looks exactly like an application that simply stopped
    // being interesting — which is how one ends up being relaunched instead of
    // waited for.
    if !online {
        let at = egui::pos2(left + 68.0, rect.center().y);
        painter.circle_filled(egui::pos2(at.x + 5.0, at.y), 4.0, Fluent::LIVE);
        painter.text(
            egui::pos2(at.x + 16.0, at.y),
            egui::Align2::LEFT_CENTER,
            "DVR unreachable — reconnecting",
            egui::FontId::proportional(11.5),
            Fluent::LIVE,
        );
    }

    // Dragging anywhere in the caption moves the window, as Fluent expects.
    let drag = ui.interact(rect, egui::Id::new("caption"), egui::Sense::click_and_drag());
    if drag.is_pointer_button_down_on() {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }

    // With a native frame the system provides the buttons, and drawing a
    // second set — in the wrong corner, in another platform's order — is the
    // quickest way to look like a port. Everything above still applies; only
    // the buttons are the system's.
    if platform::NATIVE_FRAME {
        return;
    }

    // Caption buttons, laid out right to left in the Windows order.
    let mut x = rect.max.x;
    let size = egui::vec2(46.0, rect.height());

    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    for (glyph, action) in [
        (theme::icon::CLOSE, CaptionAction::Close),
        (
            if maximized { theme::icon::RESTORE } else { theme::icon::MAXIMIZE },
            CaptionAction::Maximize,
        ),
        (theme::icon::MINIMIZE, CaptionAction::Minimize),
    ] {
        x -= size.x;
        let button = egui::Rect::from_min_size(egui::pos2(x, rect.min.y), size);
        let response = ui.interact(
            button,
            egui::Id::new(("caption", glyph)),
            egui::Sense::click(),
        );

        if response.hovered() {
            // Close goes red on hover, as every Windows app does.
            let tint = if matches!(action, CaptionAction::Close) {
                egui::Color32::from_rgb(196, 43, 28)
            } else {
                Fluent::CONTROL_HOVER
            };
            ui.painter().rect_filled(button, 0.0, tint);
        }

        ui.painter().text(
            button.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            egui::FontId::new(10.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
            Fluent::TEXT_PRIMARY,
        );

        if response.clicked() {
            match action {
                CaptionAction::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                CaptionAction::Minimize => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true))
                }
                CaptionAction::Maximize => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
            }
        }
    }
}

enum CaptionAction {
    Close,
    Minimize,
    Maximize,
}

/// Letterbox the frame into the available space, preserving aspect ratio so the
/// window can be dragged to any proportion without distorting the picture.
///
/// `entrance` is the arrival animation, 0 to 1: the picture rises the last few
/// pixels into place while fading up from black.
/// The OpenGL entry points, resolved once the context exists.
///
/// Not at startup: there is no context until eframe has made a window, and
/// asking earlier gets null for everything.
fn gl_fns() -> Option<&'static mpv::GlFns> {
    static FNS: std::sync::OnceLock<Option<mpv::GlFns>> = std::sync::OnceLock::new();
    FNS.get_or_init(|| match unsafe { mpv::GlFns::load() } {
        Ok(fns) => Some(fns),
        Err(e) => {
            log::logline!("[clicker] {e}");
            None
        }
    })
    .as_ref()
}

/// What is being waited for, so the spinner can say so.
///
/// "Tuning channel" is true of live TV, where the DVR really is driving a
/// tuner and it really does take several seconds. It is nonsense in front of a
/// recording, which is a file on a disk: nothing is being tuned, and telling
/// someone otherwise makes the app look like it does not know what it is doing.
#[derive(Clone)]
struct Loading {
    title: String,
    detail: String,
}

impl Loading {
    fn live(channel: &str) -> Self {
        Self {
            title: "Tuning channel".into(),
            detail: format!("Waiting for the DVR to start channel {channel}"),
        }
    }

    fn recording(title: &str) -> Self {
        Self {
            title: "Opening recording".into(),
            detail: title.to_string(),
        }
    }
}

/// A Fluent progress ring, shown while the source is opening.
///
/// A cold tune on an ah4c source takes several seconds before a single frame
/// exists, and a static line of text through that reads as a hang rather than
/// as work in progress.
fn tuning_indicator(ui: &egui::Ui, area: egui::Rect, what: &Loading) {
    let painter = ui.painter();
    let center = egui::pos2(area.center().x, area.center().y - 18.0);
    let radius = 21.0;

    // Continuous rotation, driven by wall time rather than frame count so the
    // speed does not change with the refresh rate.
    let t = ui.input(|i| i.time) as f32;
    let spin = t * 2.4;

    // The faint track the arc travels along.
    painter.circle_stroke(
        center,
        radius,
        egui::Stroke::new(3.0, with_alpha(Fluent::TEXT_PRIMARY, 28)),
    );

    // Fluent's ring is an arc of roughly a third of the circle, easing as it
    // goes rather than sweeping at a constant rate.
    let sweep = std::f32::consts::TAU * 0.34;
    let start = spin % std::f32::consts::TAU;
    let segments = 48;
    let points: Vec<egui::Pos2> = (0..=segments)
        .map(|i| {
            let angle = start + sweep * (i as f32 / segments as f32);
            egui::pos2(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(3.0, Fluent::ACCENT),
    ));

    painter.text(
        egui::pos2(center.x, center.y + radius + 26.0),
        egui::Align2::CENTER_CENTER,
        &what.title,
        egui::FontId::proportional(14.0),
        Fluent::TEXT_PRIMARY,
    );
    painter.text(
        egui::pos2(center.x, center.y + radius + 47.0),
        egui::Align2::CENTER_CENTER,
        &what.detail,
        egui::FontId::proportional(12.0),
        Fluent::TEXT_TERTIARY,
    );

    // Keep animating: no frames are arriving yet, so nothing else asks for a
    // repaint.
    ui.ctx().request_repaint();
}

/// Fluent's Subtle button: nothing at rest, a soft fill on hover, a firmer one
/// while pressed. No outline — an always-visible border is what made these look
/// like web buttons rather than Windows ones.
fn subtle_button(ui: &mut egui::Ui, glyph: &str, width: f32, active: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, 36.0), egui::Sense::click());

    let fill = if response.is_pointer_button_down_on() {
        Fluent::CONTROL_PRESSED
    } else if response.hovered() {
        Fluent::CONTROL_HOVER
    } else if active {
        Fluent::CONTROL
    } else {
        egui::Color32::TRANSPARENT
    };

    if fill != egui::Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, theme::RADIUS_CONTROL, fill);
    }

    // The accent only appears on the active state, so it always means "this is
    // on" rather than merely decorating the control.
    let tint = if active { Fluent::ACCENT } else { Fluent::TEXT_PRIMARY };

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::new(15.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
        tint,
    );

    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// A Fluent Subtle button carrying a short text label instead of a glyph —
/// the quality badge. Same states and geometry as `subtle_button`, so it sits
/// in the row as a sibling rather than a stranger.
fn subtle_text_button(ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
    let width = (14.0 + label.chars().count() as f32 * 7.5).max(44.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, 36.0), egui::Sense::click());

    let fill = if response.is_pointer_button_down_on() {
        Fluent::CONTROL_PRESSED
    } else if response.hovered() {
        Fluent::CONTROL_HOVER
    } else if active {
        Fluent::CONTROL
    } else {
        egui::Color32::TRANSPARENT
    };
    if fill != egui::Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, theme::RADIUS_CONTROL, fill);
    }

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(12.0),
        if active { Fluent::ACCENT } else { Fluent::TEXT_SECONDARY },
    );

    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// The live control.
///
/// Red means the picture is the live edge. Green means playback is behind it
/// and the button will take you back, so the color states what pressing it
/// would do rather than merely labelling the stream.
fn live_pill(ui: &mut egui::Ui, behind: Option<f64>, seekable: bool) -> egui::Response {
    let at_live = behind.map(|b| b <= 8.0).unwrap_or(true);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(62.0, 24.0),
        if seekable && !at_live {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );

    let tint = if at_live { Fluent::LIVE } else { Fluent::SUCCESS };
    let wash = if response.hovered() && !at_live { 78 } else { 46 };

    let painter = ui.painter();
    painter.rect_filled(rect, 12.0, with_alpha(tint, wash));
    painter.circle_filled(egui::pos2(rect.min.x + 13.0, rect.center().y), 3.5, tint);
    painter.text(
        egui::pos2(rect.min.x + 23.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "LIVE",
        egui::FontId::proportional(11.0),
        tint,
    );

    if response.hovered() && !at_live {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if !at_live {
        return response.on_hover_text("Jump to live");
    }
    response
}

/// Seconds as `m:ss`, or `h:mm:ss` once there is an hour of it.
fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn with_alpha(color: egui::Color32, alpha: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

