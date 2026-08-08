//! Audio output, and the clock the picture is scheduled against.
//!
//! The sound card is the only honest clock in a media player. It consumes
//! samples at exactly its own rate and cannot be persuaded to do otherwise, so
//! whatever it has played is, by definition, where playback has got to. Video
//! is then presented to match. The alternative, driving the picture from a
//! timer and hoping the audio keeps up, is what produces a player that is fine
//! for a minute and half a second out of sync by the end of a film.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::Shared;

/// The chosen output device, before the stream is built.
///
/// Resolved before FFmpeg is opened so the decoder can resample straight to
/// the device's rate and channel count. Converting to a fixed internal format
/// and letting the device convert again would be two resamples where one does.
pub struct Device {
    device: cpal::Device,
    config: cpal::StreamConfig,
    pub sample_rate: u32,
    pub channels: u16,
}

impl Device {
    pub fn open() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("no audio output device"))?;
        let supported = device
            .default_output_config()
            .context("no default output configuration")?;

        // Stereo is what a television mix is authored for, and downmixing in
        // swresample is better than asking a device to do it.
        let channels = supported.channels().min(2).max(1);
        let sample_rate = supported.sample_rate().0;

        Ok(Self {
            device,
            config: cpal::StreamConfig {
                channels,
                sample_rate: cpal::SampleRate(sample_rate),
                buffer_size: cpal::BufferSize::Default,
            },
            sample_rate,
            channels,
        })
    }

    /// Build the output stream on a thread of its own.
    ///
    /// `cpal::Stream` is not `Send`: it has to be created, held and dropped on
    /// one thread. Keeping it in the `Player` would make the player itself
    /// unsendable and infect everything holding one. Giving it a thread also
    /// means the device's lifetime is not tied to the UI thread's, so a stall
    /// in the interface cannot take the sound with it.
    pub fn start(self, shared: Arc<Shared>) -> Result<Output> {
        let channels = self.channels as usize;
        let rate = self.sample_rate as f64;
        let (commands, orders) = std::sync::mpsc::channel::<Order>();
        let (ready, started) = std::sync::mpsc::channel::<Result<(), String>>();

        let thread = std::thread::Builder::new()
            .name("clicker-audio".into())
            .spawn(move || {
                let built = self
                    .device
                    .build_output_stream(
                        &self.config,
                        move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                            // How far in the future this buffer will actually
                            // be heard. The samples being written now are not
                            // audible until a buffer period plus whatever the
                            // device holds, so anchoring the clock to "now"
                            // would run the picture ahead of the sound by that
                            // much. The driver knows the number; ask it.
                            let lead = info
                                .timestamp()
                                .playback
                                .duration_since(&info.timestamp().callback)
                                .map(|d| d.as_secs_f64())
                                .unwrap_or(0.0);
                            fill(&shared, data, channels, rate, lead);
                        },
                        |err| eprintln!("[audio] {err}"),
                        None,
                    )
                    .context("could not open the audio output stream")
                    .and_then(|stream| {
                        stream.play().context("could not start audio")?;
                        Ok(stream)
                    });

                let stream = match built {
                    Ok(stream) => {
                        let _ = ready.send(Ok(()));
                        stream
                    }
                    Err(e) => {
                        let _ = ready.send(Err(format!("{e:#}")));
                        return;
                    }
                };

                // Park here holding the stream. Dropping it stops the device,
                // so this thread existing is what keeps audio alive.
                while let Ok(order) = orders.recv() {
                    match order {
                        Order::Pause(true) => { let _ = stream.pause(); }
                        Order::Pause(false) => { let _ = stream.play(); }
                        Order::Quit => break,
                    }
                }
            })
            .context("could not start the audio thread")?;

        match started.recv() {
            Ok(Ok(())) => Ok(Output {
                commands,
                thread: Some(thread),
            }),
            Ok(Err(e)) => Err(anyhow!(e)),
            Err(_) => Err(anyhow!("the audio thread stopped before it started")),
        }
    }
}

enum Order {
    Pause(bool),
    Quit,
}

/// The audio callback.
///
/// Runs on a real-time thread, so it does the least it can: take the queue
/// lock, memcpy, release. No allocation, no I/O, no decoding.
fn fill(shared: &Shared, data: &mut [f32], channels: usize, rate: f64, lead: f64) {
    let volume = shared.volume();
    let paused = shared.paused.load(Ordering::Relaxed);

    if paused {
        data.fill(0.0);
        return;
    }

    let mut written = 0usize;
    // The position of the first sample in this buffer. That is what the
    // speakers are about to play, so it is what the clock should read.
    let mut buffer_start: Option<f64> = None;

    {
        let mut queue = shared.audio.lock().unwrap();
        while written < data.len() {
            let Some(chunk) = queue.front_mut() else { break };
            let available = chunk.samples.len() - chunk.consumed;
            if available == 0 {
                queue.pop_front();
                continue;
            }

            if buffer_start.is_none() {
                let offset = chunk.consumed as f64 / (channels as f64 * rate);
                buffer_start = Some(chunk.pts + offset);
            }

            let take = available.min(data.len() - written);
            let src = &chunk.samples[chunk.consumed..chunk.consumed + take];
            let dst = &mut data[written..written + take];
            if volume >= 0.999 {
                dst.copy_from_slice(src);
            } else {
                for (out, sample) in dst.iter_mut().zip(src) {
                    *out = sample * volume;
                }
            }

            chunk.consumed += take;
            written += take;
            if chunk.consumed >= chunk.samples.len() {
                queue.pop_front();
            }
        }
    }

    if written > 0 {
        // Mirrors the counter the decode thread increments, so it can decide
        // whether to keep decoding without walking the queue under this lock.
        shared
            .audio_buffered
            .fetch_sub(written.min(shared.audio_buffered.load(Ordering::Relaxed)), Ordering::Relaxed);
    }

    // Underrun. Silence is the only correct thing to send; repeating the last
    // buffer would be audible as a click or a stutter.
    //
    // Counted, because "audio is glitching" and "the decoder cannot keep the
    // queue fed" are different faults with the same symptom, and only this
    // number tells them apart.
    if written < data.len() {
        data[written..].fill(0.0);
        shared.underruns.fetch_add(1, Ordering::Relaxed);
    }

    // The clock is deliberately NOT set here.
    //
    // This callback runs on the audio device's thread, and how often it runs
    // is at the mercy of driver DPC latency. Deriving the master clock from it
    // means every scheduling hiccup in an unrelated driver becomes a timing
    // error in the picture. Playback is timed against a monotonic clock
    // instead; see player::present_loop.
    //
    // What is published is not a clock but a *reading*: which timestamp is
    // coming out of the speakers at this instant. The samples written here are
    // not audible until the device has worked through what it already holds,
    // and the driver will say how long that is, so the audible position is the
    // first sample's timestamp less that lead. The decode thread compares it
    // against the monotonic clock once every two seconds and trims the clock's
    // rate by fractions of a percent. That is a slow correction driven by an
    // occasional observation, not a clock; the jitter in when this callback
    // runs is filtered out entirely by the time constant, which is the whole
    // reason for reading it this way round.
    //
    // Only published when the buffer was filled completely. A short buffer
    // means the queue ran out, and the timestamp of the last thing scraped out
    // of an empty queue describes nothing.
    if let Some(start) = buffer_start {
        if written == data.len() {
            shared
                .audible_pts
                .store((start - lead).to_bits(), Ordering::Relaxed);
        }
    }
}

pub struct Output {
    commands: std::sync::mpsc::Sender<Order>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Output {
    pub fn set_paused(&self, paused: bool) {
        // Not actionable if it fails: the callback already writes silence when
        // paused, so the worst case is a device that keeps running quietly.
        let _ = self.commands.send(Order::Pause(paused));
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        let _ = self.commands.send(Order::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
