// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial client for Channels DVR Server
// Copyright (c) 2026 David Brustein

//! The recorded library, and the shape of a home screen.
//!
//! Channels' `/api/v1/all` returns every recording with everything needed to
//! decide what to put in front of someone: how far through it they are, whether
//! they finished it, when it was recorded, artwork, and the commercial
//! markers. `/dvr/groups` adds the per-series view, including which episode is
//! next. Between them there is no need to invent a watch history or track
//! anything locally — the server already knows, and it knows across every
//! client, which a local history never would.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// One recording.
///
/// Field names follow the API rather than Rust convention because they are
/// deserialized straight from it; renaming every one of forty fields buys
/// nothing.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Recording {
    pub id: String,
    #[serde(default)]
    pub show_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub episode_title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub season_number: u32,
    #[serde(default)]
    pub episode_number: u32,
    #[serde(default)]
    pub image_url: String,
    /// A frame grabbed from the recording itself, served by the DVR. Better
    /// than the series poster for something half watched, because it shows
    /// where you actually are.
    #[serde(default)]
    pub thumbnail_url: String,
    #[serde(default)]
    pub duration: f64,
    /// How far in, in seconds. Non-zero means started but not finished.
    #[serde(default)]
    pub playback_time: f64,
    #[serde(default)]
    pub original_air_date: String,
    /// When this was last played, in milliseconds since the epoch.
    ///
    /// The field Continue Watching is actually about, and one this client
    /// ignored for a long time in favor of `updated_at`. They are not the
    /// same thing: the server bumps `updated_at` for its own housekeeping, so
    /// on a real library twenty part-watched recordings shared one minute on
    /// one night, and a documentary nobody had got 4% into outranked an
    /// episode somebody was a third of the way through.
    #[serde(default)]
    pub last_watched_at: i64,
    /// The year, for anything that has one. Films carry this where episodes
    /// carry an air date.
    #[serde(default)]
    pub release_year: i64,
    /// The longer description, where the server has both.
    #[serde(default)]
    pub full_summary: String,
    #[serde(default)]
    pub cast: Vec<String>,
    #[serde(default)]
    pub directors: Vec<String>,
    #[serde(default)]
    pub watched: bool,
    #[serde(default)]
    pub favorited: bool,
    #[serde(default)]
    pub completed: bool,
    /// The API spells this with two Ls. Without the rename the field never
    /// matched, so it was always false and `playable` never excluded anything
    /// on account of it.
    #[serde(default, rename = "cancelled")]
    pub canceled: bool,
    #[serde(default)]
    pub corrupted: bool,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub content_rating: String,
    /// Commercial boundaries in seconds, as found by the DVR's comskip pass.
    /// Alternating start and end of each break.
    #[serde(default)]
    pub commercials: Vec<f64>,
    /// Milliseconds since the epoch.
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub channel: String,
}

impl Recording {
    /// Whether this DVR actually recorded it, as opposed to it being external
    /// media imported into Channels.
    ///
    /// The `channel` field is the discriminator: something recorded off a
    /// tuner knows which channel it came from, and something imported from a
    /// Plex-style library does not. The file path is not a safe test — the
    /// DVR's own recording directory is configurable and imported libraries
    /// sit anywhere the user put them.
    ///
    /// The distinction is not cosmetic. On a real server this is 303
    /// recordings against 7,233 imports, and treating them as one pile makes
    /// the Recordings screen useless.
    pub fn from_dvr(&self) -> bool {
        !self.channel.trim().is_empty()
    }

    /// A movie, as opposed to an episode of something.
    pub fn is_movie(&self) -> bool {
        self.categories.iter().any(|c| c.eq_ignore_ascii_case("Movie"))
            || (self.season_number == 0 && self.episode_number == 0 && self.show_id.is_empty())
    }

    /// Fraction watched, 0 to 1.
    pub fn progress(&self) -> f32 {
        if self.duration <= 0.0 {
            return 0.0;
        }
        (self.playback_time / self.duration).clamp(0.0, 1.0) as f32
    }

    /// Started, but not finished and not marked watched.
    ///
    /// The lower bound exists because a few seconds of playback is someone
    /// checking what a recording is, not settling in to watch it, and a home
    /// screen full of those is noise. The upper bound is the credits: past
    /// ninety five percent it is finished in every sense that matters.
    pub fn in_progress(&self) -> bool {
        // Anything the server has a position for, which is what Channels
        // itself shows.
        //
        // This used to require a minute of playback, on the reasoning that a
        // few seconds is an accident rather than something being watched. The
        // reasoning was fine and the result was wrong: Continue Watching
        // silently disagreed with the Channels web interface, missing whatever
        // had been started and left inside the first minute. A client's job
        // here is to show the server's answer, not a tidier one of its own.
        !self.watched && self.duration > 0.0 && self.playback_time > 0.0 && self.progress() < 0.95
    }

    /// What is left, as human text.
    pub fn remaining(&self) -> String {
        let left = (self.duration - self.playback_time).max(0.0);
        let minutes = (left / 60.0).round() as i64;
        if minutes >= 60 {
            format!("{}h {}m left", minutes / 60, minutes % 60)
        } else {
            format!("{minutes}m left")
        }
    }

    /// The year this is from, if the server knows one.
    ///
    /// `release_year` for films, the air date's year for anything broadcast.
    /// Both are absent often enough that this returns an option rather than a
    /// zero nobody would want printed.
    pub fn year(&self) -> Option<i64> {
        if self.release_year > 1800 {
            return Some(self.release_year);
        }
        self.original_air_date
            .get(..4)
            .and_then(|y| y.parse::<i64>().ok())
            .filter(|y| *y > 1800)
    }

    /// "S24E142", or empty for anything that is not episodic.
    pub fn episode_label(&self) -> String {
        if self.season_number > 0 && self.episode_number > 0 {
            format!("S{}E{}", self.season_number, self.episode_number)
        } else {
            String::new()
        }
    }

    /// The line under the title.
    pub fn subtitle(&self) -> String {
        let episode = self.episode_label();
        match (episode.is_empty(), self.episode_title.trim().is_empty()) {
            (false, false) => format!("{episode}  ·  {}", self.episode_title),
            (false, true) => episode,
            (true, false) => self.episode_title.clone(),
            (true, true) => String::new(),
        }
    }

    /// Whichever image best represents it. A part-watched recording gets its
    /// own frame; anything else gets the series artwork.
    pub fn art(&self) -> &str {
        if self.playback_time > 0.0 && !self.thumbnail_url.is_empty() {
            &self.thumbnail_url
        } else if !self.image_url.is_empty() {
            &self.image_url
        } else {
            &self.thumbnail_url
        }
    }

    /// Whether this recording is worth showing at all.
    pub fn playable(&self) -> bool {
        self.completed && !self.canceled && !self.corrupted
    }
}

/// A series, as the DVR groups it.
///
/// Built field by field from the raw JSON rather than derived, deliberately.
/// A derived `Deserialize` fails the entire struct on one unexpected type or
/// one explicit null — and because the caller treats a failure as "no series",
/// the whole Library silently rendered empty with 86 perfectly good groups
/// sitting in the response. Reading each field defensively means one odd value
/// costs that field, not the screen.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub series_id: String,
    pub image: String,
    pub unwatched: u32,
    /// The episode to play next in this series. The server works this out, so
    /// it stays right no matter which device did the watching.
    pub up_next: String,
}

/// A string from JSON that may be a string, a number, or absent.
fn loose_str(value: &serde_json::Value, key: &str) -> String {
    match value.get(key) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

impl Group {
    fn from_json(value: &serde_json::Value) -> Self {
        Self {
            id: loose_str(value, "ID"),
            name: loose_str(value, "Name"),
            series_id: loose_str(value, "SeriesID"),
            image: loose_str(value, "Image"),
            unwatched: value
                .get("NumUnwatched")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32,
            up_next: loose_str(value, "UpNextFileID"),
        }
    }

    pub fn up_next_id(&self) -> Option<String> {
        (!self.up_next.is_empty()).then(|| self.up_next.clone())
    }

    /// This series' entry in [`Home::series_stats`].
    ///
    /// Recordings point at their group by either of the two ids a group
    /// carries, depending on where the DVR got them, so both are tried.
    pub fn stats(&self, stats: &std::collections::HashMap<&str, (usize, i64)>) -> (usize, i64) {
        stats
            .get(self.id.as_str())
            .or_else(|| stats.get(self.series_id.as_str()))
            .copied()
            .unwrap_or((0, 0))
    }
}

/// One ordering of the library, picked from the sort menu beside the search.
///
/// One enum for both tabs rather than one each: "A to Z" means the same thing
/// wherever it appears, and each tab simply offers the variants that mean
/// something for what it shows — a film has a year and a length, a series has
/// unwatched episodes and a newest recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Sort {
    #[default]
    NameAZ,
    NameZA,
    /// Newest year first. Films without one go last, not first: a missing
    /// year is not a claim to be the newest thing in the library.
    YearNew,
    YearOld,
    /// Whatever the DVR got most recently, recorded or imported. On a series
    /// this is the group whose newest recording is newest.
    Added,
    Longest,
    Shortest,
    /// The most unwatched episodes first, which is "what am I behind on".
    Unwatched,
    /// The most recordings first.
    Episodes,
    /// Running first, then waiting, paused, failed, finished. The downloads
    /// screen's natural order, and only its: nothing else here has a status.
    Status,
}

/// Case-insensitive name order, without allocating.
///
/// `to_lowercase()` on both sides of a comparison is two heap strings per
/// comparison, and sorting seven thousand films is ninety thousand
/// comparisons — every frame, because an immediate-mode grid is sorted each
/// time it is drawn. Lowercasing the characters as they stream costs nothing.
pub fn name_order(a: &str, b: &str) -> std::cmp::Ordering {
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
}

impl Sort {
    /// What the Movies tab offers.
    pub const MOVIES: [Sort; 7] = [
        Sort::NameAZ,
        Sort::NameZA,
        Sort::YearNew,
        Sort::YearOld,
        Sort::Added,
        Sort::Longest,
        Sort::Shortest,
    ];

    /// What the TV tab offers.
    pub const SHOWS: [Sort; 5] = [
        Sort::NameAZ,
        Sort::NameZA,
        Sort::Unwatched,
        Sort::Episodes,
        Sort::Added,
    ];

    /// What the Recorded tab offers. Year is left out: a tab of things one
    /// DVR recorded is a tab of things from roughly now.
    pub const RECORDED: [Sort; 5] = [
        Sort::Added,
        Sort::NameAZ,
        Sort::NameZA,
        Sort::Longest,
        Sort::Shortest,
    ];

    /// What the downloads screen offers.
    pub const DOWNLOADS: [Sort; 3] = [Sort::Status, Sort::NameAZ, Sort::NameZA];

    pub fn label(self) -> &'static str {
        match self {
            Sort::NameAZ => "A to Z",
            Sort::NameZA => "Z to A",
            Sort::YearNew => "Year, newest",
            Sort::YearOld => "Year, oldest",
            Sort::Added => "Recently added",
            Sort::Longest => "Longest",
            Sort::Shortest => "Shortest",
            Sort::Unwatched => "Most unwatched",
            Sort::Episodes => "Most recordings",
            Sort::Status => "By status",
        }
    }

    /// Arrange a flat list of recordings — the Movies tab, and the Recorded
    /// tab, which shares every order here that is not about years.
    ///
    /// Every order breaks its ties by name, so two films from 1954 stand in a
    /// predictable order rather than whichever the server sent this refresh.
    pub fn apply_recordings(self, list: &mut [&Recording]) {
        use std::cmp::Reverse;
        match self {
            Sort::NameZA => list.sort_by(|a, b| name_order(&b.title, &a.title)),
            Sort::YearNew => list.sort_by(|a, b| {
                let key = |r: &Recording| (r.year().is_none(), Reverse(r.year().unwrap_or(0)));
                key(a).cmp(&key(b)).then_with(|| name_order(&a.title, &b.title))
            }),
            Sort::YearOld => list.sort_by(|a, b| {
                let key = |r: &Recording| (r.year().is_none(), r.year().unwrap_or(0));
                key(a).cmp(&key(b)).then_with(|| name_order(&a.title, &b.title))
            }),
            Sort::Added => list.sort_by_key(|r| Reverse(r.created_at)),
            Sort::Longest => list.sort_by(|a, b| {
                b.duration
                    .total_cmp(&a.duration)
                    .then_with(|| name_order(&a.title, &b.title))
            }),
            Sort::Shortest => list.sort_by(|a, b| {
                a.duration
                    .total_cmp(&b.duration)
                    .then_with(|| name_order(&a.title, &b.title))
            }),
            // A to Z, and any persisted choice that means nothing for a flat
            // list of recordings.
            _ => list.sort_by(|a, b| name_order(&a.title, &b.title)),
        }
    }

    /// Arrange the series grid. `stats` is [`Home::series_stats`].
    pub fn apply_shows(
        self,
        list: &mut [&Group],
        stats: &std::collections::HashMap<&str, (usize, i64)>,
    ) {
        match self {
            Sort::NameZA => list.sort_by(|a, b| name_order(&b.name, &a.name)),
            Sort::Unwatched => list.sort_by(|a, b| {
                b.unwatched
                    .cmp(&a.unwatched)
                    .then_with(|| name_order(&a.name, &b.name))
            }),
            Sort::Episodes => list.sort_by(|a, b| {
                b.stats(stats)
                    .0
                    .cmp(&a.stats(stats).0)
                    .then_with(|| name_order(&a.name, &b.name))
            }),
            Sort::Added => list.sort_by(|a, b| {
                b.stats(stats)
                    .1
                    .cmp(&a.stats(stats).1)
                    .then_with(|| name_order(&a.name, &b.name))
            }),
            // A to Z — the order groups already arrive in — and any persisted
            // choice that means nothing for series.
            _ => list.sort_by(|a, b| name_order(&a.name, &b.name)),
        }
    }
}

/// A program the DVR intends to record but has not yet.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Upcoming {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub channel: String,
    /// Job start, which already includes padding.
    pub start: i64,
    pub duration: i64,
    /// Empty when this job was made by hand rather than by a series pass.
    pub rule_id: String,
    pub image: String,
}

/// Everything the browsing screens show, resolved in one go.
///
/// Held together rather than fetched per screen because it all comes from the
/// same two responses, and re-requesting seven thousand recordings each time
/// someone switches tab would be absurd.
#[derive(Default, Clone, Deserialize, Serialize)]
pub struct Home {
    /// Started and unfinished, most recently touched first.
    pub continue_watching: Vec<Recording>,
    /// The next unwatched episode of each series that has one.
    pub up_next: Vec<Recording>,
    /// Newest recordings that have not been started.
    pub recent: Vec<Recording>,
    pub total_recordings: usize,

    /// Everything playable, recorded and imported alike.
    pub all: Vec<Recording>,
    /// What this DVR recorded off a tuner, newest first. Distinct from the
    /// imported library, and usually a tiny fraction of it.
    pub recorded: Vec<Recording>,
    /// Series, for the Library's poster grid.
    pub groups: Vec<Group>,
    /// Scheduled but not yet recorded, soonest first.
    pub upcoming: Vec<Upcoming>,
}

impl Home {
    /// Recordings grouped under one series, newest first.
    pub fn episodes_of(&self, show_id: &str) -> Vec<&Recording> {
        let mut list: Vec<&Recording> = self
            .all
            .iter()
            .filter(|r| r.show_id == show_id)
            .collect();
        list.sort_by_key(|r| {
            std::cmp::Reverse((r.season_number, r.episode_number, r.created_at))
        });
        list
    }

    /// Each series' episode count and newest recording, in one pass.
    ///
    /// One walk over every recording rather than one per series: the count
    /// under each poster used to be counted by scanning the whole library
    /// once per drawn row, and the sorts by episode count and newest
    /// recording would have scanned it once per comparison.
    pub fn series_stats(&self) -> std::collections::HashMap<&str, (usize, i64)> {
        let mut stats: std::collections::HashMap<&str, (usize, i64)> =
            std::collections::HashMap::new();
        for recording in &self.all {
            let entry = stats.entry(recording.show_id.as_str()).or_insert((0, 0));
            entry.0 += 1;
            entry.1 = entry.1.max(recording.created_at);
        }
        stats
    }

    /// Where the last successful load is kept.
    ///
    /// Beside the downloads rather than in the settings file: this is a cache,
    /// not a preference, and deleting it costs nothing but one refresh.
    fn cache_path() -> Option<std::path::PathBuf> {
        Some(crate::paths::data_dir()?.join("library.json"))
    }

    /// Keep a copy on disk, so the next launch has something to show before —
    /// or without — the server.
    ///
    /// A download is only watchable offline if its title, artwork and episode
    /// number survive being offline too. Without this the library is empty
    /// until the DVR answers, which is precisely when it cannot.
    pub fn save_cache(&self) {
        let Some(path) = Self::cache_path() else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let Ok(json) = serde_json::to_vec(self) else { return };
        // Written beside and renamed, so an interrupted write cannot leave a
        // half-file that fails to parse on the next start.
        let temporary = path.with_extension("json.part");
        if std::fs::write(&temporary, &json).is_ok() {
            let _ = std::fs::rename(&temporary, &path);
        }
    }

    /// The last successful load, if there is one.
    pub fn load_cache() -> Option<Self> {
        let path = Self::cache_path()?;
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }
}

pub struct Library {
    base: String,
    http: reqwest::Client,
}

impl Library {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .user_agent(crate::settings::user_agent())
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    pub async fn recordings(&self) -> Result<Vec<Recording>> {
        let url = format!("{}/api/v1/all", self.base);
        let list: Vec<Recording> = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .with_context(|| format!("GET {url}"))?
            .json()
            .await
            .with_context(|| format!("GET {url}"))?;
        Ok(list)
    }

    pub async fn groups(&self) -> Result<Vec<Group>> {
        let url = format!("{}/dvr/groups", self.base);
        let raw: serde_json::Value = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .with_context(|| format!("GET {url}"))?
            .json()
            .await
            .with_context(|| format!("GET {url}"))?;

        Ok(raw
            .as_array()
            .map(|list| list.iter().map(Group::from_json).collect())
            .unwrap_or_default())
    }

    /// Where a recording's video actually lives.
    pub fn stream_url(&self, id: &str) -> String {
        format!("{}/dvr/files/{id}/stream.mpg", self.base)
    }

    /// Tell the server how far through a recording playback has got.
    ///
    /// The position is a **path segment**, not a query parameter and not a
    /// JSON body — `PUT /dvr/files/<id>/playback_time/<seconds>`. Worth
    /// stating plainly because every other shape returns 404, and this is what
    /// makes Continue Watching and Up Next work across every Channels client
    /// rather than only inside one session of this one.
    pub async fn report_position(&self, id: &str, seconds: f64) -> Result<()> {
        let secs = seconds.max(0.0).round() as i64;
        let url = format!("{}/dvr/files/{id}/playback_time/{secs}", self.base);
        self.http
            .put(&url)
            .send()
            .await
            .with_context(|| format!("PUT {url}"))?
            .error_for_status()
            .with_context(|| format!("PUT {url}"))?;
        Ok(())
    }

    /// Mark a recording watched or unwatched.
    pub async fn set_watched(&self, id: &str, watched: bool) -> Result<()> {
        let verb = if watched { "watch" } else { "unwatch" };
        let url = format!("{}/dvr/files/{id}/{verb}", self.base);
        self.http
            .put(&url)
            .send()
            .await
            .with_context(|| format!("PUT {url}"))?
            .error_for_status()
            .with_context(|| format!("PUT {url}"))?;
        Ok(())
    }

    /// Move a recording to the DVR's Trash.
    ///
    /// This is a soft delete and deliberately so. Channels keeps deleted
    /// recordings in Trash for a retention period and empties it early only
    /// when the disk gets tight, so anything removed from here can be restored
    /// from the server's own admin page.
    ///
    /// Verified against Channels' own interface, which sends exactly this for
    /// its delete button and reserves a different call for the irreversible
    /// one: `PUT /dvr/files/<id>/permanently_delete`, behind a second
    /// confirmation that says "permanently". Nothing in this application ever
    /// sends that, and nothing should start.
    pub async fn delete(&self, id: &str) -> Result<()> {
        let url = format!("{}/dvr/files/{id}", self.base);
        self.http
            .delete(&url)
            .send()
            .await
            .with_context(|| format!("DELETE {url}"))?
            .error_for_status()
            .with_context(|| format!("DELETE {url}"))?;
        Ok(())
    }

    /// What is scheduled but has not happened yet.
    pub async fn upcoming(&self) -> Result<Vec<Upcoming>> {
        let url = format!("{}/dvr/jobs", self.base);
        let jobs: serde_json::Value = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .with_context(|| format!("GET {url}"))?
            .json()
            .await
            .with_context(|| format!("GET {url}"))?;

        let Some(list) = jobs.as_array() else { return Ok(Vec::new()) };
        let mut out: Vec<Upcoming> = list
            .iter()
            .map(|job| {
                let airing = job.get("Airing").cloned().unwrap_or(serde_json::Value::Null);
                Upcoming {
                    id: job
                        .get("ID")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    title: job
                        .get("Name")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    subtitle: airing
                        .get("EpisodeTitle")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    channel: job
                        .get("Channels")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.first())
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    start: job.get("Time").and_then(|v| v.as_i64()).unwrap_or(0),
                    duration: job.get("Duration").and_then(|v| v.as_i64()).unwrap_or(0),
                    rule_id: job
                        .get("RuleID")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    image: airing
                        .get("Image")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                }
            })
            .filter(|u| !u.id.is_empty())
            .collect();

        out.sort_by_key(|u| u.start);
        Ok(out)
    }

    /// Build everything the browsing screens need.
    ///
    /// Three requests, then all of it is arranged locally. The alternative,
    /// asking the server for each row and each screen separately, would be a
    /// dozen round trips for data that came from the same place.
    pub async fn home(&self) -> Result<Home> {
        let (recordings, groups, upcoming) =
            tokio::join!(self.recordings(), self.groups(), self.upcoming());
        let recordings = recordings?;
        let mut groups = groups.unwrap_or_default();
        let upcoming = upcoming.unwrap_or_default();

        let _ = recordings.len();

        let mut continue_watching: Vec<Recording> = recordings
            .iter()
            .filter(|r| r.playable() && r.in_progress())
            .cloned()
            .collect();
        // Most recently watched first, on the field that means that.
        //
        // `last_watched_at` is what the server records when something is
        // played. `updated_at`, which this used before, is bumped by the
        // server's own housekeeping as well: measured on a real library,
        // twenty of twenty-four part-watched recordings shared three minutes
        // on one night, and sorting by it put a documentary nobody had got 4%
        // into above an episode somebody was a third of the way through.
        //
        // `updated_at` stays behind it for anything the server has never
        // recorded a play for, so those sort among themselves rather than all
        // landing together at zero.
        continue_watching
            .sort_by_key(|r| std::cmp::Reverse((r.last_watched_at, r.updated_at)));
        continue_watching.truncate(12);

        // "Up next" is the server's own answer, resolved from ids back to
        // recordings. Series already represented in Continue Watching are left
        // out so the same program does not appear twice on one screen.
        let started: std::collections::HashSet<&str> =
            continue_watching.iter().map(|r| r.show_id.as_str()).collect();
        let by_id: std::collections::HashMap<&str, &Recording> =
            recordings.iter().map(|r| (r.id.as_str(), r)).collect();

        let mut up_next: Vec<Recording> = groups
            .iter()
            .filter(|g| g.unwatched > 0)
            .filter_map(|g| g.up_next_id())
            .filter_map(|id| by_id.get(id.as_str()).copied())
            .filter(|r| r.playable() && !started.contains(r.show_id.as_str()))
            .cloned()
            .collect();
        up_next.sort_by_key(|r| std::cmp::Reverse(r.created_at));
        up_next.truncate(12);

        let shown: std::collections::HashSet<&str> = continue_watching
            .iter()
            .chain(up_next.iter())
            .map(|r| r.id.as_str())
            .collect();

        let mut recent: Vec<Recording> = recordings
            .iter()
            .filter(|r| {
                r.playable() && !r.watched && r.playback_time == 0.0 && !shown.contains(r.id.as_str())
            })
            .cloned()
            .collect();
        recent.sort_by_key(|r| std::cmp::Reverse(r.created_at));
        recent.truncate(16);

        let all: Vec<Recording> = recordings.into_iter().filter(|r| r.playable()).collect();

        let mut recorded: Vec<Recording> =
            all.iter().filter(|r| r.from_dvr()).cloned().collect();
        recorded.sort_by_key(|r| std::cmp::Reverse(r.created_at));

        // There was an `imported` list here: every item this DVR did not
        // record, cloned out of `all`. Seven thousand two hundred of them on a
        // real library, held in memory and written into the cache file, and
        // read by nothing. The Library screen filters `all` itself.
        //
        // Sorting it was a fix to a bug nobody could see, on a field nobody
        // looked at. Deleting it is the better fix.

        // Series with nothing playable behind them would be dead tiles.
        let have: std::collections::HashSet<&str> =
            all.iter().map(|r| r.show_id.as_str()).collect();
        groups.retain(|g| have.contains(g.id.as_str()) || have.contains(g.series_id.as_str()));
        groups.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        Ok(Home {
            continue_watching,
            up_next,
            recent,
            // What the DVR recorded, not everything it can play. Reporting
            // 7,536 "recordings" when 303 came off a tuner is simply wrong.
            total_recordings: recorded.len(),
            all,
            recorded,
            groups,
            upcoming,
        })
    }
}
