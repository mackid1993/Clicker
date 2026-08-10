// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native Windows client for Channels DVR
// Copyright (c) 2026 David Brustein

//! Offline downloads.
//!
//! This is a laptop-first client, and a laptop leaves the house. A download is
//! a copy of the recording's file fetched from the DVR to local disk, so it
//! plays on a plane exactly as it would at home — same player, same file
//! format, no server in the loop.
//!
//! Downloads live in `%LOCALAPPDATA%\Clicker\Downloads`, named by recording
//! id. The id is the join key back to the library's metadata, so nothing else
//! needs to be stored alongside the file.
//!
//! Two run at once and the rest wait. Asking a DVR for eight files at once
//! does not make any of them arrive sooner — it divides the same link eight
//! ways and makes the one being waited on the slowest of the eight — and the
//! DVR is also the thing serving live TV to the same household while it does
//! it.
//!
//! ## Resuming
//!
//! A transfer can be stopped and picked up again, including across a crash or
//! a quit. The partial file is kept, and the next attempt asks for the rest of
//! it with a `Range` header. Channels supports this properly — it answers
//! `206 Partial Content` with `Accept-Ranges: bytes` and the full length in
//! `Content-Range`, which was verified against a real server rather than
//! assumed, because the whole design rests on it.
//!
//! A three-gigabyte recording is twenty minutes of transfer on a good link.
//! Losing all of it to a closed lid, and having no way to do anything about it
//! but start again, is the difference between a feature and a nuisance.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

/// How many downloads run at once. The rest queue.
const MAX_ACTIVE: usize = 2;

/// What a running transfer has been asked to do.
const RUN: u8 = 0;
const PAUSE: u8 = 1;
const CANCEL: u8 = 2;

#[derive(Clone)]
pub enum Status {
    /// Accepted, waiting for a slot.
    Queued,
    /// Fraction complete, 0 to 1, or negative when the size is unknown.
    Active(f32),
    /// Stopped, with the partial file kept and resumable.
    ///
    /// The fraction is negative for one recovered at startup: the bytes on
    /// disk are known but the total is not, because nothing has asked the
    /// server how long the whole recording is since the process restarted.
    Paused(f32),
    Done(PathBuf),
    Failed(String),
}

impl Status {
    pub fn is_finished(&self) -> bool {
        matches!(self, Status::Done(_) | Status::Failed(_))
    }

    /// Whether starting it again would continue rather than begin.
    pub fn is_resumable(&self) -> bool {
        matches!(self, Status::Paused(_) | Status::Failed(_))
    }
}

/// How a transfer ended.
enum Outcome {
    Done(PathBuf),
    /// Stopped on request, partial file kept. Carries the fraction reached.
    Paused(f32),
    /// Abandoned on request, partial file deleted.
    Cancelled,
}

/// Everything the background tasks share. Held behind one `Arc` so a task can
/// start the next queued download when it finishes without the caller being
/// involved.
struct Inner {
    dir: PathBuf,
    states: Mutex<HashMap<String, Status>>,
    /// Set to make a running transfer pause or give up. Kept apart from
    /// `states` so the signal survives the state being replaced.
    signals: Mutex<HashMap<String, Arc<AtomicU8>>>,
    /// Waiting for a slot, oldest first.
    queue: Mutex<VecDeque<(String, String)>>,
    /// Ask the interface to redraw. Stored because the queue is pumped from a
    /// finishing task, which has no other way to reach the UI.
    repaint: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    http: reqwest::Client,
    runtime: tokio::runtime::Handle,
}

pub struct Downloads {
    inner: Arc<Inner>,
}

impl Downloads {
    pub fn new(runtime: tokio::runtime::Handle, dir: PathBuf) -> Self {

        let mut states = HashMap::new();
        // The directory is the source of truth, not a manifest that could
        // disagree with it.
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };

                // A .part is a transfer the process did not live to finish.
                // Kept, not swept: it is most of a file, the server serves
                // ranges, and throwing it away would mean starting a
                // three-gigabyte download again because a lid closed.
                if path.extension().is_some_and(|e| e == "part") {
                    states.insert(stem.to_string(), Status::Paused(-1.0));
                } else if path.extension().is_some_and(|e| e == "mpg") {
                    states.insert(stem.to_string(), Status::Done(path.clone()));
                }
            }
        }

        Self {
            inner: Arc::new(Inner {
                dir,
                states: Mutex::new(states),
                signals: Mutex::new(HashMap::new()),
                queue: Mutex::new(VecDeque::new()),
                repaint: Mutex::new(None),
                http: reqwest::Client::builder()
                    .user_agent(crate::settings::user_agent())
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new()),
                runtime,
            }),
        }
    }

    /// The local file for a recording, when a finished download exists.
    pub fn local_path(&self, id: &str) -> Option<PathBuf> {
        match self.inner.states.lock().unwrap().get(id) {
            Some(Status::Done(path)) => Some(path.clone()),
            _ => None,
        }
    }

    pub fn status(&self, id: &str) -> Option<Status> {
        self.inner.states.lock().unwrap().get(id).cloned()
    }

    /// How many downloads are currently running.
    pub fn active(&self) -> usize {
        self.inner.active()
    }

    /// Everything known about: running, then waiting, then paused, then
    /// finished.
    pub fn entries(&self) -> Vec<(String, Status)> {
        let states = self.inner.states.lock().unwrap();
        let mut all: Vec<(String, Status)> =
            states.iter().map(|(id, s)| (id.clone(), s.clone())).collect();
        drop(states);

        fn rank(status: &Status) -> u8 {
            match status {
                Status::Active(_) => 0,
                Status::Queued => 1,
                Status::Paused(_) => 2,
                Status::Failed(_) => 3,
                Status::Done(_) => 4,
            }
        }
        all.sort_by(|a, b| rank(&a.1).cmp(&rank(&b.1)).then_with(|| a.0.cmp(&b.0)));
        all
    }

    pub fn is_empty(&self) -> bool {
        self.inner.states.lock().unwrap().is_empty()
    }

    /// Stop a transfer but keep what has arrived.
    ///
    /// A queued one is paused outright rather than being left to start and
    /// stop again the moment a slot frees.
    pub fn pause(&self, id: &str) {
        if let Some(signal) = self.inner.signals.lock().unwrap().get(id) {
            signal.store(PAUSE, Ordering::SeqCst);
        }
        let mut queued = false;
        self.inner.queue.lock().unwrap().retain(|(waiting, _)| {
            let keep = waiting != id;
            queued |= !keep;
            keep
        });
        if queued {
            self.inner
                .states
                .lock()
                .unwrap()
                .insert(id.to_string(), Status::Paused(-1.0));
            self.inner.pump();
        }
    }

    /// Delete a finished download, or abandon one still running.
    ///
    /// One entry point for both because from the outside they are the same
    /// wish — "I do not want this" — and which of the two applies depends on
    /// timing the person clicking cannot see.
    pub fn remove(&self, id: &str) {
        // Raise the signal first. A running task checks it between chunks, and
        // deleting the state without it would leave the task writing to a file
        // nothing is tracking.
        if let Some(signal) = self.inner.signals.lock().unwrap().get(id) {
            signal.store(CANCEL, Ordering::SeqCst);
        }
        self.inner.queue.lock().unwrap().retain(|(queued, _)| queued != id);

        let previous = self.inner.states.lock().unwrap().remove(id);
        if let Some(Status::Done(path)) = previous {
            let _ = std::fs::remove_file(path);
        }
        // A partial file belongs to nothing now. Removing is the one place
        // that deletes one; pausing and crashing both keep it.
        let _ = std::fs::remove_file(self.inner.dir.join(format!("{id}.part")));

        self.inner.pump();
    }

    /// Forget everything that has finished or failed, deleting the files.
    pub fn clear_finished(&self) {
        let finished: Vec<String> = self
            .inner
            .states
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, s)| s.is_finished())
            .map(|(id, _)| id.clone())
            .collect();
        for id in finished {
            self.remove(&id);
        }
    }

    /// Start fetching a recording, or continue one that was stopped.
    ///
    /// The same call for both: whether this begins or resumes is decided by
    /// what is on disk, not by which button was pressed, so a caller cannot
    /// get it wrong.
    pub fn start(&self, id: &str, url: String, repaint: impl Fn() + Send + Sync + 'static) {
        {
            let mut states = self.inner.states.lock().unwrap();
            match states.get(id) {
                Some(Status::Done(_)) | Some(Status::Active(_)) | Some(Status::Queued) => return,
                _ => {}
            }
            states.insert(id.to_string(), Status::Queued);
        }
        *self.inner.repaint.lock().unwrap() = Some(Arc::new(repaint));
        self.inner
            .queue
            .lock()
            .unwrap()
            .push_back((id.to_string(), url));
        self.inner.pump();
    }
}

impl Inner {
    fn active(&self) -> usize {
        self.states
            .lock()
            .unwrap()
            .values()
            .filter(|s| matches!(s, Status::Active(_)))
            .count()
    }

    fn notify(&self) {
        let repaint = self.repaint.lock().unwrap().clone();
        if let Some(repaint) = repaint {
            repaint();
        }
    }

    /// Start whatever the concurrency limit has room for.
    fn pump(self: &Arc<Self>) {
        loop {
            if self.active() >= MAX_ACTIVE {
                break;
            }
            let Some((id, url)) = self.queue.lock().unwrap().pop_front() else { break };

            // Cancelled or paused between being queued and being reached.
            if !matches!(self.states.lock().unwrap().get(&id), Some(Status::Queued)) {
                continue;
            }

            let signal = Arc::new(AtomicU8::new(RUN));
            self.signals.lock().unwrap().insert(id.clone(), Arc::clone(&signal));
            self.states
                .lock()
                .unwrap()
                .insert(id.clone(), Status::Active(-1.0));

            let inner = Arc::clone(self);
            self.runtime.spawn(async move {
                let result = fetch(&inner, &url, &id, &signal).await;
                inner.signals.lock().unwrap().remove(&id);

                {
                    let mut states = inner.states.lock().unwrap();
                    // A cancelled download has already been forgotten by
                    // `remove`, and must not be resurrected as Failed.
                    if states.contains_key(&id) {
                        match result {
                            Ok(Outcome::Done(path)) => states.insert(id.clone(), Status::Done(path)),
                            Ok(Outcome::Paused(done)) => {
                                states.insert(id.clone(), Status::Paused(done))
                            }
                            Ok(Outcome::Cancelled) => states.remove(&id),
                            // Failed, not lost: whatever arrived is still on
                            // disk and pressing resume picks it up.
                            Err(e) => states.insert(id.clone(), Status::Failed(format!("{e:#}"))),
                        };
                    }
                }

                inner.notify();
                inner.pump();
            });
        }
        self.notify();
    }
}

/// Fetch one recording, continuing from whatever is already on disk.
async fn fetch(
    inner: &Arc<Inner>,
    url: &str,
    id: &str,
    signal: &Arc<AtomicU8>,
) -> Result<Outcome> {
    use tokio::io::AsyncWriteExt;

    std::fs::create_dir_all(&inner.dir).context("creating the downloads directory")?;

    // Written to a .part name and renamed on completion, so a file with the
    // real name is always a whole one. An interrupted download can never be
    // mistaken for a finished recording that mysteriously ends early.
    let partial = inner.dir.join(format!("{id}.part"));
    let done = inner.dir.join(format!("{id}.mpg"));

    let have = std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);

    let mut request = inner.http.get(url);
    if have > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let response = request
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("GET {url}"))?;

    // Whether the server honored the range decides both where writing starts
    // and what the total is. A server that ignores it answers 200 with the
    // whole file, and appending that to what is already there would produce a
    // corrupt file the size of one and a half recordings — so the only safe
    // reading of a 200 is "start again".
    let resumed = response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let (mut received, total) = if resumed {
        (have, content_range_total(&response).or_else(|| {
            response.content_length().map(|len| len + have)
        }))
    } else {
        (0, response.content_length())
    };

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(&partial)
        .await
        .with_context(|| format!("opening {}", partial.display()))?;

    let mut stream = response;
    let mut last_report = std::time::Instant::now();
    let fraction = |received: u64| {
        total
            .map(|t| (received as f32 / t as f32).clamp(0.0, 1.0))
            .unwrap_or(-1.0)
    };

    while let Some(chunk) = stream.chunk().await.context("reading the download")? {
        // Checked per chunk rather than per progress report: stopping should
        // stop the transfer, not finish the current quarter-second of it.
        match signal.load(Ordering::SeqCst) {
            PAUSE => {
                file.flush().await.ok();
                drop(file);
                return Ok(Outcome::Paused(fraction(received)));
            }
            CANCEL => {
                drop(file);
                let _ = tokio::fs::remove_file(&partial).await;
                return Ok(Outcome::Cancelled);
            }
            _ => {}
        }

        file.write_all(&chunk).await.context("writing the download")?;
        received += chunk.len() as u64;

        // Progress is throttled: updating shared state per chunk repaints the
        // interface hundreds of times a second for no visible benefit.
        if last_report.elapsed().as_millis() >= 250 {
            last_report = std::time::Instant::now();
            let mut states = inner.states.lock().unwrap();
            if states.contains_key(id) {
                states.insert(id.to_string(), Status::Active(fraction(received)));
            }
            drop(states);
            inner.notify();
        }
    }

    file.flush().await.ok();
    drop(file);

    match signal.load(Ordering::SeqCst) {
        PAUSE => return Ok(Outcome::Paused(fraction(received))),
        CANCEL => {
            let _ = tokio::fs::remove_file(&partial).await;
            return Ok(Outcome::Cancelled);
        }
        _ => {}
    }

    tokio::fs::rename(&partial, &done)
        .await
        .context("finishing the download")?;
    Ok(Outcome::Done(done))
}

/// The whole file's length, from `Content-Range: bytes 12-99/100`.
///
/// `Content-Length` on a partial response is the length of the *part*, so it
/// cannot be used as the total without adding back what was already held —
/// and this header states the answer outright.
fn content_range_total(response: &reqwest::Response) -> Option<u64> {
    let value = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)?
        .to_str()
        .ok()?;
    value.rsplit('/').next()?.trim().parse().ok()
}
