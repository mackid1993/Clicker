// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native Windows client for Channels DVR
// Copyright (c) 2026 David Brustein

//! A log file, because a windowed application has no console.
//!
//! The player already reports everything anyone needs to diagnose a stutter: a
//! line every two seconds carrying read, decode and conversion cost, the worst
//! single read, queue depths, dropped and starved frames, audio underruns,
//! timeline discontinuities, A/V skew and the clock's rate. On a `cargo run`
//! that lands in the terminal and is genuinely useful.
//!
//! A release build is compiled with `windows_subsystem = "windows"` and has no
//! console attached, so every one of those lines is written to a handle nobody
//! is holding. Which means that when someone reports that playback stutters,
//! the numbers that would identify the cause were produced, on their machine,
//! at the moment it happened, and then thrown away — and the only way to get
//! them is to talk that person through a terminal and a shell redirect, which
//! most will not do.
//!
//! So the same lines go to a file beside `crash.log`, and a bug report becomes
//! "send me that file".
//!
//! Deliberately not a logging framework. There are no levels, no filtering and
//! no configuration: everything the program already decided was worth printing
//! is worth keeping, and anything with a knob on it is a knob that will be set
//! wrong on the one machine where the fault appears.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;
use std::time::SystemTime;

/// How large the file may get before it is rolled over, in bytes.
///
/// The player writes about 200 bytes every two seconds while something is
/// playing, so this is roughly a hundred hours of watching. One generation is
/// kept behind it: enough that a fault which happened yesterday is still there,
/// bounded enough that this can never be the reason a disk fills.
const MAX_BYTES: u64 = 2 * 1024 * 1024;

static FILE: Mutex<Option<File>> = Mutex::new(None);

/// Where the log lives. Beside the crash log and the settings, so "send me
/// everything in that folder" collects all of it.
pub fn path() -> Option<std::path::PathBuf> {
    Some(crate::paths::data_dir()?.join("player.log"))
}

/// Write one line, with the time it happened.
///
/// Failures are ignored on purpose. Logging is not important enough to
/// interrupt playback over, and a program that panics because it could not
/// write a diagnostic has turned its diagnostics into a fault of their own.
pub fn line(text: &str) {
    let Ok(mut held) = FILE.lock() else { return };

    if held.is_none() {
        *held = open();
    }
    let Some(file) = held.as_mut() else { return };

    // Local time would be friendlier to read and needs a time zone database to
    // get right. Seconds since the epoch are unambiguous, sort correctly, and
    // subtract from each other, which is what these are actually used for.
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);

    let _ = writeln!(file, "{stamp:.3} {}", text.trim_end());
    let _ = file.flush();
}

/// Open the file, rolling the previous one aside if it has grown too large.
fn open() -> Option<File> {
    let path = path()?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if std::fs::metadata(&path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        // Replaced rather than appended to, so there is exactly one generation
        // behind the current file and never a directory slowly filling with
        // numbered logs.
        let _ = std::fs::rename(&path, path.with_extension("log.old"));
    }

    let mut file = OpenOptions::new().create(true).append(true).open(&path).ok()?;

    // A header per run, so it is obvious where one session ends and the next
    // begins in a file that spans weeks, and so the version is attached to
    // every report without anyone having to be asked for it.
    let _ = writeln!(
        file,
        "\n=== {} {} started ===",
        crate::APP_NAME,
        env!("CARGO_PKG_VERSION")
    );
    Some(file)
}

/// Print to stderr and to the log file.
///
/// Both, rather than one or the other: a `cargo run` should still put it in the
/// terminal where it can be watched live, and a shipped build should still keep
/// it where it can be sent.
macro_rules! logline {
    ($($arg:tt)*) => {{
        let text = format!($($arg)*);
        eprintln!("{text}");
        crate::log::line(&text);
    }};
}

pub(crate) use logline;
