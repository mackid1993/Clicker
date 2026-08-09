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

/// How much is released at a time once the window is full.
///
/// Punching a hole is a system call, so it is not done per chunk. A quarter of
/// a gigabyte is a few seconds of stream and a handful of calls an hour.
const RELEASE_STEP: u64 = 256 * 1024 * 1024;

pub struct Timeshift {
    path: PathBuf,
    written: Arc<AtomicU64>,
    /// Bytes at the front of the file that have been released back to the disk
    /// and can no longer be read. See `discarded_fraction`.
    discarded: Arc<AtomicU64>,
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
    /// `keep_bytes` is the rolling window: how much of the recent past stays
    /// readable. Writing never stops — once the window is full, the oldest
    /// part of the file is released back to the disk and the newest keeps
    /// arriving, so a channel can be left on all day inside a fixed budget.
    pub fn start(
        runtime: &tokio::runtime::Handle,
        http: reqwest::Client,
        url: String,
        channel: &str,
        keep_bytes: u64,
        dir: PathBuf,
    ) -> std::io::Result<Self> {
        std::fs::create_dir_all(&dir)?;

        // Named for the channel and this process, so two windows on the same
        // channel cannot write into one another's buffer.
        let path = dir.join(format!("ch{channel}-{}.ts", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let written = Arc::new(AtomicU64::new(0));
        let discarded = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));

        let task_path = path.clone();
        let task_written = Arc::clone(&written);
        let task_discarded = Arc::clone(&discarded);
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
            // Sparse, so the released part of it stops occupying disk. Without
            // this the file keeps every byte it was ever given and the window
            // bounds nothing.
            make_sparse(&file);

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

                        // Recycle rather than stop. The file keeps growing, so
                        // every byte offset the demuxer holds stays valid —
                        // truncating the front instead would move all of them
                        // underneath a read in progress. What is released is
                        // the disk behind the oldest part of it, which on a
                        // sparse file costs nothing to keep addressable.
                        let gone = task_discarded.load(Ordering::SeqCst);
                        if keep_bytes > 0 && total.saturating_sub(gone) > keep_bytes + RELEASE_STEP
                        {
                            let upto = total - keep_bytes;
                            if release(&file, gone, upto - gone) {
                                task_discarded.store(upto, Ordering::SeqCst);
                            }
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
            discarded,
            stop,
        })
    }

    /// How much of the buffer has been released, as a fraction of all of it.
    ///
    /// The player uses this to move the start of its seekable window forward.
    /// Bytes rather than time because that is what is actually known here —
    /// close enough on a broadcast stream, whose bitrate barely moves, and the
    /// alternative is an index this does not need for anything else.
    pub fn discarded_fraction(&self) -> f64 {
        let written = self.written.load(Ordering::SeqCst);
        if written == 0 {
            return 0.0;
        }
        self.discarded.load(Ordering::SeqCst) as f64 / written as f64
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

/// Mark the buffer sparse, so regions can later be given back to the disk.
///
/// Best effort. A file system that will not do this — FAT32 on a removable
/// disk, say — still works; the buffer simply keeps everything it is given and
/// the window bounds addressability rather than bytes on disk.
#[cfg(windows)]
fn make_sparse(file: &tokio::fs::File) {
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
#[cfg(windows)]
fn release(file: &tokio::fs::File, from: u64, len: u64) -> bool {
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

#[cfg(not(windows))]
fn make_sparse(_file: &tokio::fs::File) {}

#[cfg(not(windows))]
fn release(_file: &tokio::fs::File, _from: u64, _len: u64) -> bool {
    false
}

/// Delete buffers left behind by a process that did not get to clean up.
///
/// Called at startup. A crash mid-watch can leave gigabytes in this directory,
/// and nothing else will ever claim them.
pub fn sweep(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "ts") {
            let _ = std::fs::remove_file(path);
        }
    }
}
