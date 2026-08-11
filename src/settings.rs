// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native Windows client for Channels DVR
// Copyright (c) 2026 David Brustein

//! Persisted settings, and finding a DVR.
//!
//! Stored beside the user's other application data rather than next to the
//! executable, so an installed copy in Program Files still works without
//! administrator rights and uninstalling leaves nothing behind in the program
//! directory.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

/// One configured DVR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    /// Base URL, including scheme and port.
    pub url: String,
    /// What the server calls itself, remembered from the last successful
    /// probe so the settings list is readable without contacting anything.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Every DVR that has been added. A household can easily have two — one
    /// for the aerial and one for a streaming source — and switching between
    /// them should not mean retyping an address.
    #[serde(default)]
    pub servers: Vec<Server>,
    /// Index into `servers`. Out of range is treated as none configured
    /// rather than as an error, so a hand-edited file cannot panic the app.
    #[serde(default)]
    pub active: usize,
    /// How this machine identifies itself to the server.
    pub client_name: String,
    /// Ask the server for the original stream rather than a transcode.
    #[serde(default = "yes")]
    pub original_quality: bool,
    /// Height to transcode down to when `original_quality` is off.
    #[serde(default = "default_height")]
    pub transcode_height: u32,
    #[serde(default = "default_kbps")]
    pub transcode_kbps: u32,
    /// How far the skip-back button and left arrow jump, in seconds.
    #[serde(default = "default_back")]
    pub skip_back_secs: u32,
    /// How far skip-forward and the right arrow jump, in seconds.
    #[serde(default = "default_forward")]
    pub skip_forward_secs: u32,
    /// Collections starred in the guide's own picker, by slug. Favorites sort
    /// to the top of the dropdown so the lineups someone actually uses stop
    /// hiding among the ones they never touch.
    #[serde(default)]
    pub favorite_collections: Vec<String>,
    /// The last collection picked in the guide. Restored on launch: choosing a
    /// collection IS choosing the default, with no separate setting to keep in
    /// agreement with it. Same for the source filter.
    #[serde(default)]
    pub last_collection: Option<String>,
    #[serde(default)]
    pub last_source: Option<String>,
    /// How the library's two tabs are ordered, restored on launch for the
    /// same reason as the collection above: picking a sort IS picking the
    /// default. One per tab, because "most unwatched" for series and "year"
    /// for films are both right and cannot share a slot.
    #[serde(default)]
    pub sort_shows: crate::library::Sort,
    #[serde(default)]
    pub sort_movies: crate::library::Sort,
    /// The Recorded tab and the downloads screen keep their own, with their
    /// own defaults: a recording list opens newest first, and a download list
    /// opens with whatever is moving on top.
    #[serde(default = "default_sort_recordings")]
    pub sort_recordings: crate::library::Sort,
    #[serde(default = "default_sort_downloads")]
    pub sort_downloads: crate::library::Sort,
    /// Suppress the "server version not approved" warning.
    ///
    /// Defaults to suppressed and is not exposed in the interface. Everyone
    /// runs the beta, so the warning fires for practically every user and
    /// tells them nothing they can act on. Kept as a field rather than deleted
    /// so an existing settings file still loads, and so it can be turned back
    /// on by hand if it ever earns its place.
    #[serde(default = "yes")]
    pub dismissed_version_warning: bool,
    /// Whether closing the window hides it to the notification area rather
    /// than exiting.
    ///
    /// Off by default. An application that ignores its own close button and
    /// leaves nothing on screen is indistinguishable from one that has crashed
    /// and leaked, and doing that uninvited is not a trade to make on someone
    /// else's behalf — it is worth having, but only once it has been asked
    /// for.
    #[serde(default)]
    pub minimize_to_tray: bool,
    /// How much disk the live buffer may use, in gigabytes. Zero turns it off.
    ///
    /// Original playback comes straight from the tuner, which cannot be
    /// rewound on its own, so the stream is written here as it arrives to give
    /// pause and rewind back. That is real disk — roughly 2GB an hour on a
    /// broadcast stream — and how much of it anyone is willing to spend is not
    /// a decision to make on their behalf. Off is a legitimate answer: live
    /// still plays, it simply cannot be rewound.
    #[serde(default = "default_live_buffer")]
    pub live_buffer_gb: u32,
    /// Where offline downloads are kept. Empty means beside the rest of the
    /// application's data.
    ///
    /// Both of these exist because the default is under the user profile, on
    /// whichever disk the system lives on, and neither of these is small: a
    /// download is the whole recording and the live buffer can be 32GB. Anyone
    /// with a second drive wants them on it, and no amount of room on the
    /// system disk makes that preference wrong.
    #[serde(default)]
    pub download_dir: String,
    /// Where the live buffer is written. Empty means the same.
    #[serde(default)]
    pub buffer_dir: String,
    /// Where everything this application caches or writes about itself goes:
    /// the library and guide caches, the player log and the crash log.
    ///
    /// Empty means the default beside the application's own data. None of it
    /// is small — a day of guide listings alone is twenty-four megabytes —
    /// and none of it has to live on the system drive.
    #[serde(default)]
    pub cache_dir: String,
    /// Decode on the processor rather than the graphics chip.
    ///
    /// Off by default, which lets the player use the integrated GPU's decoder.
    /// Worth turning on when a driver produces a picture with artifacts, which
    /// is the usual reason hardware decoding is wrong: the decode itself is
    /// correct or it is not, and when it is not, no setting above it helps.
    ///
    /// What it costs is processor time, and what that means depends on the
    /// machine: spare capacity on a desktop, battery on a laptop.
    ///
    /// The built-in pipeline ignores this and always decodes in software. That
    /// is not an oversight — it is why it costs 28% of a core where mpv's
    /// hardware path costs 72%: the frame has to come back from the GPU to be
    /// composited, and the trip back is the expensive part.
    #[serde(default)]
    pub software_decoding: bool,
    /// Whether keyboard shortcuts do anything at all.
    ///
    /// On by default, and turned off by its own shortcut, which keeps working
    /// so the switch can be reached again. Worth having for anyone who types
    /// into this on a machine that is also doing something else, and for the
    /// case where a rebind has gone wrong.
    #[serde(default = "yes")]
    pub shortcuts_enabled: bool,
    /// Rebound keys, by action id. Anything absent uses its default; an entry
    /// present but empty is an action deliberately left with no key.
    ///
    /// A map rather than a list so a settings file written by an older version
    /// keeps working when actions are added, and so hand-editing it is
    /// obvious. See `keys::ACTIONS`.
    #[serde(default)]
    pub shortcut_keys: std::collections::BTreeMap<String, String>,
    /// Where the window was when it was last closed.
    ///
    /// None until it has been opened once, which is what makes the first launch
    /// use the default size rather than something remembered from nowhere.
    #[serde(default)]
    pub window: Option<Window>,
}

/// A remembered window.
///
/// The rectangle is always the *restored* one — where the window sits when it
/// is not maximized. Storing the maximized rectangle instead would work until
/// someone un-maximized, at which point the window would restore to exactly
/// the size it already was and the button would appear to do nothing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Window {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub maximized: bool,
}

impl Window {
    /// Whether this is worth restoring.
    ///
    /// A saved position is only good while the monitor it was on still exists.
    /// Unplug the second screen and the remembered rectangle is somewhere the
    /// desktop no longer covers — Windows will place a window there quite
    /// happily, and the application starts up invisible with a taskbar button
    /// and no way to find it. So the rectangle has to overlap the desktop as it
    /// is *now*, by enough to grab.
    pub fn is_reachable(&self) -> bool {
        if !(self.width.is_finite() && self.height.is_finite()) || self.width < 200.0 || self.height < 150.0 {
            return false;
        }
        if !(self.x.is_finite() && self.y.is_finite()) {
            return false;
        }
        let Some((left, top, right, bottom)) = crate::platform::desktop_bounds() else {
            // No idea what the desktop looks like; assume it is fine rather
            // than throwing away a good position.
            return true;
        };
        // Horizontally, a title bar's worth of overlap is enough: the window
        // can be dragged back into view from either side.
        //
        // Vertically it is not, and the two are not symmetric. This window
        // draws its own caption across its top forty pixels, and that caption
        // is the only thing that can drag it — so a window whose top edge is
        // above the desktop has nothing left to grab and cannot be moved at
        // all. The top edge has to be on the desktop, not merely near it.
        const GRAB: f32 = 48.0;
        self.x + self.width - GRAB > left
            && self.x + GRAB < right
            && self.y >= top
            && self.y + GRAB < bottom
    }
}

fn default_live_buffer() -> u32 {
    4
}

fn yes() -> bool {
    true
}
fn default_height() -> u32 {
    720
}
fn default_kbps() -> u32 {
    4000
}
// The classic DVR asymmetry: a short hop back to catch a line of dialogue, a
// long one forward to clear a commercial.
fn default_back() -> u32 {
    15
}
fn default_forward() -> u32 {
    30
}
fn default_sort_recordings() -> crate::library::Sort {
    crate::library::Sort::Added
}
fn default_sort_downloads() -> crate::library::Sort {
    crate::library::Sort::Status
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            active: 0,
            client_name: hostname(),
            original_quality: true,
            transcode_height: default_height(),
            transcode_kbps: default_kbps(),
            skip_back_secs: default_back(),
            skip_forward_secs: default_forward(),
            favorite_collections: Vec::new(),
            last_collection: None,
            last_source: None,
            sort_shows: Default::default(),
            sort_movies: Default::default(),
            sort_recordings: default_sort_recordings(),
            sort_downloads: default_sort_downloads(),
            dismissed_version_warning: true,
            minimize_to_tray: false,
            live_buffer_gb: default_live_buffer(),
            download_dir: String::new(),
            buffer_dir: String::new(),
            cache_dir: String::new(),
            software_decoding: false,
            shortcuts_enabled: true,
            shortcut_keys: Default::default(),
            window: None,
        }
    }
}

impl Settings {
    /// Where downloads go, configured or default.
    pub fn download_path(&self) -> PathBuf {
        resolve(&self.download_dir, "Downloads")
    }

    /// Where the live buffer goes, configured or default.
    pub fn buffer_path(&self) -> PathBuf {
        resolve(&self.buffer_dir, "Timeshift")
    }

    /// Where the caches and logs go.
    ///
    /// Deliberately not built on `resolve`: that falls back to `data_dir()`,
    /// which is the very thing this decides.
    pub fn cache_path(&self) -> Option<PathBuf> {
        let configured = self.cache_dir.trim();
        (!configured.is_empty()).then(|| PathBuf::from(configured))
    }
}

/// A configured directory, or the default one beside the application's data.
///
/// The configured path is used as given rather than having a folder appended:
/// someone who points this at a directory called ClickerDownloads means that
/// directory, not a Downloads folder inside it.
fn resolve(configured: &str, default_leaf: &str) -> PathBuf {
    let configured = configured.trim();
    if !configured.is_empty() {
        return PathBuf::from(configured);
    }
    crate::paths::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(default_leaf)
}

/// Whether a directory can be written to, checked by writing to it.
///
/// Not by asking about permissions, which is a question with a long and
/// unreliable answer on every system: a path can be readable, exist, and still
/// refuse a write for reasons no attribute reports — an ACL on Windows, a
/// sandbox or a read-only mount elsewhere. Creating a file and removing it
/// again is the only test that means anything.
pub fn writable(dir: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating {}", dir.display()))?;
    let probe = dir.join(".clicker-write-test");
    std::fs::write(&probe, b"")
        .with_context(|| format!("writing to {}", dir.display()))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

impl Settings {
    /// Whether onboarding still needs to run.
    pub fn configured(&self) -> bool {
        self.active_server().is_some()
    }

    pub fn active_server(&self) -> Option<&Server> {
        self.servers.get(self.active)
    }

    /// The base URL to talk to, or empty when nothing is configured.
    pub fn server_url(&self) -> String {
        self.active_server().map(|s| s.url.clone()).unwrap_or_default()
    }

    /// Add a server, or select it if the address is already known. Returns the
    /// index it ended up at.
    pub fn add_server(&mut self, server: Server) -> usize {
        match self.servers.iter().position(|s| s.url == server.url) {
            Some(index) => {
                // Refresh the remembered name and version; the server may have
                // been renamed or updated since it was added.
                self.servers[index].name = server.name;
                self.servers[index].version = server.version;
                self.active = index;
                index
            }
            None => {
                self.servers.push(server);
                self.active = self.servers.len() - 1;
                self.active
            }
        }
    }

    pub fn remove_server(&mut self, index: usize) {
        if index >= self.servers.len() {
            return;
        }
        self.servers.remove(index);
        // Keep the selection pointing at something that exists.
        if self.active >= self.servers.len() {
            self.active = self.servers.len().saturating_sub(1);
        }
    }

    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        // A settings file that has been hand-edited into nonsense should not
        // stop the application starting.
        serde_json::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        // Kept in step here rather than at every call site, so a device name
        // edited in settings applies to the next request rather than the next
        // launch.
        set_user_agent(&self.client_name);

        let path = config_path().ok_or_else(|| anyhow!("no application data directory"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("creating the settings directory")?;
        }
        let text = serde_json::to_string_pretty(self).context("serializing settings")?;
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}

fn config_path() -> Option<PathBuf> {
    Some(crate::paths::config_dir()?.join("settings.json"))
}

/// This machine's name, as the default client name.
///
/// A DVR shows connected clients by name, and "DESKTOP-4F2K1A" is at least
/// recognizable, where a blank or a generic default is not.
pub fn hostname() -> String {
    crate::platform::machine_name().unwrap_or_else(|| crate::APP_NAME.to_string())
}

/// What a server said when asked to identify itself.
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub version: String,
    pub name: String,
}

/// Channels' default port. Only used when nothing else is given — a port typed
/// into the address always wins, and reverse proxies routinely put a DVR
/// somewhere else entirely.
pub const DEFAULT_PORT: u16 = 8089;

/// Normalize whatever was typed into a URL worth trying.
///
/// All of these should work, because all of them are what people actually
/// type: `192.168.1.50`, `192.168.1.50:9000`, `dvr.example.com`,
/// `http://dvr.local:8089`, `https://dvr.example.com/channels`.
///
/// The port is only filled in when the address does not carry one. Assuming
/// 8089 unconditionally would break every setup behind a reverse proxy, which
/// is a common way to run this.
pub fn normalize(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }

    let secure = trimmed.starts_with("https://");
    let with_scheme = if secure || trimmed.starts_with("http://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };

    let after_scheme = with_scheme.splitn(2, "//").nth(1).unwrap_or("");
    let (host, path) = match after_scheme.find('/') {
        Some(index) => (&after_scheme[..index], &after_scheme[index..]),
        None => (after_scheme, ""),
    };

    // An IPv6 literal is bracketed, and the colons inside it are not a port.
    let has_port = if host.starts_with('[') {
        host.rsplit_once(']').map(|(_, rest)| rest.starts_with(':')).unwrap_or(false)
    } else {
        host.contains(':')
    };

    if has_port {
        with_scheme
    } else {
        // https implies 443 and a proxy that already knows where to send it;
        // adding Channels' port there would almost certainly be wrong.
        let scheme = if secure { "https" } else { "http" };
        if secure {
            format!("{scheme}://{host}{path}")
        } else {
            format!("{scheme}://{host}:{DEFAULT_PORT}{path}")
        }
    }
}

/// Ask a candidate address whether it is a Channels DVR.
/// The User-Agent every request carries, device name included.
///
/// Process-wide because it identifies this installation rather than any one
/// request, and behind a lock rather than a `OnceLock` because the name is a
/// setting and settings change while the program is running.
static USER_AGENT: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

/// `Clicker/0.0.2 (Living Room PC)`.
///
/// The device name is here and nowhere else because this is the only place the
/// server will take one. Channels identifies a streaming client by its IP
/// address: verified directly against a DVR, which ignored the User-Agent and
/// every one of `client`, `client_name`, `device`, `device_name`, `name` and
/// `player` as query parameters, keying its activity on the address in every
/// case. So the name cannot reach the client list, but it does reach the logs,
/// which is worth more than a setting that goes nowhere at all.
pub fn user_agent() -> String {
    USER_AGENT
        .read()
        .ok()
        .and_then(|held| held.clone())
        .unwrap_or_else(|| format!("{}/{}", crate::APP_NAME, env!("CARGO_PKG_VERSION")))
}

/// Rebuild it from the settings. Called at startup and whenever they are saved.
pub fn set_user_agent(client_name: &str) {
    let version = env!("CARGO_PKG_VERSION");
    let name = client_name.trim();
    // Anything that would break the header, dropped. A device name is free
    // text and someone will eventually put a newline in it.
    let name: String = name
        .chars()
        .filter(|c| !c.is_control() && *c != '(' && *c != ')')
        .collect();

    let app = crate::APP_NAME;
    let agent = if name.is_empty() {
        format!("{app}/{version}")
    } else {
        format!("{app}/{version} ({name})")
    };
    if let Ok(mut held) = USER_AGENT.write() {
        *held = Some(agent);
    }
}

pub async fn probe(url: &str) -> Result<ServerInfo> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .user_agent(user_agent())
        .build()?;

    let status: serde_json::Value = http
        .get(format!("{url}/dvr"))
        .send()
        .await
        .with_context(|| format!("could not reach {url}"))?
        .error_for_status()
        .with_context(|| format!("{url} answered, but not as a DVR"))?
        .json()
        .await
        .context("the response was not what a Channels DVR returns")?;

    let version = status
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let name = status
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Channels DVR")
        .to_string();

    Ok(ServerInfo { version, name })
}
