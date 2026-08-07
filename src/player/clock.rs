//! The master clock.
//!
//! Hand rolled on `QueryPerformanceCounter`, and deliberately not derived from
//! the audio device.
//!
//! Taking the clock from the sound card is the textbook approach and it is
//! what this originally did. The problem is that the *observation* of that
//! clock arrives on the audio driver's callback thread, whose scheduling is at
//! the mercy of DPC latency: any unrelated driver misbehaving turns directly
//! into timing error in the picture. The device consumes samples at a perfectly
//! steady rate, but we cannot read that rate steadily, and a clock is only as
//! good as its worst reading.
//!
//! So the master is QPC: a hardware counter, monotonic, unaffected by anything
//! any driver does. Video is scheduled against it.
//!
//! That leaves one honest problem. The QPC crystal and the audio DAC's crystal
//! are different oscillators, typically tens of parts per million apart. Left
//! alone they diverge, and the audio buffer slowly fills or empties until it
//! either overflows or runs dry — minutes or hours in, but inevitably. The fix
//! is `nudge`: a very slow correction that trims the clock's rate to keep the
//! audio buffer at its target depth. The time constant is deliberately in the
//! tens of seconds, so instantaneous callback jitter is filtered out entirely
//! and only genuine crystal drift gets through.

use std::sync::Mutex;

#[cfg(windows)]
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

pub struct Clock {
    /// Counter ticks per second. Fixed at boot and never changes.
    frequency: f64,
    inner: Mutex<Inner>,
}

struct Inner {
    /// Counter value when the clock was last anchored.
    anchor_ticks: i64,
    /// Stream position at that instant, in seconds.
    anchor_pts: f64,
    /// Multiplier on real time. 1.0 exactly, give or take crystal drift.
    rate: f64,
    running: bool,
    started: bool,
}

fn now_ticks() -> i64 {
    #[cfg(windows)]
    {
        let mut ticks = 0i64;
        // Cannot fail on anything since Windows XP.
        unsafe { QueryPerformanceCounter(&mut ticks).ok() };
        ticks
    }
    #[cfg(not(windows))]
    {
        std::time::UNIX_EPOCH.elapsed().map(|d| d.as_nanos() as i64).unwrap_or(0)
    }
}

fn frequency() -> f64 {
    #[cfg(windows)]
    {
        let mut hz = 0i64;
        unsafe { QueryPerformanceFrequency(&mut hz).ok() };
        if hz > 0 { hz as f64 } else { 1.0 }
    }
    #[cfg(not(windows))]
    {
        1_000_000_000.0
    }
}

impl Clock {
    pub fn new() -> Self {
        Self {
            frequency: frequency(),
            inner: Mutex::new(Inner {
                anchor_ticks: now_ticks(),
                anchor_pts: 0.0,
                rate: 1.0,
                running: true,
                started: false,
            }),
        }
    }

    /// Stream position now, in seconds. Negative before the first frame.
    pub fn now(&self) -> f64 {
        let inner = self.inner.lock().unwrap();
        if !inner.started {
            return -1.0;
        }
        if !inner.running {
            return inner.anchor_pts;
        }
        let elapsed = (now_ticks() - inner.anchor_ticks) as f64 / self.frequency;
        inner.anchor_pts + elapsed * inner.rate
    }

    pub fn started(&self) -> bool {
        self.inner.lock().unwrap().started
    }

    /// Begin, from the first frame's timestamp. Does nothing once running.
    pub fn start(&self, pts: f64) {
        let mut inner = self.inner.lock().unwrap();
        if inner.started {
            return;
        }
        inner.anchor_pts = pts;
        inner.anchor_ticks = now_ticks();
        inner.started = true;
    }

    /// After a seek. The position is known exactly and any previous value is
    /// wrong, so this re-anchors rather than adjusting.
    pub fn reset(&self, pts: f64) {
        let mut inner = self.inner.lock().unwrap();
        inner.anchor_pts = pts;
        inner.anchor_ticks = now_ticks();
        inner.started = true;
    }

    pub fn set_running(&self, running: bool) {
        let mut inner = self.inner.lock().unwrap();
        if inner.running == running {
            return;
        }
        if inner.running {
            // Bank the elapsed time before stopping, or a pause silently
            // rewinds the clock by however long it was paused for.
            let elapsed = (now_ticks() - inner.anchor_ticks) as f64 / self.frequency;
            inner.anchor_pts += elapsed * inner.rate;
        }
        inner.anchor_ticks = now_ticks();
        inner.running = running;
    }

    /// Trim the clock's rate so the audio buffer holds steady.
    ///
    /// `error` is how far the buffer is from where it should be, in seconds:
    /// positive when it is fuller than intended, meaning the clock is running
    /// slow and audio is piling up.
    ///
    /// The correction is capped at 0.2%, which is far more than crystal drift
    /// ever needs and far too small to be audible or visible. Anything larger
    /// would be chasing a supply problem, and a clock is the wrong instrument
    /// for that.
    pub fn nudge(&self, error: f64) {
        const MAX_ADJUST: f64 = 0.002;
        const GAIN: f64 = 0.02;

        let mut inner = self.inner.lock().unwrap();
        if !inner.started || !inner.running {
            return;
        }

        // Re-anchor at the current position first, so changing the rate does
        // not retroactively rewrite where playback has already got to.
        let elapsed = (now_ticks() - inner.anchor_ticks) as f64 / self.frequency;
        inner.anchor_pts += elapsed * inner.rate;
        inner.anchor_ticks = now_ticks();

        let target = 1.0 + (error * GAIN).clamp(-MAX_ADJUST, MAX_ADJUST);
        // Move towards the target slowly. The audio buffer is noisy; reacting
        // to a single reading would turn that noise into pitch wobble.
        inner.rate = (inner.rate * 0.95 + target * 0.05).clamp(1.0 - MAX_ADJUST, 1.0 + MAX_ADJUST);
    }

    /// The current rate, for the stats readout. Should sit within a few parts
    /// per million of 1.0; anything further means the two clocks genuinely
    /// disagree, or something upstream is not delivering at real time.
    pub fn rate(&self) -> f64 {
        self.inner.lock().unwrap().rate
    }
}
