//! The live buffer.
//!
//! Direct playback asks the tuner for its transport stream and nothing else —
//! no server-side pipeline, which is the whole point of it. The cost is that
//! the response is one endless HTTP body with no length and no ranges, so
//! there is nothing to seek within and pause and rewind are impossible on it.
//!
//! That is not an acceptable trade for live television, so the stream is
//! written here as it arrives and the player opens the file rather than the
//! socket. A file on disk is seekable by definition, which gives back the
//! whole of timeshift: pause, rewind, and scrub anywhere from the moment the
//! channel was tuned.
//!
//! FFmpeg's own `cache:` protocol was tried first and does not do this. It
//! caches the body, but the underlying `URLContext` stays unseekable, so
//! `pb->seekable` remains 0 and the MPEG-TS demuxer refuses to binary-search
//! for a timestamp — measured: every seek came back `REFUSED by demuxer`.
//! Owning the file is what makes the difference.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How large the buffer is allowed to grow before writing stops.
///
/// A direct broadcast stream is around 5 Mbit, so this is roughly two hours.
/// Stopping is not the same as failing: everything already written stays
/// seekable and keeps playing, and only the live edge stops advancing —
/// which is a far better failure than filling someone's disk.
const MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;

pub struct Timeshift {
    path: PathBuf,
    written: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

impl Drop for Timeshift {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // The buffer is worthless once nothing is playing from it, and it is
        // measured in gigabytes. Deleting on the way out means a crash is the
        // only way to leave one behind, and the next start sweeps those.
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Timeshift {
    /// Begin writing a live stream to disk. Returns immediately; the file
    /// grows behind it.
    pub fn start(
        runtime: &tokio::runtime::Handle,
        http: reqwest::Client,
        url: String,
        channel: &str,
    ) -> std::io::Result<Self> {
        let dir = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("RustDVR")
            .join("Timeshift");
        std::fs::create_dir_all(&dir)?;

        // Named for the channel and this process, so two windows on the same
        // channel cannot write into one another's buffer.
        let path = dir.join(format!("ch{channel}-{}.ts", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let written = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));

        let task_path = path.clone();
        let task_written = Arc::clone(&written);
        let task_stop = Arc::clone(&stop);
        runtime.spawn(async move {
            use tokio::io::AsyncWriteExt;

            let mut file = match tokio::fs::File::create(&task_path).await {
                Ok(file) => file,
                Err(e) => {
                    eprintln!("[timeshift] could not create the buffer: {e}");
                    return;
                }
            };

            let mut response = match http.get(&url).send().await.and_then(|r| r.error_for_status())
            {
                Ok(response) => response,
                Err(e) => {
                    eprintln!("[timeshift] could not open the stream: {e}");
                    return;
                }
            };

            let mut total: u64 = 0;
            loop {
                if task_stop.load(Ordering::SeqCst) {
                    break;
                }
                match response.chunk().await {
                    Ok(Some(chunk)) => {
                        if file.write_all(&chunk).await.is_err() {
                            break;
                        }
                        total += chunk.len() as u64;
                        // Published only after the write, so the player never
                        // seeks to a byte that has not landed.
                        task_written.store(total, Ordering::SeqCst);
                        if total >= MAX_BYTES {
                            eprintln!("[timeshift] buffer full at {total} bytes; live stops here");
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        eprintln!("[timeshift] stream ended: {e}");
                        break;
                    }
                }
            }
            let _ = file.flush().await;
        });

        Ok(Self {
            path,
            written,
            stop,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn bytes(&self) -> u64 {
        self.written.load(Ordering::SeqCst)
    }

    /// Block until there is enough on disk to open, or give up.
    ///
    /// Opening an empty file gets nothing but "invalid data": the demuxer
    /// needs enough of a transport stream to find its programs and probe the
    /// codecs. This is the one place the caller has to wait, and it happens on
    /// the open thread rather than the interface's.
    pub fn wait_for(&self, bytes: u64, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.bytes() >= bytes {
                return true;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        self.bytes() >= bytes
    }
}

/// Delete buffers left behind by a process that did not get to clean up.
///
/// Called at startup. A crash mid-watch can leave gigabytes in this directory,
/// and nothing else will ever claim them.
pub fn sweep() {
    let Some(base) = std::env::var_os("LOCALAPPDATA") else { return };
    let dir = PathBuf::from(base).join("RustDVR").join("Timeshift");
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "ts") {
            let _ = std::fs::remove_file(path);
        }
    }
}
