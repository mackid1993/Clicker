// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native Windows client for Channels DVR
// Copyright (c) 2026 David Brustein

//! What a source is, and where to join it.
//!
//! Description, not machinery. mpv opens everything and decides for itself how
//! to read it; these two answer questions the application still has to have an
//! opinion about — what to call the source on the stats card, and whether a
//! live playlist should be joined at its head or near its edge.

/// What kind of source this is.
///
/// A label for the interface, not a decision the player acts on. Whether a
/// stream can actually be rewound is answered once it is open, because a URL
/// cannot be trusted to say: plenty of `.m3u8` playlists are sliding windows
/// that cannot seek backwards at all, and plenty of files with no extension
/// seek perfectly.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transport {
    /// One long HTTP response, such as Channels' `stream.mpg`. Lowest latency
    /// and no transcode, but there is nothing to seek within.
    Direct,
    /// A segmented playlist. Channels keeps every segment from the moment the
    /// channel was tuned, verified against the server: `EXT-X-MEDIA-SEQUENCE`
    /// stays at 1 while the list grows, so the whole session stays
    /// addressable. Other servers roll segments off the front, which is why
    /// the seekable window is read from the stream rather than assumed.
    Hls,
    /// A recording or any other addressable file, local or over HTTP.
    File,
    /// A live stream being written to a local file as it arrives.
    ///
    /// Seekable like any file, but catching up with the writer looks exactly
    /// like the end of it — so end-of-file here means "wait", not "stop".
    Timeshift,
}

impl Transport {
    pub fn of(uri: &str) -> Self {
        let lower = uri.to_ascii_lowercase();
        let path = lower.split(['?', '#']).next().unwrap_or(&lower);
        if path.ends_with(".m3u8") || path.ends_with(".m3u") || path.contains("/hls/") {
            Transport::Hls
        } else if path.ends_with(".mpg") || path.ends_with(".ts") || path.contains("stream.mpg") {
            Transport::Direct
        } else {
            Transport::File
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Transport::Direct => "Direct",
            Transport::Hls => "HLS",
            Transport::File => "File",
            Transport::Timeshift => "Direct + buffer",
        }
    }
}

/// Where to join a live playlist.
///
/// Only meaningful for HLS; every other source ignores it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JoinAt {
    /// The head of the playlist. Right for a fresh tune: Channels' playlist
    /// begins at the moment of tuning, so the head is a few seconds back and
    /// those seconds are a buffer held on the server rather than in memory.
    Start,
    /// Near the live edge. Right for re-opening a channel already playing —
    /// changing quality — where the playlist has been accumulating for as long
    /// as the channel has been on and its head is no longer anywhere near now.
    LiveEdge,
}

impl JoinAt {
    /// The HLS demuxer's `live_start_index`, which mpv passes through.
    ///
    /// Left at the head for a fresh tune, because the default is three
    /// segments from the end and that would throw away the server-side buffer
    /// that makes a channel rewindable the moment it comes on.
    pub fn live_start_index(self) -> i32 {
        match self {
            JoinAt::Start => 0,
            // Four segments back, not the last one. The final segment is the
            // one the server is still writing, and reads of it block until it
            // is finished; a few segments of margin is what keeps the fetch
            // hitting files that are already complete.
            JoinAt::LiveEdge => -4,
        }
    }
}
