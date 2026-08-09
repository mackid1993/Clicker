//! The Channels DVR HTTP API.
//!
//! Recording is a server-side act. Pressing record does not capture anything
//! locally: it asks the DVR to schedule a job, and the DVR tunes, records and
//! files it. That is why the button has to round-trip to the server before it
//! can honestly show itself as lit.
//!
//! One-off recordings are *jobs* (`/dvr/jobs`). Season passes are *rules*
//! (`/dvr/rules`) matched on series. Both are created by POSTing to a `/new`
//! path and removed by DELETEing the id.

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};

#[derive(Clone)]
pub struct Dvr {
    base: String,
    http: reqwest::Client,
}

/// A program in the guide, and the raw server object it came from.
///
/// The raw value is kept deliberately: creating a job means handing the airing
/// back to the DVR so the recording carries the right metadata, and rebuilding
/// that object field by field would silently drop whatever this client does not
/// happen to know about.
#[derive(Clone, Debug)]
pub struct Airing {
    pub title: String,
    pub subtitle: String,
    /// Broadcast start, as a Unix timestamp.
    pub start: i64,
    pub duration: i64,
    pub channel: String,
}

impl Airing {
    pub fn end(&self) -> i64 {
        self.start + self.duration
    }
}

/// The server's configured recording padding, in seconds.
///
/// Channels does not pad a manually created job the way it pads one created by
/// a rule, so the padding has to be applied here for a job scheduled from the
/// record button to behave like every other recording on the server.
#[derive(Clone, Copy, Debug, Default)]
pub struct Padding {
    pub start: i64,
    pub end: i64,
}

/// How a season pass should behave.
#[derive(Clone, Copy, Debug)]
pub struct PassOptions {
    pub padding: Padding,
    /// Only record episodes the guide marks as new, rather than every repeat.
    pub new_only: bool,
    /// How many to keep. 0 means all of them.
    pub keep: i64,
}

impl Default for PassOptions {
    fn default() -> Self {
        Self {
            padding: Padding::default(),
            new_only: true,
            keep: 0,
        }
    }
}

fn as_i64(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn as_str(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

impl Dvr {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .user_agent(crate::settings::user_agent())
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    async fn get(&self, path: &str) -> Result<Value> {
        let url = format!("{}{}", self.base, path);
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .with_context(|| format!("GET {url}"))?;
        Ok(response.json().await.with_context(|| format!("GET {url}"))?)
    }

    /// The recording padding configured on the server.
    pub async fn padding(&self) -> Result<Padding> {
        let status = self.get("/dvr").await?;
        let padding = status.get("padding").cloned().unwrap_or(Value::Null);
        Ok(Padding {
            start: as_i64(&padding, "start"),
            end: as_i64(&padding, "end"),
        })
    }

    /// The program showing on `channel` at `now`.
    ///
    /// The guide is asked for a short window rather than the whole day: the
    /// only thing needed is what is on right now, and a day of listings for
    /// every channel is a large response to parse for one answer.
    pub async fn current_airing(&self, channel: &str, now: i64) -> Result<Airing> {
        let guide = self.get("/devices/ANY/guide?duration=1").await?;
        let entries = guide
            .as_array()
            .ok_or_else(|| anyhow!("guide was not a list"))?;

        let entry = entries
            .iter()
            .find(|e| {
                e.get("Channel")
                    .map(|c| as_str(c, "Number") == channel)
                    .unwrap_or(false)
            })
            .ok_or_else(|| anyhow!("channel {channel} is not in the guide"))?;

        let airings = entry
            .get("Airings")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("channel {channel} has no listings"))?;

        // Prefer whatever covers this moment. A guide window that begins now
        // can still lead with a program already part way through, so simply
        // taking the first entry would schedule the wrong thing near the top of
        // the hour.
        let raw = airings
            .iter()
            .find(|a| {
                let start = as_i64(a, "Time");
                let end = start + as_i64(a, "Duration");
                start <= now && now < end
            })
            .or_else(|| airings.first())
            .ok_or_else(|| anyhow!("channel {channel} has no listings"))?;

        Ok(Airing {
            title: as_str(raw, "Title"),
            subtitle: as_str(raw, "EpisodeTitle"),
            start: as_i64(raw, "Time"),
            duration: as_i64(raw, "Duration"),
            channel: channel.to_string(),
        })
    }

    /// Schedule a one-off recording, returning the job id.
    pub async fn create_job(&self, airing: &Airing, padding: Padding) -> Result<String> {
        let body = json!({
            "Name": airing.title,
            "Time": airing.start - padding.start,
            "Duration": airing.duration + padding.start + padding.end,
            "Channels": [airing.channel],
            // The server's own object, read back from the guide's cache rather
            // than carried in memory for every airing on every channel. Null
            // if this airing did not come from a guide load — the server fills
            // in what it can from the fields above, which is the same position
            // a client that never had the object is in.
            "Airing": crate::guide::raw_airing(&airing.channel, airing.start)
                .unwrap_or(Value::Null),
        });

        let url = format!("{}/dvr/jobs/new", self.base);
        let created: Value = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?
            .error_for_status()
            .with_context(|| format!("POST {url}"))?
            .json()
            .await
            .with_context(|| format!("POST {url}"))?;

        let id = as_str(&created, "ID");
        if id.is_empty() {
            return Err(anyhow!("the DVR accepted the job but returned no id"));
        }
        Ok(id)
    }

    /// Create a season pass for an airing's series.
    ///
    /// Matched on `SeriesID`, so it catches every episode wherever it turns up
    /// rather than being tied to a channel or a time slot. Padding is set on
    /// the rule itself: unlike manually created jobs, the DVR *does* apply a
    /// rule's padding when it turns it into a recording.
    pub async fn create_series_rule(
        &self,
        airing: &Airing,
        series_id: &str,
        options: PassOptions,
    ) -> Result<()> {
        if series_id.is_empty() {
            return Err(anyhow!("this program has no series to record"));
        }

        // `Tags: New` is how Channels expresses "new episodes only". Omitting
        // the key entirely is what means "everything" — sending it empty is
        // not the same thing.
        let eq = if options.new_only {
            json!({ "SeriesID": series_id, "Tags": "New" })
        } else {
            json!({ "SeriesID": series_id })
        };

        let body = json!({
            "Name": airing.title,
            "EQ": eq,
            // 0 keeps everything. Silently discarding recordings is not
            // something to default to.
            "KeepNum": options.keep,
            "PaddingStart": options.padding.start,
            "PaddingEnd": options.padding.end,
            "Duplicates": false,
            "Rerecord": false,
            "Paused": false,
            "Priority": 0,
        });

        let url = format!("{}/dvr/rules/new", self.base);
        self.http
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?
            .error_for_status()
            .with_context(|| format!("POST {url}"))?;
        Ok(())
    }

    /// Remove a season pass.
    pub async fn delete_rule(&self, id: &str) -> Result<()> {
        let url = format!("{}/dvr/rules/{id}", self.base);
        self.http
            .delete(&url)
            .send()
            .await
            .with_context(|| format!("DELETE {url}"))?
            .error_for_status()
            .with_context(|| format!("DELETE {url}"))?;
        Ok(())
    }

    pub async fn delete_job(&self, id: &str) -> Result<()> {
        let url = format!("{}/dvr/jobs/{id}", self.base);
        self.http
            .delete(&url)
            .send()
            .await
            .with_context(|| format!("DELETE {url}"))?
            .error_for_status()
            .with_context(|| format!("DELETE {url}"))?;
        Ok(())
    }

    /// The id of an existing job covering this airing, if the DVR already has
    /// one. Checked on startup so the record button reflects the server rather
    /// than only what this session did.
    pub async fn job_for(&self, airing: &Airing) -> Result<Option<String>> {
        let jobs = self.get("/dvr/jobs").await?;
        let Some(jobs) = jobs.as_array() else { return Ok(None) };

        Ok(jobs
            .iter()
            .find(|job| {
                let on_channel = job
                    .get("Channels")
                    .and_then(Value::as_array)
                    .map(|c| c.iter().any(|n| n.as_str() == Some(airing.channel.as_str())))
                    .unwrap_or(false);
                // Match on the airing's own start, not the job's: the job's
                // start has padding folded into it and will not equal the
                // broadcast time.
                let same_airing = job
                    .get("Airing")
                    .map(|a| as_i64(a, "Time") == airing.start)
                    .unwrap_or(false);
                on_channel && same_airing
            })
            .map(|job| as_str(job, "ID"))
            .filter(|id| !id.is_empty()))
    }
}
