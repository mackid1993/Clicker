//! The video player.
//!
//! FFmpeg decodes; everything else is here. The structure is the one ffplay
//! settled on, for the reason ffplay settled on it: a single thread owns the
//! demuxer and both decoders (an `AVFormatContext` is not thread safe), decoded
//! frames go into bounded queues, and **audio is the master clock**. Video is
//! presented against that clock rather than on a timer.
//!
//! That last point is the whole design. A player that shows each frame as it is
//! decoded looks correct for a minute and then drifts, because the decoder does
//! not run at exactly the frame rate and nothing ever pulls it back. Audio
//! cannot drift without being audible, so the sound card's consumption rate is
//! the only honest clock in the system, and the picture is scheduled to it.
//!
//! Three threads:
//!   * **decode** — pumps the shim, fills the queues, handles seeks
//!   * **audio** — cpal's callback, drains audio and advances the clock
//!   * **present** — waits until a frame is due, publishes it, asks for a repaint

mod audio;
mod clock;
mod ffi;

use std::collections::VecDeque;
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};

pub use clock::Clock;

/// What kind of source this is.
///
/// A label for the interface, not a decision the player acts on. Whether a
/// stream can actually be rewound is answered by the demuxer once it is open,
/// because a URL cannot be trusted to say: plenty of `.m3u8` playlists are
/// sliding windows that cannot seek backwards at all, and plenty of files with
/// no extension seek perfectly.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transport {
    /// One long HTTP response, such as Channels' `stream.mpg`. Lowest latency
    /// and no transcode, but there is nothing to seek within.
    Direct,
    /// A segmented playlist. Channels keeps every segment from the moment the
    /// channel was tuned, verified against the server: `EXT-X-MEDIA-SEQUENCE`
    /// stays at 1 while the list grows, so the whole session stays
    /// addressable. Other servers roll segments off the front, which is why
    /// the seekable window is read from the demuxer rather than assumed.
    Hls,
    /// A recording or any other addressable file, local or over HTTP.
    File,
    /// A live stream being written to a local file as it arrives.
    ///
    /// Seekable like any file, but catching up with the writer looks exactly
    /// like the end of it — so end-of-file here means "wait", not "stop".
    Timeshift,
}

impl Transport {
    pub fn of(uri: &str) -> Self {
        let lower = uri.to_ascii_lowercase();
        let path = lower.split(['?', '#']).next().unwrap_or(&lower);
        if path.ends_with(".m3u8") || path.ends_with(".m3u") || path.contains("/hls/") {
            Transport::Hls
        } else if path.ends_with(".mpg") || path.ends_with(".ts") || path.contains("stream.mpg") {
            Transport::Direct
        } else {
            Transport::File
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Transport::Direct => "Direct",
            Transport::Hls => "HLS",
            Transport::File => "File",
            Transport::Timeshift => "Direct + buffer",
        }
    }
}

/// Where to join a live playlist.
///
/// Only meaningful for HLS; every other demuxer ignores it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JoinAt {
    /// The head of the playlist. Right for a fresh tune: Channels' playlist
    /// begins at the moment of tuning, so the head is a few seconds back and
    /// those seconds are a buffer held on the server rather than in memory.
    Start,
    /// Near the live edge. Right for re-opening a channel already playing —
    /// changing quality — where the playlist has been accumulating for as long
    /// as the channel has been on and its head is no longer anywhere near now.
    LiveEdge,
}

impl JoinAt {
    fn live_start_index(self) -> std::os::raw::c_int {
        match self {
            JoinAt::Start => 0,
            // Four segments back, not the last one. The final segment is the
            // one the server is still writing, and reads of it block until it
            // is finished; a few segments of margin is what keeps the fetch
            // hitting files that are already complete.
            JoinAt::LiveEdge => -4,
        }
    }
}

/// A decoded frame waiting to be uploaded.
#[derive(Default)]
pub struct FrameSlot {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    /// Bumped on every new frame so the UI knows whether to re-upload.
    pub generation: u64,
}

struct VideoFrame {
    pts: f64,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

/// Interleaved f32, already resampled to the output device's rate.
struct AudioChunk {
    pts: f64,
    samples: Vec<f32>,
    consumed: usize,
}

/// Facts about the stream, refreshed by the decode thread.
#[derive(Default, Clone)]
struct StreamFacts {
    /// Seconds. For a growing HLS playlist this is the live edge, and it moves.
    duration: f64,
    seekable: bool,
    has_audio: bool,
}

enum Command {
    Seek(f64),
    Quit,
}

/// Queue limits. Deep enough to ride out a slow segment fetch, shallow enough
/// that a seek does not have to throw away seconds of work.
///
/// Twelve frames is 200ms at 59.94fps, and 100MB at 1080p — which is why it is
/// not larger. With the pre-roll below in place the worst measured read is
/// 54ms, so this is around four times the margin actually needed, and the
/// buffer that does the real work on a live stream is not this one anyway; see
/// `LIVE_PREROLL`.
///
/// It also sets how much audio can be held: video and audio are interleaved in
/// the transport stream and decoded by the same thread, so filling this queue
/// stops audio being decoded too. 200ms of video is 200ms of sound in hand,
/// which is an order of magnitude more than the device's own buffer.
const MAX_VIDEO_FRAMES: usize = 12;
const MAX_AUDIO_SECONDS: f64 = 1.0;

/// How many frame buffers to keep for recycling.
///
/// Only needs to cover the difference between what the decoder is filling and
/// what the presenter is releasing, which in a steady state is one or two. A
/// pool the size of the queue would double the memory to hold buffers that are
/// never reached: after a seek drains twelve frames at once the surplus is
/// simply freed, which is the right thing to do with 100MB.
const MAX_SPARE_BUFFERS: usize = 4;

/// How far behind the live edge to sit on a segmented live stream.
///
/// A live HLS player has one genuinely hard problem: the newest segment is
/// still being written. Ask for it and the server answers at the rate the
/// broadcast arrives, so `av_read_frame` blocks for the remainder of that
/// segment — measured on this stream at up to 1.1 seconds, once per two-second
/// segment, for as long as playback continued. Nothing downstream can survive
/// that. A quarter-second frame queue drains in a fifth of the time, so the
/// picture froze, and the audio queue, which is filled from the same thread by
/// the same reads, sat empty and underran on hundreds of callbacks a minute.
/// That is the stutter, and it is not a decode fault: the decode thread was
/// measured at 32% busy throughout, and colour conversion at 3%.
///
/// The fix is not a bigger buffer here. Holding a second of 1080p frames costs
/// half a gigabyte, and it would only postpone the problem, because a player
/// pinned to the live edge consumes exactly as fast as the server produces and
/// so can never get ahead. The fix is to stand further back. A Channels session
/// keeps every segment it has ever cut, so segments a few seconds old are
/// complete files on a LAN: they arrive in tens of milliseconds instead of
/// blocking for a second. The buffer lives on the server, costs this process
/// nothing, and is bounded by the one number below.
///
/// It is bought by not reading for that long once, at startup, while the
/// "tuning" indicator is already showing, and then kept for the rest of the
/// session for free: the demuxer and the broadcast both advance at 1x, so the
/// gap neither grows nor closes.
///
/// Four seconds, which is two segments on this server. The floor is set by
/// segment length, not by taste: one segment behind is the newest *complete*
/// file, and anything less asks for the one still being written, which is the
/// entire fault above. Two segments leaves a full segment of margin over the
/// 1.1s worst case actually measured, so the reads stay in the tens of
/// milliseconds. Going to one segment would trade every bit of that margin for
/// two seconds of startup, on top of a server tune that already takes nearly
/// nine — a bad bargain.
const LIVE_PREROLL: Duration = Duration::from_secs(4);

/// Frames queued before the clock starts, and the longest it will wait for
/// them.
///
/// Both small on purpose. The queue holds twelve frames, so this is a fifth of
/// a second of pictures, not seconds of them — the real protection against a
/// stalling read is having fallen behind the live edge, which is the join
/// point's job, not this one's. This exists so playback does not begin on a
/// single frame and immediately starve.
const PREROLL_FRAMES: usize = 10;
const PREROLL_MAX: Duration = Duration::from_millis(1200);

/// An open faster than this means the server already had the channel running.
///
/// Measured on a real DVR: 8.61s when it had to tune, 0.35s when the session
/// was already up. The two are an order of magnitude apart, so where exactly
/// the line falls between them does not matter much.
const WARM_OPEN: Duration = Duration::from_secs(2);

/// How long a caption stays up with nothing new arriving, in stream seconds.
///
/// A caption decoder does not announce that a programme has stopped talking,
/// it simply stops sending — so something has to decide when silence has gone
/// on long enough that the last line is stale rather than still being read.
/// Eight seconds is longer than any single pop-on caption and shorter than the
/// gap before an advert break.
const CAPTION_HOLD: f64 = 8.0;

struct Shared {
    frame: Mutex<FrameSlot>,
    decoded: AtomicU64,
    dropped: AtomicU64,

    video: Mutex<VecDeque<VideoFrame>>,
    audio: Mutex<VecDeque<AudioChunk>>,

    /// Buffers recycled between the decoder and the UI. At 1080p a frame is
    /// 8.3MB; allocating one per frame at 60fps would be half a gigabyte a
    /// second of pure allocator traffic.
    spare: Mutex<Vec<Vec<u8>>>,

    clock: Clock,
    facts: Mutex<StreamFacts>,
    error: Mutex<Option<String>>,

    paused: AtomicBool,
    quit: AtomicBool,
    /// Set by a seek, cleared by the presentation thread once it has re-anchored
    /// the clock on a frame that actually arrived. See `present_loop`.
    resync: AtomicBool,
    /// Set whenever the clock is anchored on a video frame, cleared once it has
    /// been snapped onto the audio that is actually audible. See `present_loop`.
    align_audio: AtomicBool,
    /// f32 bits. Read in the audio callback, which must not lock.
    volume: AtomicU32,

    /// The furthest point the stream has reached, as f64 bits. For a live
    /// playlist this is the live edge, and it advances for as long as the
    /// session runs.
    ///
    /// Tracked here rather than asked of the demuxer because a live HLS
    /// playlist has no duration to report: it carries no EXT-X-ENDLIST, so
    /// FFmpeg will not say where it ends. The furthest timestamp actually
    /// decoded is a direct measurement of the same thing.
    live_edge: AtomicU64,

    /// When `live_edge` was last advanced by an actual decoded frame, as
    /// microseconds since `opened_at`, and whether this source has a live edge
    /// that moves on its own. See `Shared::edge`.
    live_edge_at: AtomicU64,
    opened_at: Instant,
    live: bool,

    /// Samples currently queued for the device, across all channels.
    ///
    /// Kept as a counter rather than measured by walking the queue. Summing it
    /// meant holding the audio lock for O(queue) on every pass of the decode
    /// loop, contending with the real-time callback that has to take the same
    /// lock, which is audible.
    audio_buffered: AtomicUsize,

    /// Rolling average milliseconds per decoded frame, as f32 bits.
    decode_ms: AtomicU32,

    // --- instrumentation ---------------------------------------------------
    //
    // `decode_ms` above lumps demux, decode and colour conversion into one
    // figure, which is what the stats card shows and is useless for deciding
    // which of the three to fix. These separate them, and record the shape of
    // the failures rather than only their averages: this pipeline averaged 8ms
    // a frame while stalling for a second every two seconds, which reads as
    // perfectly healthy in a mean and stutters continuously in practice.
    /// EMA microseconds inside `rd_video_copy` (swscale YUV -> RGBA).
    convert_us: AtomicU32,
    /// How many `rd_next` calls took longer than one frame period. On a live
    /// stream this is where a segment fetch shows up, because the fetch happens
    /// inside that call, on the decode thread.
    stalls: AtomicU64,
    /// Times the presentation thread woke with the clock running and found no
    /// frame at all. This is starvation, and it is a different fault from
    /// dropping: nothing was thrown away, there was nothing to show.
    starved: AtomicU64,
    /// Audio callbacks that could not be filled from the queue.
    underruns: AtomicU64,
    /// Earliest PTS ever seen, as f64 bits. The stream's timeline does not
    /// start at zero, and pretending it does makes every position wrong.
    first_pts: AtomicU64,

    /// How much of a live buffer has been released back to the disk, 0 to 1.
    ///
    /// Those bytes read as zeros now, so the seekable window has to start
    /// after them — offering a seek into a hole would answer a scrub with
    /// silence and a black frame.
    discarded: AtomicU32,

    /// Whether captions have been seen in this stream at all.
    cc_available: AtomicBool,
    /// Whether they are switched on.
    cc_enabled: AtomicBool,
    /// The caption line currently on screen. Empty when there is none.
    caption: Mutex<String>,

    /// Which timestamp is coming out of the speakers, as f64 bits, published
    /// by the audio callback. NaN when nothing is playing. Read every two
    /// seconds to trim the clock; see `audio::fill` for why this is a reading
    /// and not a clock.
    audible_pts: AtomicU64,

    output_rate: f64,
    output_channels: usize,
}

impl Shared {
    fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    fn live_edge(&self) -> f64 {
        f64::from_bits(self.live_edge.load(Ordering::Relaxed))
    }

    /// Where the live edge is *now*, rather than where it was last seen.
    ///
    /// `live_edge` is the furthest timestamp actually decoded, and while
    /// playback is rewound nothing further is being decoded, so it stops
    /// moving. That breaks the one control that exists to undo a rewind:
    /// rewind ten minutes into a programme and "jump to live" would take you
    /// back to the moment you pressed rewind, because that is still the
    /// furthest frame the player has ever seen. The "-10:00" behind-live
    /// readout freezes with it.
    ///
    /// A broadcast does not stop while you are watching an earlier part of it.
    /// It advances at exactly one second per second, so the edge is wherever it
    /// was last observed plus however long ago that was. This is an
    /// extrapolation and it is labelled as one, but it is extrapolating the
    /// most reliable quantity in the system.
    ///
    /// Note that it tracks *this player's* live edge, which the startup
    /// pre-roll deliberately places a few seconds behind the broadcast's. That
    /// is the right target: returning to the broadcast's true edge would put
    /// playback back on the segment being written, which is the condition
    /// LIVE_PREROLL exists to avoid.
    fn edge(&self) -> f64 {
        let observed = self.live_edge();
        if !self.live || !observed.is_finite() {
            return observed;
        }
        let seen_at = self.live_edge_at.load(Ordering::Relaxed);
        let now = self.opened_at.elapsed().as_micros() as u64;
        observed + now.saturating_sub(seen_at) as f64 / 1_000_000.0
    }

    fn first_pts(&self) -> f64 {
        f64::from_bits(self.first_pts.load(Ordering::Relaxed))
    }

    /// The earliest timestamp seen. Only ever moves backwards, and only from
    /// its sentinel: an MPEG-TS timeline starts wherever the broadcaster's
    /// encoder happened to be, not at zero, so the bottom of the seekable
    /// window is a measurement rather than a constant.
    fn observe_first(&self, pts: f64) {
        if !pts.is_finite() || pts < 0.0 {
            return;
        }
        let mut current = self.first_pts.load(Ordering::Relaxed);
        loop {
            let seen = f64::from_bits(current);
            if seen.is_finite() && seen <= pts {
                return;
            }
            match self.first_pts.compare_exchange_weak(
                current,
                pts.to_bits(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(other) => current = other,
            }
        }
    }


    /// Only ever moves forward. A seek backwards decodes older timestamps
    /// again, and letting those pull the live edge back would shrink the
    /// timeshift window every time it was used.
    fn observe(&self, pts: f64) {
        if !pts.is_finite() || pts < 0.0 {
            return;
        }
        let mut current = self.live_edge.load(Ordering::Relaxed);
        loop {
            if f64::from_bits(current) >= pts {
                return;
            }
            match self.live_edge.compare_exchange_weak(
                current,
                pts.to_bits(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    // Stamp when this was seen, so `edge` can carry on from it
                    // once decoding has moved somewhere earlier in the stream.
                    self.live_edge_at
                        .store(self.opened_at.elapsed().as_micros() as u64, Ordering::Relaxed);
                    return;
                }
                Err(seen) => current = seen,
            }
        }
    }
}

pub struct Player {
    shared: Arc<Shared>,
    commands: Sender<Command>,
    pub transport: Transport,
    /// cpal's stream is not `Send`, so it stays with whoever built the player.
    /// Dropping it stops the device, so it has to be kept alive.
    _audio: Option<audio::Output>,
    threads: Vec<std::thread::JoinHandle<()>>,
    /// Raised to make FFmpeg abandon whatever network operation it is in.
    /// See `stop`.
    abort: Arc<AtomicI32>,
}

/// The shim's handle, and the abort flag FFmpeg holds a raw pointer to.
///
/// Not `Send` by default because of the raw pointer; it is only ever touched by
/// the decode thread, which is what makes that sound. The flag rides along so
/// that it is freed strictly after `rd_close`: FFmpeg reads it from inside the
/// format context, so outliving that context is not optional.
struct Media(*mut ffi::RdMedia, Arc<AtomicI32>);
unsafe impl Send for Media {}

impl Drop for Media {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { ffi::rd_close(self.0) };
            self.0 = std::ptr::null_mut();
        }
        // Only now may the flag go, and it does, when field 1 drops.
    }
}

impl Player {
    /// Open a stream and start playing it.
    ///
    /// `repaint` is called when a new frame has been published, so the UI
    /// redraws exactly as often as there is something new to show rather than
    /// spinning at the display's refresh rate.
    pub fn open(
        uri: &str,
        transport: Transport,
        repaint: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        Self::open_at(uri, transport, JoinAt::Start, repaint)
    }

    /// Open a stream, choosing where a live playlist is joined.
    ///
    /// The join point cannot be corrected afterwards. A live stream states no
    /// duration, so its seekable window is measured from the timestamps this
    /// player has actually decoded — which means for the first second there is
    /// no window at all and every seek is refused, and thereafter the window
    /// only grows at playback speed. Seeking to a live edge twenty minutes
    /// ahead would take twenty minutes to become possible.
    pub fn open_at(
        uri: &str,
        transport: Transport,
        join: JoinAt,
        repaint: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        // The audio device is chosen first because FFmpeg is told to resample
        // straight to its rate and channel count. Converting to some fixed
        // internal format and letting the device convert again would be two
        // resamples where one will do.
        let device = audio::Device::open()?;

        // Timed, because how long the server takes to answer is what says
        // whether it had to tune. See where `preroll` is decided.
        let opening = Instant::now();
        let url = CString::new(uri)?;
        // Falls back to a bare product string if the device name has a NUL in
        // it, which is not a reason to refuse to play anything.
        let agent = CString::new(crate::settings::user_agent())
            .unwrap_or_else(|_| CString::new("RustDVR").unwrap());
        let mut err = vec![0i8; 512];
        // Created before the open, because opening a live playlist is itself a
        // blocking network operation and dropping a player that is still
        // opening has to be able to cut it short.
        let abort = Arc::new(AtomicI32::new(0));
        let handle = unsafe {
            ffi::rd_open(
                url.as_ptr(),
                device.sample_rate as i32,
                device.channels as i32,
                join.live_start_index(),
                agent.as_ptr(),
                abort.as_ptr(),
                err.as_mut_ptr(),
                err.len() as i32,
            )
        };
        if handle.is_null() {
            let message = unsafe { std::ffi::CStr::from_ptr(err.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            return Err(anyhow!(if message.is_empty() {
                "could not open the stream".to_string()
            } else {
                message
            }));
        }
        let media = Media(handle, Arc::clone(&abort));

        let facts = StreamFacts {
            duration: unsafe { ffi::rd_duration(handle) },
            seekable: unsafe { ffi::rd_seekable(handle) } != 0,
            has_audio: unsafe { ffi::rd_has_audio(handle) } != 0,
        };

        let shared = Arc::new(Shared {
            frame: Mutex::new(FrameSlot::default()),
            decoded: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            video: Mutex::new(VecDeque::new()),
            audio: Mutex::new(VecDeque::new()),
            spare: Mutex::new(Vec::new()),
            clock: Clock::new(),
            facts: Mutex::new(facts.clone()),
            error: Mutex::new(None),
            paused: AtomicBool::new(false),
            quit: AtomicBool::new(false),
            resync: AtomicBool::new(false),
            align_audio: AtomicBool::new(false),
            volume: AtomicU32::new(1.0f32.to_bits()),
            live_edge: AtomicU64::new(0.0f64.to_bits()),
            live_edge_at: AtomicU64::new(0),
            opened_at: Instant::now(),
            // Only a growing playlist has a live edge that moves without this
            // player doing anything. A recording's "edge" is its last frame.
            // A growing local buffer has a live edge that moves on its own,
            // exactly as a playlist does.
            live: (transport == Transport::Hls && facts.duration <= 0.0)
                || transport == Transport::Timeshift,
            audio_buffered: AtomicUsize::new(0),
            decode_ms: AtomicU32::new(0.0f32.to_bits()),
            convert_us: AtomicU32::new(0),
            stalls: AtomicU64::new(0),
            starved: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            first_pts: AtomicU64::new(f64::NAN.to_bits()),
            discarded: AtomicU32::new(0.0f32.to_bits()),
            cc_available: AtomicBool::new(false),
            cc_enabled: AtomicBool::new(false),
            caption: Mutex::new(String::new()),
            audible_pts: AtomicU64::new(f64::NAN.to_bits()),
            output_rate: device.sample_rate as f64,
            output_channels: device.channels as usize,
        });

        let (tx, rx) = std::sync::mpsc::channel();

        // Only a segmented live source can be behind its own live edge, and
        // only a live source has no duration to state. A recording has both a
        // duration and every byte already written, so it must not be delayed.
        //
        // How long the open took decides whether the wait is needed at all.
        // The server does not answer with a playlist until the tuner is locked
        // and there is something to list, so a slow open means it tuned for
        // us: the playlist is new, it is growing in real time, and reading
        // flat out would pin the player against the writer. A fast one means
        // the session was already running and every segment behind the edge is
        // already on disk, where the wait buys nothing but four seconds of
        // spinner. Measured on this server: 8.61s tuning, 0.35s warm — far
        // enough apart that the threshold between them is not a fine judgement.
        let preroll = if transport == Transport::Timeshift {
            // A local file. There is nothing to fall behind: the writer is
            // already as far ahead as the network has managed, and the reader
            // cannot outrun it by more than the wait in the EOF branch.
            Duration::ZERO
        } else if transport == Transport::Hls && facts.duration <= 0.0 {
            if opening.elapsed() < WARM_OPEN {
                Duration::ZERO
            } else {
                LIVE_PREROLL
            }
        } else {
            Duration::ZERO
        };
        eprintln!(
            "[player] opened in {:.2}s, preroll {:.0}s",
            opening.elapsed().as_secs_f64(),
            preroll.as_secs_f64()
        );

        let mut threads = Vec::new();
        threads.push({
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("rustdvr-decode".into())
                .spawn(move || decode_loop(media, shared, rx, preroll, transport))?
        });
        threads.push({
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("rustdvr-present".into())
                .spawn(move || present_loop(shared, repaint))?
        });

        // Audio last: the device starts pulling the moment it is built, and
        // there should be something for it to pull.
        let output = if facts.has_audio {
            match device.start(Arc::clone(&shared)) {
                Ok(out) => Some(out),
                Err(e) => {
                    // A picture with no sound beats no picture. The clock falls
                    // back to wall time, which is what a silent stream would
                    // have used anyway.
                    eprintln!("[player] no audio output: {e:#}");
                    shared.facts.lock().unwrap().has_audio = false;
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            shared,
            commands: tx,
            transport,
            _audio: output,
            threads,
            abort,
        })
    }

    /// What FFmpeg build this is, and what licence it carries.
    pub fn backend() -> String {
        // Playback faults on a live stream cannot be reproduced in a test: the
        // stream only exists in real time, only arrives at 1x, and the failure
        // being chased takes tens of seconds to develop. This runs the pipeline
        // headless against a real URL and prints what it measured. It does
        // nothing at all unless RUSTDVR_SELFTEST is set, and it is called from
        // here because this is the one player entry point the application
        // already touches before it builds any interface.
        self_test();
        format!("FFmpeg {} ({})", ffi::version(), ffi::license())
    }

    pub fn frame(&self) -> std::sync::MutexGuard<'_, FrameSlot> {
        self.shared.frame.lock().unwrap()
    }

    pub fn decoded(&self) -> u64 {
        self.shared.decoded.load(Ordering::Relaxed)
    }

    pub fn dropped(&self) -> u64 {
        self.shared.dropped.load(Ordering::Relaxed)
    }

    /// Frames waiting to be shown. Should sit near the queue limit on a stream
    /// that is keeping up, and hover near zero on one that is not: an empty
    /// queue means the decoder cannot supply frames fast enough, which is a
    /// different fault from a full one that cannot be drained.
    pub fn queued_frames(&self) -> usize {
        self.shared.video.lock().unwrap().len()
    }

    /// Seconds of audio buffered for the device.
    pub fn queued_audio(&self) -> f64 {
        let samples = self.shared.audio_buffered.load(Ordering::Relaxed) as f64;
        let rate = self.shared.output_rate.max(1.0);
        let channels = self.shared.output_channels.max(1) as f64;
        samples / (rate * channels)
    }

    /// How long the decode thread spent producing each frame, in milliseconds,
    /// averaged over recent frames. This is the number that decides whether
    /// 60fps is achievable at all: it covers demux, decode and the colour
    /// conversion, which all happen on one thread.
    pub fn decode_ms(&self) -> f32 {
        f32::from_bits(self.shared.decode_ms.load(Ordering::Relaxed))
    }

    pub fn error(&self) -> Option<String> {
        self.shared.error.lock().unwrap().clone()
    }

    /// Tell the player how much of its buffer has been recycled away.
    pub fn set_discarded(&self, fraction: f64) {
        self.shared
            .discarded
            .store((fraction.clamp(0.0, 1.0) as f32).to_bits(), Ordering::Relaxed);
    }

    /// Whether this stream has been seen to carry closed captions.
    pub fn captions_available(&self) -> bool {
        self.shared.cc_available.load(Ordering::Relaxed)
    }

    pub fn captions_on(&self) -> bool {
        self.shared.cc_enabled.load(Ordering::Relaxed)
    }

    /// Turn captions on or off. Takes effect on the next decoded frame.
    pub fn set_captions(&self, on: bool) {
        self.shared.cc_enabled.store(on, Ordering::SeqCst);
        if !on {
            self.shared.caption.lock().unwrap().clear();
        }
    }

    /// The caption line to draw, if any.
    pub fn caption(&self) -> Option<String> {
        let text = self.shared.caption.lock().unwrap();
        (!text.is_empty()).then(|| text.clone())
    }

    pub fn stop(&self) {
        self.shared.quit.store(true, Ordering::SeqCst);
        let _ = self.commands.send(Command::Quit);
        // The flag last, and separately, because it is the only one of the
        // three that a thread sitting inside av_read_frame can observe. The
        // other two are checked between reads, and a read of a live HLS
        // segment has been measured at 6.3 seconds against a 15s rw_timeout —
        // which is how long `Drop` used to block the UI thread in its join.
        self.abort.store(1, Ordering::SeqCst);
    }

    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::SeqCst);
        self.shared.clock.set_running(!paused);
        if let Some(audio) = &self._audio {
            audio.set_paused(paused);
        }
    }

    pub fn set_volume(&self, level: f64) {
        let level = level.clamp(0.0, 1.0) as f32;
        self.shared.volume.store(level.to_bits(), Ordering::Relaxed);
    }

    /// Where playback currently is, in seconds.
    pub fn position(&self) -> Option<f64> {
        let now = self.shared.clock.now();
        if now.is_finite() && now >= 0.0 {
            Some(now)
        } else {
            None
        }
    }

    /// What the container says the whole stream is. For a growing HLS playlist
    /// this is the live edge and it moves as the session goes on.
    pub fn duration(&self) -> Option<f64> {
        let facts = self.shared.facts.lock().unwrap();
        (facts.duration > 0.0).then_some(facts.duration)
    }

    /// The window that can be seeked within, as `(start, end)` seconds.
    ///
    /// The end is whichever is further on: a duration the container actually
    /// stated, or the furthest timestamp decoded so far. For a recording the
    /// first is right; for a live playlist there is no stated duration at all,
    /// and the second is the live edge.
    pub fn seek_range(&self) -> Option<(f64, f64)> {
        let facts = self.shared.facts.lock().unwrap();
        if !facts.seekable {
            return None;
        }
        // The bottom of the window is the first timestamp actually seen, not
        // zero. A broadcast transport stream is stamped from wherever the
        // originating encoder's clock happened to be, not from the start of
        // anything: this channel opens at around 75,262 seconds, and a
        // recording off it at 81,876. Assuming zero made the progress bar read
        // twenty-two hours long with the handle pinned at the far right, and
        // made every seek into the left of the bar ask for a position before
        // the stream begins — which FFmpeg's HLS demuxer rejects outright
        // (find_timestamp_in_playlist treats anything below its first
        // timestamp as not found, and the seek fails with EIO).
        let start = self.shared.first_pts();
        let start = if start.is_finite() { start } else { 0.0 };

        // A stated duration is a *length*, so the position it describes is the
        // first timestamp plus it. Comparing it against the live edge directly
        // compares a length with an absolute position — 3,423 against 81,876 —
        // and the edge always wins, so a fifty-seven minute recording offered a
        // seekable window covering only the seconds already decoded.
        let stated_end = if facts.duration > 0.0 {
            start + facts.duration
        } else {
            f64::NEG_INFINITY
        };
        let end = stated_end.max(self.shared.edge());

        // A couple of seconds in, there is nothing to scrub through yet.
        if end - start <= 1.0 {
            return None;
        }

        // Move the start past whatever the live buffer has given back to the
        // disk. Measured in bytes and applied as time, which is close enough
        // on a broadcast stream whose bitrate barely moves.
        let discarded = f32::from_bits(self.shared.discarded.load(Ordering::Relaxed)) as f64;
        let start = start + (end - start) * discarded.clamp(0.0, 1.0);
        if end - start <= 1.0 {
            return None;
        }
        Some((start, end))
    }

    pub fn is_seekable(&self) -> bool {
        self.seek_range().is_some()
    }

    /// Where a request would actually land, once held inside the window.
    fn clamped_target(&self, secs: f64) -> Option<f64> {
        let (start, end) = self.seek_range()?;
        // Landing exactly on the live edge asks for a segment still being
        // written, which stalls until it lands. Stop just short of it.
        Some(secs.clamp(start, (end - 1.5).max(start)))
    }

    /// Jump to an absolute position, clamped into the seekable window.
    pub fn seek_to(&self, secs: f64) -> bool {
        let Some(target) = self.clamped_target(secs) else { return false };
        self.commands.send(Command::Seek(target)).is_ok()
    }

    /// Jump forward or back by an interval.
    ///
    /// Clamping can reverse the direction, and silently doing the opposite of
    /// what a button says is worse than doing nothing. Pressing skip-forward
    /// during live playback is the case that matters: there is nothing ahead,
    /// the target clamps to just short of the live edge, and that is a second
    /// or so *behind* where playback already is — so the picture jumps
    /// backwards, refetches a segment and glitches, in response to a button
    /// with a forward-pointing arrow on it.
    ///
    /// Returns true either way, because there is nothing wrong: the request
    /// was understood and there was simply nowhere to go. Returning false is
    /// how the interface is told a source cannot seek at all.
    pub fn seek_by(&self, delta_secs: f64) -> bool {
        let Some(current) = self.position() else { return false };
        let Some(target) = self.clamped_target(current + delta_secs) else { return false };
        if (delta_secs > 0.0 && target <= current + 0.25)
            || (delta_secs < 0.0 && target >= current - 0.25)
        {
            return true;
        }
        self.commands.send(Command::Seek(target)).is_ok()
    }

    pub fn seek_to_live(&self) -> bool {
        let Some((_, end)) = self.seek_range() else { return false };
        // Already live. Seeking anyway would throw away the queues and refetch
        // a segment to arrive back where it started.
        if let Some(current) = self.position() {
            if end - current < 2.0 {
                return true;
            }
        }
        self.seek_to(end)
    }

    /// How far behind the live edge playback is, in seconds.
    pub fn behind_live(&self) -> Option<f64> {
        let (_, end) = self.seek_range()?;
        let position = self.position()?;
        Some((end - position).max(0.0))
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
        // Let the device go before the threads, so the callback cannot be
        // reading a queue that is being torn down.
        self._audio = None;
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

/// Run the pipeline headless against a real stream and report what it did.
///
/// Driven entirely by the environment so it can never affect a normal run:
///
/// * `RUSTDVR_SELFTEST`      — the URL to open. Nothing happens without it.
/// * `RUSTDVR_SELFTEST_SECS` — how long to watch for. Default 60.
/// * `RUSTDVR_SEEK_TEST`     — seconds in at which to press skip-back.
///
/// Exits the process when it finishes, because there is no interface behind it
/// and leaving a window open would only confuse whoever is reading the numbers.
fn self_test() {
    let Ok(url) = std::env::var("RUSTDVR_SELFTEST") else { return };
    let secs: f64 = std::env::var("RUSTDVR_SELFTEST_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60.0);
    let seek_at: Option<f64> = std::env::var("RUSTDVR_SEEK_TEST")
        .ok()
        .and_then(|v| v.parse().ok());

    eprintln!("[selftest] opening {url}");
    let opened = Instant::now();
    let player = match Player::open(&url, Transport::of(&url), || {}) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[selftest] could not open: {e:#}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "[selftest] open took {:.2}s, seekable {}",
        opened.elapsed().as_secs_f64(),
        player.is_seekable()
    );

    // Each step presses a transport control and, three seconds later, reports
    // where playback actually got to. Three seconds because a seek in a
    // segmented stream is not instant: the segment has to be fetched and the
    // clock re-anchored on a frame that really arrived.
    let steps: Vec<(&str, Box<dyn Fn(&Player) -> bool>, f64)> = vec![
        ("skip back 15s", Box::new(|p: &Player| p.seek_by(-15.0)), -15.0),
        ("skip back 15s again", Box::new(|p: &Player| p.seek_by(-15.0)), -15.0),
        // The point of jump-to-live is to undo a rewind, so it is tested after
        // sitting rewound for a while: the live edge has to have kept moving
        // during that time or this lands back where the rewind started.
        ("jump to live", Box::new(|p: &Player| p.seek_to_live()), f64::NAN),
        ("skip forward 30s at live", Box::new(|p: &Player| p.seek_by(30.0)), 0.0),
    ];

    let started = Instant::now();
    let mut step = 0usize;
    let mut pending: Option<(Instant, f64, &str, f64)> = None;
    while started.elapsed().as_secs_f64() < secs {
        std::thread::sleep(Duration::from_millis(100));

        if let Some(at) = seek_at {
            // Steps run eight seconds apart, so each one settles before the
            // next and the numbers cannot be blamed on the previous jump.
            let due = at + step as f64 * 8.0;
            if pending.is_none() && step < steps.len() && started.elapsed().as_secs_f64() >= due {
                let (label, action, expected) = &steps[step];
                step += 1;
                let before = player.position().unwrap_or(f64::NAN);
                let range = player.seek_range();
                let behind = player.behind_live().unwrap_or(f64::NAN);
                let accepted = action(&player);
                eprintln!(
                    "[selftest] {label} at {before:.3}s ({behind:.1}s behind live), window {range:?}, accepted {accepted}"
                );
                pending = Some((Instant::now(), before, label, *expected));
            }
        }

        if let Some((when, before, label, expected)) = pending {
            if when.elapsed() > Duration::from_secs(3) {
                pending = None;
                let after = player.position().unwrap_or(f64::NAN);
                let moved = after - before - when.elapsed().as_secs_f64();
                if expected.is_finite() {
                    eprintln!(
                        "[selftest]   {label}: moved {moved:+.3}s, wanted {expected:+.3}s, error {:+.3}s",
                        moved - expected
                    );
                } else {
                    eprintln!("[selftest]   {label}: moved {moved:+.3}s");
                }
            }
        }
    }

    eprintln!(
        "[selftest] done: decoded {} dropped {} ({:.2}% of decoded), {:.2} fps average",
        player.decoded(),
        player.dropped(),
        player.dropped() as f64 / player.decoded().max(1) as f64 * 100.0,
        player.decoded() as f64 / started.elapsed().as_secs_f64(),
    );
    drop(player);
    std::process::exit(0);
}

/// Demux, decode, and fill the queues. Owns the FFmpeg handle outright.
fn decode_loop(
    media: Media,
    shared: Arc<Shared>,
    commands: Receiver<Command>,
    preroll: Duration,
    transport: Transport,
) {
    let handle = media.0;
    // The container's idea of the picture size, which is only a starting point.
    // It is what the codec parameters carried at open, and a Channels HLS
    // master playlist offers several variants, so the decoder can and does hand
    // back something else. These follow the frames from here on.
    let mut width = unsafe { ffi::rd_video_width(handle) }.max(0) as u32;
    let mut height = unsafe { ffi::rd_video_height(handle) }.max(0) as u32;
    let channels = unsafe { ffi::rd_out_channels(handle) }.max(1) as usize;
    let rate = unsafe { ffi::rd_out_rate(handle) }.max(1) as f64;

    let mut frame_bytes = (width as usize) * (height as usize) * 4;
    let mut last_facts_check = Instant::now();
    let mut failures = 0u32;
    // Mirrors the shared flag, so the shim is only told when it changes.
    let mut cc_on = false;
    // Stream time of the last caption, for ageing out a stale one.
    let mut caption_pts = f64::NAN;

    let opened_at = Instant::now();
    let mut report_next_pts = false;
    let mut last_video_pts = f64::NAN;
    let mut decoded_at_last_report = 0u64;
    // Frames before this timestamp are decoded but not shown, so a seek lands
    // where it was asked to rather than at the segment boundary before it.
    let mut skip_until = f64::NEG_INFINITY;

    // Fall behind the live edge before reading a packet — but only when that
    // is worth four seconds, which is not always.
    //
    // The wait exists so reads only ever ask for finished segments. On a
    // channel the server has just tuned it earns that: the playlist starts
    // empty and grows in real time, so a player that reads as fast as it can
    // pins itself against the writer. Measured without the wait on a cold
    // tune, ten seconds in: `pos 9.7s edge 9.8s`, reads stalling 368ms every
    // second, 47 starved wake-ups and six frames dropped.
    //
    // On a session the server already has running it earns nothing. Those
    // segments are written, so they arrive at LAN speed and playback races
    // straight back to the edge regardless: `pos 2.0s edge 2.2s` either way,
    // nothing starved, four seconds spent on a spinner for an identical
    // result.
    //
    // Which of the two this is has already been measured by the time we get
    // here — see `open_at`, where a slow open means the server was tuning.
    if !preroll.is_zero() {
        let until = Instant::now() + preroll;
        while Instant::now() < until && !shared.quit.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    loop {
        if shared.quit.load(Ordering::SeqCst) {
            break;
        }

        // Commands first, so a seek is not made to wait behind a full queue.
        let mut seek_to = None;
        while let Ok(command) = commands.try_recv() {
            match command {
                Command::Seek(target) => seek_to = Some(target),
                Command::Quit => return,
            }
        }

        if let Some(target) = seek_to {
            report_next_pts = true;
            last_video_pts = f64::NAN;
            let rc = unsafe { ffi::rd_seek(handle, target) };
            eprintln!(
                "[player] seek to {target:.3}s at wall {:.1}s -> {}",
                opened_at.elapsed().as_secs_f64(),
                if rc == 0 { "ok" } else { "REFUSED by demuxer" }
            );
            // Whatever was on screen was said before the jump.
            caption_pts = f64::NAN;
            shared.caption.lock().unwrap().clear();

            if rc == 0 {
                // Everything queued belongs to before the seek, so it all goes.
                // The clock is moved to the requested position immediately so
                // the progress bar answers the button press rather than waiting
                // for a segment to arrive.
                shared.clock.reset(target);
                recycle_video(&shared);
                shared.audio.lock().unwrap().clear();
                shared.audio_buffered.store(0, Ordering::Relaxed);
                // The last audible timestamp describes a moment that is no
                // longer being played. Correcting drift against it would drive
                // the clock by the length of the jump.
                shared
                    .audible_pts
                    .store(f64::NAN.to_bits(), Ordering::Relaxed);

                // Decode past the segment boundary to the frame that was
                // actually asked for.
                //
                // A seek has to land on a keyframe, and in a segmented stream
                // that means the start of the segment containing the target:
                // measured, skip-back was arriving 1.0 to 1.2 seconds early
                // every time, because Channels cuts two-second segments and
                // AVSEEK_FLAG_BACKWARD lands at the front of one. "Back 15
                // seconds" that moves 16.2 is not what the button says.
                //
                // The frames in between are decoded and thrown away rather
                // than shown, which is the only way to arrive mid-GOP with a
                // complete picture. It costs a fraction of a second: at the
                // measured decode cost this is around 300ms of work for a
                // whole segment, and it happens once per press.
                skip_until = target;

                // The requested position is a request, not a result. Even with
                // the skip above, the frame that arrives is the only honest
                // statement of where playback now is: leaving the clock on the
                // request makes the presentation thread compare real frames
                // against a fictional position. Measured before this was
                // added, frames arrived stamped six seconds later than the
                // clock and the picture froze solid until wall time caught up
                // — the queue sat full, the decoder stopped, and nothing was
                // shown for the entire gap.
                shared.resync.store(true, Ordering::SeqCst);
            }
            continue;
        }

        // Refresh what the container claims. A live playlist grows, so the
        // seekable window and the live edge both move while playing.
        if last_facts_check.elapsed() > Duration::from_secs(2) {
            // Where the time is actually going. Printed rather than only shown
            // in the stats card, because a stutter has to be diagnosed from
            // what the pipeline was doing at the moment it happened, and the
            // card is a snapshot taken afterwards.
            let buffered = shared.audio_buffered.load(Ordering::Relaxed) as f64
                / (rate * channels as f64);

            let (mut read_us, mut dec_us, mut read_max_us) = (0i64, 0i64, 0i64);
            unsafe {
                ffi::rd_take_timings(handle, &mut read_us, &mut dec_us, &mut read_max_us)
            };
            let window = last_facts_check.elapsed().as_secs_f64().max(0.001);

            let queued = shared.video.lock().unwrap().len();
            let held = shared.spare.lock().unwrap().len() + queued + 1;
            let position = shared.clock.now();

            // How far the picture is ahead of the sound, in milliseconds.
            // Positive means video is running early.
            let audible = f64::from_bits(shared.audible_pts.load(Ordering::Relaxed));
            let skew = if audible.is_finite() && shared.clock.started() {
                position - audible
            } else {
                f64::NAN
            };

            // One line, and every number on it answers a different question:
            // read/dec/conv say which of the three jobs on this thread is
            // costing what (they are milliseconds spent per second of wall
            // time, so 1000 would be saturated); stall is the worst single
            // read, which is where a live segment fetch shows up; starved and
            // underrun are the two ways the pipeline runs dry, and they have
            // different causes; skew is A/V sync.
            eprintln!(
                "[player] {:.1}s pos {:.1}s edge {:.1}s | read {:.0}+dec {:.0}+conv {:.0} ms/s, stall {:.0}ms x{} | vq {}/{} aq {:.0}ms held {} ({:.0}MB) | fps {:.2} dropped {} starved {} underrun {} | skew {:+.0}ms rate {:.5}",
                opened_at.elapsed().as_secs_f64(),
                position,
                shared.live_edge(),
                read_us as f64 / 1000.0 / window,
                dec_us as f64 / 1000.0 / window,
                shared.convert_us.load(Ordering::Relaxed) as f64 * 59.94 / 1000.0,
                read_max_us as f64 / 1000.0,
                shared.stalls.swap(0, Ordering::Relaxed),
                queued,
                MAX_VIDEO_FRAMES,
                buffered * 1000.0,
                held,
                held as f64 * frame_bytes as f64 / (1024.0 * 1024.0),
                (shared.decoded.load(Ordering::Relaxed) - decoded_at_last_report) as f64 / window,
                shared.dropped.load(Ordering::Relaxed),
                shared.starved.swap(0, Ordering::Relaxed),
                shared.underruns.swap(0, Ordering::Relaxed),
                skew * 1000.0,
                shared.clock.rate(),
            );
            decoded_at_last_report = shared.decoded.load(Ordering::Relaxed);

            // Slow drift correction.
            //
            // The performance counter and the audio DAC's crystal are separate
            // oscillators, tens of parts per million apart, so a clock left
            // alone walks away from the sound over hours. What is corrected
            // against is the skew above: where the picture is against what is
            // actually audible, which is the thing that matters and whose
            // correct value is zero.
            //
            // It used to correct against how much audio was queued, towards a
            // target of half the queue limit — half a second. That target was
            // unreachable: video and audio come from the same interleaved
            // stream decoded by the same thread, so filling the video queue
            // stops audio being decoded, and the audio buffer therefore cannot
            // exceed the video queue's own depth, which is 200ms. The
            // controller spent every session driving at a number it could never
            // arrive at, and the rate saturated at the -0.2% clamp and stayed
            // there: measured drifting 1.00000 -> 0.99843 over seventy seconds
            // and still falling. That is the picture running slow against the
            // sound for as long as the application is open.
            if shared.facts.lock().unwrap().has_audio && skew.is_finite() {
                shared.clock.nudge(-skew);
            }

            last_facts_check = Instant::now();
            let duration = unsafe { ffi::rd_duration(handle) };
            let seekable = unsafe { ffi::rd_seekable(handle) } != 0;
            let mut facts = shared.facts.lock().unwrap();
            if duration > facts.duration {
                facts.duration = duration;
            }
            facts.seekable = seekable;
        }

        // Back off when both queues are full, otherwise this thread decodes an
        // entire live stream as fast as it can download it.
        if queues_full(&shared, rate, channels) {
            std::thread::sleep(Duration::from_millis(4));
            continue;
        }

        let started = Instant::now();
        let mut pts = 0.0f64;
        let kind = unsafe { ffi::rd_next(handle, &mut pts) };
        let demuxed = started.elapsed();

        // A live HLS segment fetch happens inside rd_next, on this thread, so
        // a slow one stops decoding entirely for its duration. Counted rather
        // than averaged, because that is exactly the kind of fault an average
        // hides: this pipeline once stalled a full second every two seconds
        // while averaging a few milliseconds a frame.
        if demuxed.as_millis() > 16 {
            shared.stalls.fetch_add(1, Ordering::Relaxed);
        }

        match kind {
            ffi::RD_VIDEO => {
                failures = 0;
                shared.observe(pts);
                shared.observe_first(pts);
                if report_next_pts {
                    report_next_pts = false;
                    eprintln!("[player] first frame after seek lands at {pts:.3}s");
                }
                // A timestamp that jumps is not a decode fault but it looks
                // exactly like one downstream, so it has to be visible.
                if last_video_pts.is_finite() {
                    let step = pts - last_video_pts;
                    if !(-0.001..0.5).contains(&step) {
                        eprintln!(
                            "[player] PTS DISCONTINUITY at wall {:.1}s: {last_video_pts:.3}s -> {pts:.3}s ({step:+.3}s)",
                            opened_at.elapsed().as_secs_f64()
                        );
                    }
                }
                last_video_pts = pts;

                // Still catching up to where the seek was asked to land. The
                // frame had to be decoded — the ones after it depend on it —
                // but it is not converted or queued, which skips the only
                // expensive part.
                if pts < skip_until - 0.001 {
                    continue;
                }
                if skip_until.is_finite() {
                    eprintln!(
                        "[player] resumed at {pts:.3}s (asked for {skip_until:.3}s)"
                    );
                    skip_until = f64::NEG_INFINITY;
                }

                // Captions ride inside the picture, so this is the only place
                // they can be collected: the shim extracts the A53 side data
                // from each decoded frame and hands back whatever the EIA-608
                // decoder made of it.
                let available = unsafe { ffi::rd_cc_available(handle) } != 0;
                if available != shared.cc_available.load(Ordering::Relaxed) {
                    shared.cc_available.store(available, Ordering::Relaxed);
                }
                let want_cc = shared.cc_enabled.load(Ordering::Relaxed);
                if want_cc != cc_on {
                    cc_on = want_cc;
                    unsafe { ffi::rd_cc_enable(handle, cc_on as i32) };
                }
                if cc_on {
                    let mut buffer = [0i8; 512];
                    let got = unsafe {
                        ffi::rd_cc_take(handle, buffer.as_mut_ptr(), buffer.len() as i32)
                    };
                    if got != 0 {
                        let text = unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) }
                            .to_string_lossy()
                            .trim()
                            .to_string();
                        caption_pts = pts;
                        *shared.caption.lock().unwrap() = text;
                    } else if caption_pts.is_finite() && pts - caption_pts > CAPTION_HOLD {
                        // Nothing has arrived for a while, so what is on
                        // screen is stale. A caption decoder is not obliged to
                        // send anything to say a programme has stopped talking
                        // — it simply stops — and without a timeout the last
                        // line of dialogue sits over the whole advert break
                        // that follows it.
                        //
                        // Measured against the stream's own clock rather than
                        // the wall's, so pausing holds the caption rather than
                        // ageing it out while the picture is frozen.
                        caption_pts = f64::NAN;
                        shared.caption.lock().unwrap().clear();
                    }
                }

                // Size the buffer to the picture that was actually decoded.
                //
                // This used to be fixed at whatever the container advertised at
                // open, and `rd_video_copy` wrote the frame's own dimensions
                // into it unchecked. When the two disagreed — which is exactly
                // what changing stream quality causes — the copy ran past the
                // end of the allocation and corrupted the heap. That was the
                // hard crash on changing resolution.
                let fw = unsafe { ffi::rd_frame_width(handle) }.max(0) as u32;
                let fh = unsafe { ffi::rd_frame_height(handle) }.max(0) as u32;
                if fw > 0 && fh > 0 && (fw != width || fh != height) {
                    eprintln!(
                        "[player] picture size changed {width}x{height} -> {fw}x{fh}, resizing"
                    );
                    width = fw;
                    height = fh;
                    frame_bytes = (fw as usize) * (fh as usize) * 4;
                    // Every pooled buffer is now the wrong size. Returning them
                    // would only hand a short one straight back to the copy.
                    shared.spare.lock().unwrap().clear();
                }

                let mut pixels = take_buffer(&shared, frame_bytes);
                let convert_started = Instant::now();
                let copied = unsafe {
                    ffi::rd_video_copy(
                        handle,
                        pixels.as_mut_ptr(),
                        (width * 4) as i32,
                        width as i32,
                        height as i32,
                    )
                };
                ema(
                    &shared.convert_us,
                    convert_started.elapsed().as_micros().min(u32::MAX as u128) as u32,
                );
                if copied > 0 {
                    shared.decoded.fetch_add(1, Ordering::Relaxed);
                    let mut queue = shared.video.lock().unwrap();
                    queue.push_back(VideoFrame { pts, width, height, pixels });
                } else {
                    shared.spare.lock().unwrap().push(pixels);
                }

                // Demux, decode and colour conversion, measured together
                // because they all happen here on this one thread. Above
                // roughly 16ms this cannot sustain 60fps no matter what the
                // rest of the pipeline does.
                let ms = started.elapsed().as_secs_f32() * 1000.0;
                let previous = f32::from_bits(shared.decode_ms.load(Ordering::Relaxed));
                let smoothed = if previous > 0.0 {
                    previous * 0.9 + ms * 0.1
                } else {
                    ms
                };
                shared.decode_ms.store(smoothed.to_bits(), Ordering::Relaxed);
            }
            ffi::RD_AUDIO => {
                failures = 0;
                shared.observe(pts);
                shared.observe_first(pts);
                // Sound from before the seek target would play under a picture
                // that has already moved past it.
                if pts < skip_until - 0.001 {
                    continue;
                }
                let max = unsafe { ffi::rd_audio_samples(handle) }.max(0) as usize;
                if max > 0 {
                    let mut samples = vec![0f32; max * channels];
                    let got = unsafe {
                        ffi::rd_audio_copy(handle, samples.as_mut_ptr(), max as i32)
                    };
                    if got > 0 {
                        samples.truncate(got as usize * channels);
                        shared
                            .audio_buffered
                            .fetch_add(samples.len(), Ordering::Relaxed);
                        shared.audio.lock().unwrap().push_back(AudioChunk {
                            pts,
                            samples,
                            consumed: 0,
                        });
                    }
                }
            }
            ffi::RD_EOF => {
                if transport == Transport::Timeshift {
                    // Caught up with the writer. Normal, and frequent: the
                    // player drains its queue faster than a 5 Mbit stream
                    // fills the file, so this happens several times a second
                    // at the live edge. Not a failure, and not counted as one
                    // — counting it would end the session after ten seconds
                    // of perfectly healthy playback.
                    unsafe { ffi::rd_retry_eof(handle) };
                    std::thread::sleep(Duration::from_millis(30));
                } else {
                    // A live stream can report EOF between segments. Wait and
                    // try again rather than tearing down a session that is
                    // still valid.
                    std::thread::sleep(Duration::from_millis(250));
                    failures += 1;
                    if failures > 40 {
                        *shared.error.lock().unwrap() = Some("the stream ended".into());
                        break;
                    }
                }
            }
            ffi::RD_NOTHING => {}
            _ => {
                failures += 1;
                if failures > 200 {
                    *shared.error.lock().unwrap() =
                        Some("the stream stopped decoding".into());
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

/// Whether to stop decoding for a moment.
///
/// Either queue being full is enough. Requiring both was a real bug: audio is
/// consumed steadily by the device and rarely reaches its limit, so the
/// condition was almost never true and the video queue grew without bound. At
/// 1080p a frame is 8.3MB, so that is memory climbing by half a gigabyte a
/// second, and latency climbing with it, until the whole thing stalls.
fn queues_full(shared: &Shared, rate: f64, channels: usize) -> bool {
    if shared.video.lock().unwrap().len() >= MAX_VIDEO_FRAMES {
        return true;
    }
    let buffered = shared.audio_buffered.load(Ordering::Relaxed);
    buffered as f64 / (rate * channels as f64) >= MAX_AUDIO_SECONDS
}

/// Exponential moving average, in place, on a `u32` of microseconds.
fn ema(cell: &AtomicU32, sample: u32) {
    let previous = cell.load(Ordering::Relaxed);
    let next = if previous == 0 {
        sample
    } else {
        ((previous as u64 * 9 + sample as u64) / 10) as u32
    };
    cell.store(next, Ordering::Relaxed);
}

/// A buffer of exactly `bytes`, recycled if one is free.
///
/// Deliberately not cleared. The previous version did `clear()` then
/// `resize(bytes, 0)`, which zeroes the whole buffer: at 1080p that is an 8.3MB
/// memset per frame, 500MB/s of pure write bandwidth at 60fps, on the decode
/// thread, immediately before `sws_scale` overwrites every one of those bytes.
fn take_buffer(shared: &Shared, bytes: usize) -> Vec<u8> {
    let mut spare = shared.spare.lock().unwrap();
    match spare.pop() {
        Some(buffer) if buffer.len() == bytes => buffer,
        _ => vec![0u8; bytes],
    }
}

fn recycle_video(shared: &Shared) {
    let mut queue = shared.video.lock().unwrap();
    let mut spare = shared.spare.lock().unwrap();
    for frame in queue.drain(..) {
        if spare.len() < MAX_SPARE_BUFFERS {
            spare.push(frame.pixels);
        }
    }
}

/// Publish frames when the clock says they are due.
///
/// Frames whose time has already passed are dropped rather than shown late:
/// showing them would push everything after them further behind, which is how
/// a player that is momentarily slow never catches up.
fn present_loop(shared: Arc<Shared>, repaint: impl Fn() + Send + Sync + 'static) {
    loop {
        if shared.quit.load(Ordering::SeqCst) {
            return;
        }
        if shared.paused.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(16));
            continue;
        }

        let mut due: Option<VideoFrame> = None;
        let mut wait = Duration::from_millis(4);

        let now;
        {
            let mut queue = shared.video.lock().unwrap();
            if queue.is_empty() && shared.clock.started() {
                shared.starved.fetch_add(1, Ordering::Relaxed);
            }

            // Start on a cushion rather than on the very first frame.
            //
            // This is the buffer the four-second sleep before reading was
            // supposed to provide and did not. Holding the clock for a few
            // frames costs only as long as they take to arrive — a fraction of
            // a second where segments are already written, and naturally
            // longer where they are not, which is the case the wait existed
            // for. The deadline is there so a source that simply cannot
            // deliver that many frames still starts.
            if !shared.clock.started() {
                let deep_enough = queue.len() >= PREROLL_FRAMES;
                let waited = shared.opened_at.elapsed() > PREROLL_MAX;
                if deep_enough || waited {
                    if let Some(first) = queue.front() {
                        shared.clock.start(first.pts);
                        shared.align_audio.store(true, Ordering::SeqCst);
                    }
                }
            }

            // Then snap the clock onto the sound, once, as soon as the device
            // says what it is playing.
            //
            // A video frame's timestamp says where *decoding* has got to, not
            // where playback has. Between the decoder and the speaker sits the
            // audio queue, and on a source that can be read faster than real
            // time that queue fills to its limit immediately: measured on a
            // recording, audio sat a full second deep while the clock, anchored
            // on a video frame, ran a second ahead of it. A second of the
            // picture leading the sound is not subtle, and the slow drift
            // correction below cannot fix it — capped at 0.2%, closing a second
            // would take eight minutes.
            //
            // So it is corrected in one step rather than trimmed. Doing it here
            // costs nothing visible: at startup nothing has been shown yet, and
            // after a seek the picture is being rebuilt anyway.
            if shared.align_audio.load(Ordering::SeqCst) {
                let audible = f64::from_bits(shared.audible_pts.load(Ordering::Relaxed));
                if audible.is_finite() {
                    shared.clock.reset(audible);
                    shared.align_audio.store(false, Ordering::SeqCst);
                }
            }

            // A seek has happened and this is the first frame back from it.
            // Take its timestamp as the truth about where playback is, rather
            // than the position that was asked for. Done here, holding the
            // queue lock and before the clock is read, so no frame is ever
            // judged against a position from before the re-anchor.
            if shared.resync.load(Ordering::SeqCst) {
                if let Some(front) = queue.front() {
                    shared.clock.reset(front.pts);
                    shared.resync.store(false, Ordering::SeqCst);
                    // ...and then onto the audio, as at startup.
                    shared.align_audio.store(true, Ordering::SeqCst);
                }
            }

            now = shared.clock.now();

            // Take the frame that is due, and only skip past ones that are
            // genuinely stale.
            //
            // The previous version dropped every frame whose time had passed
            // by any margin at all, which meant a single late wake-up threw
            // away a frame that was one millisecond overdue and perfectly
            // showable. On a 60fps source that is visible judder produced
            // entirely by the presentation policy rather than by anything
            // wrong upstream. A frame is only skipped once a *newer* one is
            // also already due, which is the only case where showing it would
            // put the picture behind.
            while let Some(front) = queue.front() {
                if front.pts > now + 0.0005 {
                    // Not yet. Sleep until it is, but never so long that a
                    // pause or a seek goes unnoticed.
                    let delay = (front.pts - now).clamp(0.0005, 0.020);
                    wait = Duration::from_secs_f64(delay);
                    break;
                }
                if let Some(previous) = due.replace(queue.pop_front().unwrap()) {
                    shared.dropped.fetch_add(1, Ordering::Relaxed);
                    shared.spare.lock().unwrap().push(previous.pixels);
                }
            }
        }

        if let Some(frame) = due {
            let mut slot = shared.frame.lock().unwrap();
            let old = std::mem::replace(&mut slot.pixels, frame.pixels);
            slot.width = frame.width;
            slot.height = frame.height;
            slot.generation = slot.generation.wrapping_add(1);
            drop(slot);

            if !old.is_empty() {
                let mut spare = shared.spare.lock().unwrap();
                if spare.len() < MAX_SPARE_BUFFERS {
                    spare.push(old);
                }
            }
            repaint();
        }

        // Windows' timer granularity was the first suspect for the unsteady
        // frame rate, on the reasoning that a 2ms sleep costs 15.6ms unless
        // something has called timeBeginPeriod. It was measured rather than
        // assumed: the worst overshoot across a run was 0.6 to 1.1ms, so the
        // timer is already at high resolution here and no amount of correct
        // scheduling arithmetic was being wasted on it. The frames were
        // missing, not mistimed.
        std::thread::sleep(wait);
    }
}
