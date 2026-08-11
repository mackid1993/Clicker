// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial client for Channels DVR Server
// Copyright (c) 2026 David Brustein

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
    /// The texture, when it was last asked for, whether its ink is dark, and
    /// what it costs.
    ///
    /// The size is carried because eviction has to be about memory. Counting
    /// entries treats a 4.9MB hero and a 40KB channel logo as the same thing,
    /// and the limit that was safe for three hundred logos is not the limit
    /// that is safe for three hundred heroes.
    Ready(egui::TextureHandle, u64, bool, usize, f32),
    /// Remembered so a broken URL is not retried on every frame — but with
    /// the moment it failed, because "broken" is often "broken just now".
    ///
    /// A picture that could not be fetched because the network was refused,
    /// or because the DVR was restarting, is not a broken URL; it is a good
    /// one asked at a bad moment. Remembering the failure forever meant the
    /// first launch on a Mac — where every request before the permission is
    /// granted fails — left a library of blank cards that only a restart
    /// could fill in.
    Failed(std::time::Instant),
}

/// A decoded image and what the caller needs to know about how to draw it.
struct Decoded {
    image: ColorImage,
    /// True for artwork drawn in dark ink on transparency, which needs
    /// something light behind it to be visible at all on a dark surface.
    dark: bool,
    /// How bright the left of the picture is, 0 to 1.
    ///
    /// The hero writes white text over the left of its artwork and darkens
    /// that side to keep it legible. A fixed amount of darkening cannot do
    /// that: it is too much over a night scene and not enough over a snowy
    /// field or a title card, where the text disappears into the picture.
    /// Measured here, once per image on the worker, rather than sampled from
    /// the texture every frame on the UI thread.
    left_luma: f32,
}

/// Textures kept resident at once. Measured before this existed: the home,
/// guide and library screens together held ~480MB, dwarfing the player's
/// ~150MB — full-size posters, cached forever, for rows long since scrolled
/// away. Three hundred thumbnails is a few screens of scrollback.
const MAX_RESIDENT: usize = 300;

/// How much artwork may be resident at once.
///
/// The number that actually matters now that sizes differ by two orders of
/// magnitude. 192MB is a few screens of everything plus a run of heroes, and
/// it is a ceiling this can be held to rather than a number that happens to
/// come out of counting entries.
const MAX_BYTES: usize = 192 * 1024 * 1024;

/// How large a card's artwork is kept.
///
/// A grid card is about 155 points wide, so 640 was asking the server for
/// sixteen times the area ever drawn and paying for it four times over in
/// memory: 640x960 is 2.4MB decoded, and forty of those on screen is most of
/// the budget before anything scrolls. 360 covers the card on a 200% display
/// and leaves room for the cache to actually be a cache.
const CARD_MAX: u32 = 360;

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
    /// Use counter for eviction, advanced once per frame at the end of
    /// `pump` and stamped by every `get`, so everything a frame draws
    /// shares one age.
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
                Some(decoded) => {
                    // Four bytes a pixel, which is what the texture costs on
                    // the GPU and what its CPU copy cost on the way there.
                    let bytes = decoded.image.width() * decoded.image.height() * 4;
                    let luma = decoded.left_luma;
                    Entry::Ready(
                        ctx.load_texture(&url, decoded.image, egui::TextureOptions::LINEAR),
                        self.tick,
                        decoded.dark,
                        bytes,
                        luma,
                    )
                }
                None => Entry::Failed(std::time::Instant::now()),
            };
            self.entries.insert(url, entry);
        }

        // Least-recently-used eviction, on memory and on count.
        //
        // Dropping the handle frees the GPU texture and its CPU copy; the URL
        // is forgotten too, so it reloads if it ever scrolls back into view —
        // the trade a cache is for.
        //
        // Both limits, because either alone is wrong. Three hundred channel
        // logos are 12MB and evicting them achieves nothing; three hundred
        // heroes are a gigabyte and a half. The count keeps the map small, the
        // budget keeps the memory bounded, and the hero is why the second one
        // had to exist: a new one is fetched on every visit to the home screen
        // and none of them is small.
        let mut ready: Vec<(String, u64, usize)> = self
            .entries
            .iter()
            .filter_map(|(url, e)| match e {
                Entry::Ready(_, used, _, bytes, _) => Some((url.clone(), *used, *bytes)),
                _ => None,
            })
            .collect();

        let mut held: usize = ready.iter().map(|(_, _, bytes)| bytes).sum();
        if held > MAX_BYTES || ready.len() > MAX_RESIDENT {
            // Oldest first, so what goes is what nothing has looked at longest.
            ready.sort_by_key(|(_, used, _)| *used);

            // Never evict what was asked for most recently.
            //
            // Everything drawn in a frame is stamped with the same tick — the
            // increment lives at the bottom of this function, once per frame,
            // and nowhere else. That placement is load-bearing: when the tick
            // advanced on every request instead, each poster in a frame had a
            // different age, "newest" protected exactly one image, and a
            // screen over budget evicted its own working set — which was
            // requested again immediately, evicting others in turn. On a
            // large library that is a loop: artwork appearing, vanishing and
            // reappearing on a screen nobody is scrolling.
            //
            // Holding the newest tick back is what breaks it. If a single
            // frame really does want more than the budget, the limits are
            // exceeded for that frame rather than the cache eating itself.
            let newest = ready.last().map(|(_, used, _)| *used).unwrap_or(0);

            let mut count = ready.len();
            for (url, used, bytes) in ready {
                if held <= MAX_BYTES && count <= MAX_RESIDENT {
                    break;
                }
                if used == newest {
                    break;
                }
                self.entries.remove(&url);
                held = held.saturating_sub(bytes);
                count -= 1;
            }
        }

        // The next frame's requests all carry the next tick. The arrivals
        // above were stamped with the current one — the same age as the frame
        // that asked for them — so they are protected alongside it.
        self.tick += 1;
    }

    /// How much artwork is in memory: how many pictures, and how many bytes.
    ///
    /// For the log. When someone reports artwork flickering, the question is
    /// whether the cache is holding the working set or churning through it,
    /// and that is this number against how many cards are on screen.
    pub fn resident(&self) -> (usize, usize) {
        self.entries
            .values()
            .filter_map(|e| match e {
                Entry::Ready(_, _, _, bytes, _) => Some(*bytes),
                _ => None,
            })
            .fold((0, 0), |(n, total), bytes| (n + 1, total + bytes))
    }

    /// Drop every remembered failure, so the next ask fetches again.
    ///
    /// For the moment something changes that makes all of them worth
    /// retrying at once — a permission granted, a server that has come back —
    /// rather than waiting out the timer picture by picture.
    pub fn forget_failures(&mut self) {
        self.entries.retain(|_, entry| !matches!(entry, Entry::Failed(_)));
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

        // A failure is worth another go after a while. Long enough that a
        // genuinely dead URL is asked for seldom — a card on screen redraws
        // sixty times a second and must not fetch sixty times — and short
        // enough that somebody who has just granted a permission or restarted
        // their DVR sees the artwork fill in rather than wondering what is
        // wrong with the library.
        const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(20);
        if let Some(Entry::Failed(when)) = self.entries.get(url) {
            if when.elapsed() > RETRY_AFTER {
                self.entries.remove(url);
            }
        }

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
            Some(Entry::Ready(texture, used, _, _, _)) => {
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
        matches!(self.entries.get(url), Some(Entry::Ready(_, _, true, _, _)))
    }

    /// How bright the left of this artwork is, 0 to 1, once it has loaded.
    ///
    /// None while it is still arriving, which the caller should read as "no
    /// reason to darken yet" rather than "dark": the picture is not on screen
    /// either, so there is nothing to be unreadable over.
    pub fn left_luma(&self, url: &str) -> Option<f32> {
        match self.entries.get(url) {
            Some(Entry::Ready(_, _, _, _, luma)) => Some(*luma),
            _ => None,
        }
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
        left_luma: left_luma(&rgba),
        image: ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
    })
}

/// How bright the left of a picture is, 0 to 1.
///
/// Only the left 55%, and only the middle band vertically, because that is
/// where the hero puts its text: brightness in the top corner or off the right
/// edge has no bearing on whether a title is readable.
///
/// Rec. 709 luma, then sampled on a grid rather than over every pixel. A 1280
/// wide image is a million pixels and the answer to "is this side bright" does
/// not change between one pixel and the next.
fn left_luma(rgba: &image::RgbaImage) -> f32 {
    let (width, height) = (rgba.width(), rgba.height());
    if width == 0 || height == 0 {
        return 0.0;
    }
    let right = (width as f32 * 0.55) as u32;
    let (top, bottom) = (height / 5, height * 4 / 5);
    let step = (width / 64).max(1);

    let mut total = 0.0f32;
    let mut counted = 0u32;
    let mut y = top;
    while y < bottom {
        let mut x = 0;
        while x < right {
            let p = rgba.get_pixel(x, y).0;
            total += 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
            counted += 1;
            x += step;
        }
        y += step;
    }
    if counted == 0 {
        return 0.0;
    }
    (total / counted as f32 / 255.0).clamp(0.0, 1.0)
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
