// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native Windows client for Channels DVR
// Copyright (c) 2026 David Brustein

//! Where Clicker keeps things.
//!
//! Two homes: settings under the profile root that roams with the user, and
//! everything large or rebuildable — downloads, the library cache, the
//! timeshift buffer, the crash log — under the machine-local one. Which
//! directories those actually are is the platform's answer: `APPDATA` and
//! `LOCALAPPDATA` on Windows, `~/Library/Application Support` on macOS, the
//! XDG pair on Linux.
//!
//! The earliest builds wrote to a directory under a different name, so it is
//! taken over on the way past. Moved rather than copied — a rename is atomic
//! and instant whatever it holds, which matters when it holds gigabytes of
//! recordings — and attempted once per run behind a `OnceLock`, because these
//! are asked for on every settings save and every buffer that starts.

use std::path::PathBuf;
use std::sync::OnceLock;

/// Deleting this constant deletes the takeover with it, which is the intention
/// once no install is old enough to need it.
const FORMER: &str = "RustDVR";
const CURRENT: &str = "Clicker";

/// Settings. Roams with the user profile.
pub fn config_dir() -> Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| home(crate::platform::config_home())).clone()
}

/// Downloads, the library cache, timeshift buffers, the crash log. Local to
/// the machine, because none of it is worth roaming and some of it is enormous.
pub fn data_dir() -> Option<PathBuf> {
    DIR.get_or_init(|| home(crate::platform::data_home())).clone()
}

static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Point everything above at a directory of the user's choosing.
///
/// Must be called before anything reads `data_dir`, because the answer is
/// memoized on first use — and the logger and the crash handler both ask
/// early, by design, since they have to work before anything else does. Call
/// it immediately after the settings are loaded and before anything else.
/// Returns whether it took, so a late call is a visible failure rather than a
/// setting that silently does nothing.
pub fn set_data_dir(dir: PathBuf) -> bool {
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    DIR.set(Some(dir)).is_ok()
}

/// The directory under one of the profile roots, after taking over whatever
/// the old name left there.
fn home(base: Option<PathBuf>) -> Option<PathBuf> {
    let base = base?;
    let current = base.join(CURRENT);
    let former = base.join(FORMER);

    // Only when there is something to move and nothing to overwrite. If both
    // exist, the new one is authoritative and the old one is left alone rather
    // than merged — merging two states silently is how a settings file ends up
    // disagreeing with the downloads beside it.
    if !current.exists() && former.is_dir() {
        if std::fs::rename(&former, &current).is_err() {
            // The move failed — most likely something in there is open in
            // another copy of the program. Keep using the old home rather than
            // starting empty beside it: stale naming is a cosmetic problem,
            // and losing a library is not.
            return Some(former);
        }
    }
    Some(current)
}
