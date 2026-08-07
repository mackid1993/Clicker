//! Offline downloads.
//!
//! This is a laptop-first client, and a laptop leaves the house. A download is
//! a copy of the recording's file fetched from the DVR to local disk, so it
//! plays on a plane exactly as it would at home — same player, same file
//! format, no server in the loop.
//!
//! Downloads live in `%LOCALAPPDATA%\RustDVR\Downloads`, named by recording
//! id. The id is the join key back to the library's metadata, so nothing else
//! needs to be stored alongside the file: title, artwork and progress all come
//! from the server's records when online, and from nothing when offline — an
//! offline library listing is a problem for another day, stated honestly.
//!
//! Two run at once and the rest wait. Asking a DVR for eight files at once
//! does not make any of them arrive sooner — it divides the same link eight
//! ways and makes the one being waited on the slowest of the eight — and the
//! DVR is also the thing serving live TV to the same household while it does
//! it.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

/// How many downloads run at once. The rest queue.
const MAX_ACTIVE: usize = 2;

#[derive(Clone)]
pub enum Status {
    /// Accepted, waiting for a slot.
    Queued,
    /// Fraction complete, 0 to 1, or negative when the size is unknown.
    Active(f32),
    Done(PathBuf),
    Failed(String),
}

impl Status {
    pub fn is_finished(&self) -> bool {
        matches!(self, Status::Done(_) | Status::Failed(_))
    }
}

/// Everything the background tasks share. Held behind one `Arc` so a task can
/// start the next queued download when it finishes without the caller being
/// involved.
struct Inner {
    dir: PathBuf,
    states: Mutex<HashMap<String, Status>>,
    /// Raised to make a running download give up. Kept apart from `states` so
    /// the flag survives the state being replaced.
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
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
    pub fn new(runtime: tokio::runtime::Handle) -> Self {
        let dir = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("RustDVR")
            .join("Downloads");

        let mut states = HashMap::new();
        // Anything already on disk from an earlier session is a finished
        // download; the directory is the source of truth, not a manifest that
        // could disagree with it.
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                // A .part is the remains of a download interrupted by the
                // process going away. It is not resumable — the server is
                // asked for the whole file — so leaving it would only occupy
                // disk that nothing would ever claim.
                if path.extension().is_some_and(|e| e == "part") {
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
                if path.extension().is_some_and(|e| e == "mpg") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        states.insert(stem.to_string(), Status::Done(path.clone()));
                    }
                }
            }
        }

        Self {
            inner: Arc::new(Inner {
                dir,
                states: Mutex::new(states),
                cancels: Mutex::new(HashMap::new()),
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

    /// Everything known about, newest activity first: running, then waiting,
    /// then finished.
    pub fn entries(&self) -> Vec<(String, Status)> {
        let states = self.inner.states.lock().unwrap();
        let mut all: Vec<(String, Status)> =
            states.iter().map(|(id, s)| (id.clone(), s.clone())).collect();
        drop(states);

        fn rank(status: &Status) -> u8 {
            match status {
                Status::Active(_) => 0,
                Status::Queued => 1,
                Status::Failed(_) => 2,
                Status::Done(_) => 3,
            }
        }
        all.sort_by(|a, b| rank(&a.1).cmp(&rank(&b.1)).then_with(|| a.0.cmp(&b.0)));
        all
    }

    pub fn is_empty(&self) -> bool {
        self.inner.states.lock().unwrap().is_empty()
    }

    /// Delete a finished download, or abandon one still running.
    ///
    /// One entry point for both because from the outside they are the same
    /// wish — "I do not want this" — and which of the two applies depends on
    /// timing the person clicking cannot see.
    pub fn remove(&self, id: &str) {
        // Raise the flag first. A running task checks it between chunks, and
        // deleting the state without it would leave the task writing to a file
        // nothing is tracking.
        if let Some(flag) = self.inner.cancels.lock().unwrap().get(id) {
            flag.store(true, Ordering::SeqCst);
        }
        self.inner.queue.lock().unwrap().retain(|(queued, _)| queued != id);

        let previous = self.inner.states.lock().unwrap().remove(id);
        if let Some(Status::Done(path)) = previous {
            let _ = std::fs::remove_file(path);
        }
        // A partial file belongs to nothing now.
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

    /// Start fetching a recording. Does nothing if it is already local or
    /// already on its way.
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

            // Cancelled between being queued and being reached.
            if !matches!(self.states.lock().unwrap().get(&id), Some(Status::Queued)) {
                continue;
            }

            let flag = Arc::new(AtomicBool::new(false));
            self.cancels.lock().unwrap().insert(id.clone(), Arc::clone(&flag));
            self.states
                .lock()
                .unwrap()
                .insert(id.clone(), Status::Active(-1.0));

            let inner = Arc::clone(self);
            self.runtime.spawn(async move {
                let result = fetch(&inner, &url, &id, &flag).await;
                inner.cancels.lock().unwrap().remove(&id);

                {
                    let mut states = inner.states.lock().unwrap();
                    // A cancelled download has already been forgotten by
                    // `remove`, and must not be resurrected as Failed.
                    if states.contains_key(&id) {
                        match result {
                            Ok(Some(path)) => states.insert(id.clone(), Status::Done(path)),
                            Ok(None) => states.remove(&id),
                            Err(e) => {
                                states.insert(id.clone(), Status::Failed(format!("{e:#}")))
                            }
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

/// Fetch one recording. `Ok(None)` means it was cancelled.
async fn fetch(
    inner: &Arc<Inner>,
    url: &str,
    id: &str,
    cancelled: &Arc<AtomicBool>,
) -> Result<Option<PathBuf>> {
    use tokio::io::AsyncWriteExt;

    std::fs::create_dir_all(&inner.dir).context("creating the downloads directory")?;

    // Written to a .part name and renamed on completion, so a file with the
    // real name is always a whole one. An interrupted download can never be
    // mistaken for a finished recording that mysteriously ends early.
    let partial = inner.dir.join(format!("{id}.part"));
    let done = inner.dir.join(format!("{id}.mpg"));

    let response = inner
        .http
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("GET {url}"))?;

    let total = response.content_length();
    let mut file = tokio::fs::File::create(&partial)
        .await
        .with_context(|| format!("creating {}", partial.display()))?;

    let mut stream = response;
    let mut received: u64 = 0;
    let mut last_report = std::time::Instant::now();

    while let Some(chunk) = stream.chunk().await.context("reading the download")? {
        // Checked per chunk rather than per progress report: a cancel should
        // stop the transfer, not finish the current quarter-second of it.
        if cancelled.load(Ordering::SeqCst) {
            drop(file);
            let _ = tokio::fs::remove_file(&partial).await;
            return Ok(None);
        }

        file.write_all(&chunk).await.context("writing the download")?;
        received += chunk.len() as u64;

        // Progress is throttled: updating shared state per chunk repaints the
        // interface hundreds of times a second for no visible benefit.
        if last_report.elapsed().as_millis() >= 250 {
            last_report = std::time::Instant::now();
            let fraction = total
                .map(|t| (received as f32 / t as f32).clamp(0.0, 1.0))
                .unwrap_or(-1.0);
            let mut states = inner.states.lock().unwrap();
            if states.contains_key(id) {
                states.insert(id.to_string(), Status::Active(fraction));
            }
            drop(states);
            inner.notify();
        }
    }

    file.flush().await.ok();
    drop(file);

    if cancelled.load(Ordering::SeqCst) {
        let _ = tokio::fs::remove_file(&partial).await;
        return Ok(None);
    }

    tokio::fs::rename(&partial, &done)
        .await
        .context("finishing the download")?;
    Ok(Some(done))
}
