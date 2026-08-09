//! Artwork loading.
//!
//! Posters and preview frames arrive over HTTP and have to become GPU textures
//! before they can be drawn. Both the fetch and the JPEG decode happen off the
//! UI thread; only the upload does not, because that is the one part that has
//! to touch the graphics context.
//!
//! Requests are made once per URL and never repeated, including for failures.
//! A home screen asks for the same image on every frame it is visible, so
//! without that a single missing poster becomes a request storm at the
//! display's refresh rate.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};

use egui::ColorImage;

enum Entry {
    Loading,
    /// The texture, when it was last asked for, and whether its ink is dark.
    Ready(egui::TextureHandle, u64, bool),
    /// Remembered so a broken URL is not retried forever.
    Failed,
}

/// A decoded image and what the caller needs to know about how to draw it.
struct Decoded {
    image: ColorImage,
    /// True for artwork drawn in dark ink on transparency, which needs
    /// something light behind it to be visible at all on a dark surface.
    dark: bool,
}

/// Textures kept resident at once. Measured before this existed: the home,
/// guide and library screens together held ~480MB, dwarfing the player's
/// ~150MB — full-size posters, cached forever, for rows long since scrolled
/// away. Three hundred thumbnails is a few screens of scrollback.
const MAX_RESIDENT: usize = 300;

/// How large a card's artwork is kept. Cards are around 230px and posters
/// around 170, so this is already generous for them.
const CARD_MAX: u32 = 640;

/// How large the hero's artwork is kept, and asked for.
///
/// The hero is drawn the width of the window, so at the card's limit it is a
/// 640px picture stretched across a thousand and it looks it. The artwork
/// server will serve whatever size is asked for — the API hands out URLs
/// ending `?w=720&h=540`, and the same asset comes back at 1600x1200 when the
/// query says so — so the fix is to ask, not to upscale.
pub const HERO_MAX: u32 = 1280;

/// The same artwork, requested at a usable size.
///
/// Rewrites the size the API asked for rather than appending to it, because a
/// URL with two `w=` parameters is a URL the server is entitled to read either
/// way. Anything without them is returned untouched: the DVR's own preview
/// frames are served at one size and have no query to rewrite.
pub fn at_size(url: &str, width: u32, height: u32) -> String {
    if !url.contains("w=") && !url.contains("h=") {
        return url.to_string();
    }
    let mut out = String::with_capacity(url.len() + 8);
    for (index, part) in url.split(['?', '&']).enumerate() {
        // The first piece is the path, and the separator before each parameter
        // after it is what this rebuilds.
        if index == 0 {
            out.push_str(part);
            continue;
        }
        out.push(if index == 1 { '?' } else { '&' });
        if let Some(rest) = part.strip_prefix("w=") {
            let _ = rest;
            out.push_str(&format!("w={width}"));
        } else if let Some(rest) = part.strip_prefix("h=") {
            let _ = rest;
            out.push_str(&format!("h={height}"));
        } else {
            out.push_str(part);
        }
    }
    out
}

pub struct Images {
    runtime: tokio::runtime::Handle,
    http: reqwest::Client,
    tx: Sender<(String, Option<Decoded>)>,
    rx: Receiver<(String, Option<Decoded>)>,
    entries: HashMap<String, Entry>,
    /// Monotonic use counter for eviction; bumped on every `get`.
    tick: u64,
}

impl Images {
    pub fn new(runtime: tokio::runtime::Handle) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            runtime,
            http: reqwest::Client::builder()
                .user_agent(crate::settings::user_agent())
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            tx,
            rx,
            entries: HashMap::new(),
            tick: 0,
        }
    }

    /// Turn finished downloads into textures, and evict what nothing is
    /// looking at. Called once per frame.
    pub fn pump(&mut self, ctx: &egui::Context) {
        while let Ok((url, decoded)) = self.rx.try_recv() {
            let entry = match decoded {
                Some(decoded) => Entry::Ready(
                    ctx.load_texture(&url, decoded.image, egui::TextureOptions::LINEAR),
                    self.tick,
                    decoded.dark,
                ),
                None => Entry::Failed,
            };
            self.entries.insert(url, entry);
        }

        // Least-recently-used eviction. Dropping the handle frees the GPU
        // texture and its CPU copy; the URL is forgotten too, so it reloads
        // if it ever scrolls back into view — the trade a cache is for.
        let resident = self
            .entries
            .values()
            .filter(|e| matches!(e, Entry::Ready(..)))
            .count();
        if resident > MAX_RESIDENT {
            let mut ready: Vec<(String, u64)> = self
                .entries
                .iter()
                .filter_map(|(url, e)| match e {
                    Entry::Ready(_, used, _) => Some((url.clone(), *used)),
                    _ => None,
                })
                .collect();
            ready.sort_by_key(|(_, used)| *used);
            for (url, _) in ready.into_iter().take(resident - MAX_RESIDENT) {
                self.entries.remove(&url);
            }
        }
    }

    /// The texture for a URL, starting the load if this is the first ask.
    ///
    /// Returns `None` while loading and for anything that failed, so callers
    /// draw a placeholder rather than waiting.
    pub fn get(&mut self, url: &str) -> Option<&egui::TextureHandle> {
        self.get_at(url, CARD_MAX)
    }

    /// The same, at a size of the caller's choosing.
    ///
    /// The hero is drawn a thousand pixels wide and everything else is a card
    /// a couple of hundred across, so one limit cannot serve both: at the
    /// card's size the hero is visibly soft, and at the hero's size every
    /// thumbnail costs four times the memory it needs.
    pub fn get_at(&mut self, url: &str, max: u32) -> Option<&egui::TextureHandle> {
        if url.is_empty() {
            return None;
        }
        self.tick += 1;

        if !self.entries.contains_key(url) {
            self.entries.insert(url.to_string(), Entry::Loading);

            let tx = self.tx.clone();
            let http = self.http.clone();
            let owned = url.to_string();
            self.runtime.spawn(async move {
                let decoded = fetch_and_decode(&http, &owned, max).await;
                let _ = tx.send((owned, decoded));
            });
        }

        let tick = self.tick;
        match self.entries.get_mut(url) {
            Some(Entry::Ready(texture, used, _)) => {
                *used = tick;
                Some(&*texture)
            }
            _ => None,
        }
    }

    /// Whether the artwork at this URL is drawn in dark ink.
    ///
    /// Answers false while it is still loading, which is the right way round:
    /// the plate appears when the logo does, rather than flashing empty first.
    pub fn is_dark(&self, url: &str) -> bool {
        matches!(self.entries.get(url), Some(Entry::Ready(_, _, true)))
    }
}

/// Decoding happens here, on the worker, not on the UI thread.
///
/// A 720x540 JPEG is a few milliseconds to decode, which is most of a frame
/// budget, and a home screen can ask for a dozen at once.
async fn fetch_and_decode(http: &reqwest::Client, url: &str, max: u32) -> Option<Decoded> {
    let bytes = http
        .get(url)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .bytes()
        .await
        .ok()?;

    let decoded = image::load_from_memory(&bytes).ok()?;

    // Downscale to the size anything actually draws at. Cards are ~230px and
    // posters ~170; keeping the hero sharp needs no more than 640. A 720x540
    // poster at full size is 1.5MB of texture — measured across the home,
    // guide and library screens, storing originals was hundreds of megabytes
    // of memory for pixels no one could ever see.
    let decoded = if decoded.width() > max || decoded.height() > max {
        decoded.thumbnail(max, max)
    } else {
        decoded
    };

    let rgba = decoded.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(Decoded {
        dark: is_dark_ink(rgba.as_raw()),
        image: ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
    })
}

/// Whether artwork is drawn in dark ink, measured over the pixels that are
/// actually opaque.
///
/// Stations ship their marks as PNGs on transparency, and a good number of
/// them are black-on-transparent — on the guide's dark channel column those
/// came out as a logo-shaped hole and were simply invisible. Deciding it here,
/// once per image on the worker, means the guide can put a light plate behind
/// just those without washing out the ones already drawn in white.
///
/// The transparent surround has to be excluded rather than counted as black:
/// it is most of a typical logo's area, and including it would pull every
/// average down to the same answer.
fn is_dark_ink(rgba: &[u8]) -> bool {
    let mut luma = 0u64;
    let mut opaque = 0u64;
    for px in rgba.chunks_exact(4) {
        if px[3] < 128 {
            continue;
        }
        // Rec. 709, in tenths of a thousandth so it stays integer.
        luma += 2126 * px[0] as u64 + 7152 * px[1] as u64 + 722 * px[2] as u64;
        opaque += 1;
    }
    // Nothing opaque at all is not dark; it is nothing, and a plate behind
    // nothing is just a gray box.
    opaque > 0 && luma / opaque / 10_000 < 110
}
