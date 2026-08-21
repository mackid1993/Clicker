// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial client for Channels DVR Server
// Copyright (c) 2026 David Brustein

//! The TV guide: channels, what is on them, and what is scheduled to record.
//!
//! Three sources of truth, merged once and then filtered locally:
//!
//! * `/dvr/guide/channels` — every channel, keyed by number, carrying which
//!   device it came from
//! * `/devices/ANY/guide?duration=N` — the listings
//! * `/dvr/jobs` and `/dvr/rules` — what is already going to be recorded
//!
//! Collections and sources are separate filters that **intersect**. Picking
//! DirecTV and then picking the HDHomeRun should show what is on both, which
//! is usually nothing; the alternative, where the second choice silently
//! replaces the first, means the interface quietly ignores half of what it was
//! told.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct Channel {
    #[serde(rename = "Number", default)]
    pub number: String,
    /// What a collection calls this channel, which is not its number.
    ///
    /// The server keeps both. `Number` is where the channel sits in the
    /// lineup, and it can be changed; `ChannelID` is what the source called
    /// it, and it cannot. A channel collection stores the second: the
    /// server builds a collection's `items` out of each source's channel
    /// `ID`, falling back to the guide number only for a source that
    /// offers no id. Matching a collection against numbers is therefore
    /// matching against the wrong column, and it works only where nobody
    /// has renumbered anything and every source names its channels after
    /// their numbers.
    ///
    /// Filled in from the number when the server sends none, which is the
    /// same fallback the server applies when it builds the guide.
    #[serde(rename = "ChannelID", default)]
    pub id: String,
    #[serde(rename = "Name", default)]
    pub name: String,
    #[serde(rename = "Image", default)]
    pub logo: String,
    /// Which tuner or source this channel belongs to.
    #[serde(rename = "DeviceID", default)]
    pub source: String,
    #[serde(rename = "HD", default)]
    pub hd: bool,
    #[serde(rename = "Hidden", default)]
    pub hidden: bool,
    #[serde(rename = "Favorite", default)]
    pub favorite: bool,
}

/// One program.
#[derive(Debug, Clone)]
pub struct Airing {
    pub title: String,
    pub episode_title: String,
    pub summary: String,
    pub start: i64,
    pub duration: i64,
    pub channel: String,
    pub series_id: String,
    pub program_id: String,
    pub is_new: bool,
    pub is_movie: bool,
}

impl Airing {
    pub fn end(&self) -> i64 {
        self.start + self.duration
    }

    pub fn airing_now(&self, now: i64) -> bool {
        self.start <= now && now < self.end()
    }

    /// Whether this has yet to happen. The only thing that can be done with a
    /// program that has not aired is record it, which is what makes a plain
    /// left click unambiguous there.
    pub fn in_future(&self, now: i64) -> bool {
        self.start > now
    }

    pub fn subtitle(&self) -> String {
        if self.episode_title.is_empty() {
            self.summary.clone()
        } else {
            self.episode_title.clone()
        }
    }
}

#[derive(Debug, Clone)]
pub struct Row {
    pub channel: Channel,
    pub airings: Vec<Airing>,
}

/// A named group of channels, as configured on the server.
#[derive(Debug, Clone, Deserialize)]
pub struct Collection {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub slug: String,
    /// The channels in it, by `ChannelID` — not by number. See
    /// [`Channel::id`], which is what these are matched against.
    ///
    /// `null` on the wire when the collection is empty, not `[]`: the server
    /// is Go, and Go marshals a nil slice as `null`. Plain `#[serde(default)]`
    /// only covers a *missing* key, so without the custom reader one
    /// channel-less collection — created and not yet filled — refused to
    /// parse at all.
    #[serde(default, deserialize_with = "null_items")]
    pub items: Vec<String>,
}

fn null_items<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(Option::<Vec<String>>::deserialize(d)?.unwrap_or_default())
}

/// What the DVR already intends to record.
#[derive(Default, Clone)]
pub struct Schedule {
    /// Keyed `channel|airing start`, because a job's own start has padding
    /// folded into it and will not match a guide entry.
    pub jobs: HashMap<String, String>,
    /// Series that have a pass, by SeriesID. The id is kept, not just
    /// membership, because canceling a pass needs it and hunting for it again
    /// would mean another round trip at exactly the wrong moment.
    pub rules: HashMap<String, Pass>,
}

/// A season pass, and the one thing about it that decides what a guide cell
/// should say: whether it takes every showing or only new episodes.
#[derive(Debug, Clone)]
pub struct Pass {
    pub id: String,
    pub new_only: bool,
}

impl Schedule {
    pub fn job_for(&self, airing: &Airing) -> Option<&String> {
        self.jobs.get(&key(&airing.channel, airing.start))
    }

    /// Whether a pass exists for this programme's series at all.
    ///
    /// Membership, not coverage — which is what the context menu wants, so
    /// that "Cancel the series pass" is offered from any showing and a
    /// repeat cannot be used to create a second pass for a series that
    /// already has one. For what to draw on the cell, see `pass_records`.
    pub fn has_pass(&self, airing: &Airing) -> bool {
        self.rule_for(airing).is_some()
    }

    /// Whether the pass will actually record *this* showing.
    ///
    /// A pass set to new episodes only matches the same `New` tag the guide
    /// reads for its own badge — Channels evaluates the rule's `EQ` against
    /// the airing's tags — so a repeat is not going to be recorded and the
    /// cell should not claim it will. Marking every showing of a long-running
    /// series was making the guide answer "what am I already getting" with a
    /// list of things it was not getting.
    pub fn pass_records(&self, airing: &Airing) -> bool {
        self.rule_for(airing)
            .is_some_and(|pass| !pass.new_only || airing.is_new)
    }

    pub fn rule_for(&self, airing: &Airing) -> Option<&Pass> {
        if airing.series_id.is_empty() {
            return None;
        }
        self.rules.get(&airing.series_id)
    }
}

/// Empty minutes left past the last listing, so the end of the guide is
/// somewhere to arrive at rather than a wall.
const TAIL_PADDING: i64 = 60;

pub fn key(channel: &str, start: i64) -> String {
    format!("{}|{}", channel.trim().to_lowercase(), start)
}

/// Where the server's own airing objects are kept.
///
/// On disk rather than in memory. Scheduling a recording hands the server back
/// its own object untouched, because rebuilding it field by field would
/// silently drop anything this client does not know about — but that is needed
/// for the one airing being recorded, and it was being held for all of them.
/// A day of listings is 24MB of JSON across thirteen thousand airings, and as
/// parsed values that is a couple of hundred thousand small allocations sitting
/// in the heap to serve an action that happens once in a while.
///
/// One line per airing, so finding one is a scan and a single parse rather
/// than reading the whole file back into a tree.
fn raw_cache_path() -> Option<std::path::PathBuf> {
    Some(crate::paths::data_dir()?.join("guide-airings.jsonl"))
}

/// Write the airing objects out, replacing whatever was there.
fn write_raw_cache(lines: &str) {
    let Some(path) = raw_cache_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Written whole and replaced, not appended: these describe one load of the
    // guide, and half of one load plus half of another describes nothing.
    let _ = std::fs::write(path, lines);
}

/// The server's object for one airing, or none if this load did not cache it.
///
/// Scans for the key rather than parsing every line, so the cost is reading
/// 24MB and parsing one object.
pub fn raw_airing(channel: &str, start: i64) -> Option<Value> {
    let path = raw_cache_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    let wanted = format!("\"{}\"", key(channel, start));
    for line in text.lines() {
        // The key is written first on every line, so this is a prefix test
        // rather than a search through the airing's own text.
        if line.starts_with(&format!("{{\"k\":{wanted}")) {
            let parsed: Value = serde_json::from_str(line).ok()?;
            return parsed.get("a").cloned();
        }
    }
    None
}

#[derive(Default, Clone)]
pub struct GuideData {
    pub rows: Vec<Row>,
    pub collections: Vec<Collection>,
    pub sources: Vec<String>,
    pub schedule: Schedule,
    /// The window the listings cover.
    pub start: i64,
    /// How far past `start` the grid may be scrolled, in minutes.
    ///
    /// Minutes rather than hours because listings do not end on the hour. The
    /// last programs of the window run to the half hour, and measuring this in
    /// whole hours rounded that away — the guide stopped half an hour short of
    /// listings it was holding. See where it is computed.
    pub minutes: i64,
}

pub struct GuideApi {
    base: String,
    http: reqwest::Client,
}

fn as_i64(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0)
}
fn as_str(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}
fn has_tag(v: &Value, key: &str, needle: &str) -> bool {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .any(|s| s.eq_ignore_ascii_case(needle))
        })
        .unwrap_or(false)
}

impl GuideApi {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .user_agent(crate::settings::user_agent())
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    async fn get(&self, path: &str) -> Result<Value> {
        let url = format!("{}{}", self.base, path);
        Ok(self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .with_context(|| format!("GET {url}"))?
            .json()
            .await
            .with_context(|| format!("GET {url}"))?)
    }

    /// Load the whole guide for a window starting now.
    pub async fn load(&self, now: i64, hours: i64) -> Result<GuideData> {
        // Listings are requested from the aligned start, not from this exact
        // second, and the two have to match. Asking from `now` while drawing a
        // grid that begins at the previous half hour leaves up to thirty
        // minutes at the left of every row with no program in it — a dead
        // band that looks like missing data and is really a mismatched query.
        let now = now - now.rem_euclid(1800);
        // `duration` is in SECONDS, and `time` has to be given explicitly.
        //
        // This is not a detail. `?duration=6` — which reads as six hours and is
        // what this originally sent — returns exactly one airing per channel
        // and nothing else, so the guide rendered as a column of lonely boxes
        // with hours of blank space beside them. It looked like a layout bug
        // and was not. Measured against the real server: `duration=6` gives 1.0
        // airings per channel, `time=<now>&duration=21600` gives 8.7 on
        // average and up to 29.
        //
        // The path is held in a binding because passing `&format!(...)`
        // straight into join! creates a temporary that is dropped while the
        // future still borrows it.
        let listings_path = format!(
            "/devices/ANY/guide?time={now}&duration={}",
            hours * 3600
        );
        let (channels, listings, collections, jobs, rules, devices) = tokio::join!(
            self.get("/dvr/guide/channels"),
            self.get(&listings_path),
            self.get("/dvr/collections/channels"),
            self.get("/dvr/jobs"),
            self.get("/dvr/rules"),
            self.get("/devices"),
        );

        // What each source is actually called.
        //
        // A channel does not carry the name of the source it came from. It
        // carries `DeviceID`, which is a serial number or an internal handle:
        // checked against a real server, the two sources there are `1063F9E0`
        // and `M3U-DirecTV`, where Channels itself shows "HDHomeRun DUO" and
        // "DirecTV". The guide was showing the raw identifiers, which is why
        // the source filter did not match what people see in Channels and why
        // a source could look like it was missing entirely: nobody recognizes
        // their aerial by its serial number.
        //
        // The names live on /devices, keyed by that same DeviceID.
        let mut device_names: HashMap<String, String> = HashMap::new();
        if let Ok(Value::Array(list)) = devices {
            for device in list {
                let id = as_str(&device, "DeviceID");
                // FriendlyName is what the Channels interface displays. Falling
                // back to the id keeps a source that has no name visible rather
                // than dropping it, which is the failure this is fixing.
                let name = [
                    as_str(&device, "FriendlyName"),
                    as_str(&device, "Name"),
                    id.clone(),
                ]
                .into_iter()
                .find(|n| !n.trim().is_empty())
                .unwrap_or_default();
                if !id.is_empty() {
                    device_names.insert(id, name);
                }
            }
        }

        // Past this, a listing is not a listing.
        //
        // Guide data occasionally carries dates that are jokes or mistakes: a
        // Paramount recording of a Star Trek episode is timed July 21, 2185.
        // An airing that far out is harmless to the scroll limit, which is set
        // from a quartile rather than a maximum, but it is still a cell laid
        // out hundreds of millions of pixels to the right, and it is never
        // something anyone meant to schedule. A month is far past anything a
        // guide is asked for and far short of a century.
        let horizon = now + 30 * 24 * 3600;

        // Built as the rows are, written once at the end. 24MB of text, which
        // is the point: it is 24MB on a disk instead of a couple of hundred
        // thousand allocations in the heap.
        let mut raw_lines = String::with_capacity(24 * 1024 * 1024);

        // Channels come back as an object keyed by number, not a list.
        let mut by_number: HashMap<String, Channel> = HashMap::new();
        if let Ok(Value::Object(map)) = channels {
            for (number, value) in map {
                if let Ok(mut channel) = serde_json::from_value::<Channel>(value) {
                    if channel.number.is_empty() {
                        channel.number = number.clone();
                    }
                    // Every channel needs one, because that is what the
                    // collection filter matches against. A server that sends no
                    // `ChannelID` is one whose collections are keyed by number,
                    // so the number is the right answer there.
                    if channel.id.is_empty() {
                        channel.id = channel.number.clone();
                    }
                    // The device id becomes the name it is known by. Left as
                    // the id when the device list did not mention it, so a
                    // channel from a source that has since gone still groups
                    // somewhere rather than falling out of the filter.
                    if let Some(name) = device_names.get(&channel.source) {
                        channel.source = name.clone();
                    }
                    if !channel.hidden {
                        by_number.insert(number, channel);
                    }
                }
            }
        }

        let mut rows: Vec<Row> = Vec::new();
        if let Ok(Value::Array(entries)) = listings {
            for entry in entries {
                let number = entry
                    .get("Channel")
                    .map(|c| as_str(c, "Number"))
                    .unwrap_or_default();
                if number.is_empty() {
                    continue;
                }

                // Prefer the merged channel record, which knows the source.
                let channel = by_number.get(&number).cloned().unwrap_or_else(|| {
                    let c = entry.get("Channel").cloned().unwrap_or(Value::Null);
                    let device = as_str(&c, "DeviceID");
                    Channel {
                        number: number.clone(),
                        // Same fallback as above. A channel that reaches
                        // this arm is one the channel list did not have,
                        // and it still has to be selectable by collection.
                        id: match as_str(&c, "ChannelID") {
                            id if id.is_empty() => number.clone(),
                            id => id,
                        },
                        name: as_str(&c, "Name"),
                        logo: as_str(&c, "Image"),
                        // Named, on the same terms as above: a channel that
                        // only appears in the listings still has to land in
                        // the same group as its siblings, and it will not if
                        // one of them says "DirecTV" and the other says
                        // "M3U-DirecTV".
                        source: device_names.get(&device).cloned().unwrap_or(device),
                        hd: c.get("HD").and_then(Value::as_bool).unwrap_or(false),
                        hidden: false,
                        favorite: false,
                    }
                });

                let airings = entry
                    .get("Airings")
                    .and_then(Value::as_array)
                    .map(|list| {
                        list.iter()
                            .filter_map(|a| {
                                let airing = Airing {
                                    title: as_str(a, "Title"),
                                    episode_title: as_str(a, "EpisodeTitle"),
                                    summary: as_str(a, "Summary"),
                                    start: as_i64(a, "Time"),
                                    duration: as_i64(a, "Duration"),
                                    channel: number.clone(),
                                    series_id: as_str(a, "SeriesID"),
                                    program_id: as_str(a, "ProgramID"),
                                    is_new: has_tag(a, "Tags", "New"),
                                    is_movie: has_tag(a, "Categories", "Movie"),
                                };
                                if airing.start >= horizon {
                                    return None;
                                }
                                // The server's own object goes to the file, not
                                // into this struct. One line, key first, so it
                                // can be found later without parsing the rest.
                                use std::fmt::Write;
                                let _ = writeln!(
                                    raw_lines,
                                    "{{\"k\":\"{}\",\"a\":{}}}",
                                    key(&airing.channel, airing.start),
                                    a
                                );
                                Some(airing)
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                if !airings.is_empty() {
                    rows.push(Row { channel, airings });
                }
            }
        }

        write_raw_cache(&raw_lines);
        drop(raw_lines);

        // Numeric where possible: "10" before "9" is what string ordering gives
        // and it looks broken on a channel list.
        rows.sort_by(|a, b| {
            let pa: f64 = a.channel.number.parse().unwrap_or(f64::MAX);
            let pb: f64 = b.channel.number.parse().unwrap_or(f64::MAX);
            pa.partial_cmp(&pb)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.channel.number.cmp(&b.channel.number))
        });

        let mut sources: Vec<String> = rows
            .iter()
            .map(|r| r.channel.source.clone())
            .filter(|s| !s.is_empty())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        sources.sort();

        // One element at a time. Parsing the whole array in one call made a
        // single unreadable collection erase every other one — the error was
        // swallowed and the picker simply had nothing in it, on a server where
        // the collections were fine. A collection this client cannot read
        // should cost exactly itself.
        let collections: Vec<Collection> = match collections {
            Ok(Value::Array(list)) => list
                .into_iter()
                .filter_map(|c| serde_json::from_value(c).ok())
                .collect(),
            _ => Vec::new(),
        };

        // The window starts at the previous half hour, not at this exact
        // second. Listings are organized in half-hour slots, so a ruler that
        // begins at "1:56 AM" is both ugly and harder to read against, and
        // anything already in progress needs somewhere to the left of now to
        // be drawn from.
        let aligned_start = now - now.rem_euclid(1800);

        // How far the grid may be scrolled: the point up to which most channels
        // still have something to show.
        //
        // Not the furthest airing. One channel showing a twelve-hour infomercial
        // block reaches days past everything else, and scrolling to meet it
        // crosses a widening band of empty rows — the whole guide blank but for
        // one lonely cell. Not the requested duration either, because the server
        // may simply have less than that.
        //
        // So: the last airing on each channel, and a quarter of the way up that
        // list. Three quarters of the channels still have listings at the right
        // edge, and the long tail belonging to a handful of them is left off the
        // end rather than dragging the scroll out for everyone.
        //
        // Not capped at the requested duration either. What comes back reaches
        // past what was asked for — a request for a day answers with listings
        // to a day and a half — and those are real programs on real channels,
        // so stopping the grid at the request would hide listings already in
        // memory. The quartile is what keeps the far edge honest; the request
        // only decides how much to fetch.
        let mut ends: Vec<i64> = rows
            .iter()
            .filter_map(|row| row.airings.iter().map(Airing::end).max())
            .collect();
        ends.sort_unstable();
        let covered = ends
            .get(ends.len() / 4)
            .copied()
            .unwrap_or(aligned_start + hours * 3600);

        Ok(GuideData {
            rows,
            collections,
            sources,
            schedule: build_schedule(jobs.ok(), rules.ok()),
            start: aligned_start,
            // To the minute — half an hour of listings is two programs — plus
            // an hour of room past the end. Scrolling to a hard stop with the
            // last cell jammed against the right edge reads as the guide
            // having been cut off rather than having ended.
            minutes: ((covered - aligned_start) / 60).max(30) + TAIL_PADDING,
        })
    }
}

fn build_schedule(jobs: Option<Value>, rules: Option<Value>) -> Schedule {
    let mut schedule = Schedule::default();

    if let Some(Value::Array(list)) = jobs {
        for job in list {
            let id = as_str(&job, "ID");
            if id.is_empty() {
                continue;
            }
            // The airing's own start, not the job's: the job's has padding
            // folded in and will never equal a guide entry.
            let start = job
                .get("Airing")
                .map(|a| as_i64(a, "Time"))
                .unwrap_or_else(|| as_i64(&job, "Time"));
            if let Some(channels) = job.get("Channels").and_then(Value::as_array) {
                for channel in channels.iter().filter_map(Value::as_str) {
                    schedule.jobs.insert(key(channel, start), id.clone());
                }
            }
        }
    }

    if let Some(Value::Array(list)) = rules {
        for rule in list {
            let series = rule
                .get("EQ")
                .map(|eq| as_str(eq, "SeriesID"))
                .unwrap_or_default();
            let id = as_str(&rule, "ID");
            // `Tags: New` inside the rule's own EQ is how Channels says "new
            // episodes only" — the same key this application writes when it
            // creates a pass, and the same tag an airing carries. Read as
            // either a string or a list, because a rule that came from
            // somewhere other than here may hold either.
            let new_only = rule
                .get("EQ")
                .map(|eq| {
                    has_tag(eq, "Tags", "New")
                        || eq
                            .get("Tags")
                            .and_then(Value::as_str)
                            .is_some_and(|t| t.eq_ignore_ascii_case("New"))
                })
                .unwrap_or(false);
            if !series.is_empty() && !id.is_empty() {
                schedule.rules.insert(series, Pass { id, new_only });
            }
        }
    }

    schedule
}

/// Which rows survive the current filters.
///
/// Collection and source intersect rather than override; see the module note.
pub fn filter<'a>(
    data: &'a GuideData,
    collection: Option<&str>,
    source: Option<&str>,
    search: &str,
) -> Vec<&'a Row> {
    let allowed: Option<HashSet<&str>> = collection.and_then(|slug| {
        data.collections
            .iter()
            .find(|c| c.slug == slug)
            .map(|c| c.items.iter().map(String::as_str).collect())
    });

    let needle = search.trim().to_lowercase();

    data.rows
        .iter()
        .filter(|row| {
            if let Some(allowed) = &allowed {
                // By id, not by number. See the field it reads.
                if !allowed.contains(row.channel.id.as_str()) {
                    return false;
                }
            }
            if let Some(source) = source {
                if row.channel.source != source {
                    return false;
                }
            }
            if !needle.is_empty() {
                let matches_channel = row.channel.name.to_lowercase().contains(&needle)
                    || row.channel.number.contains(&needle);
                let matches_program = row
                    .airings
                    .iter()
                    .any(|a| a.title.to_lowercase().contains(&needle));
                if !matches_channel && !matches_program {
                    return false;
                }
            }
            true
        })
        .collect()
}
