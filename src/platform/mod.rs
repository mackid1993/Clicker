// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial client for Channels DVR Server
// Copyright (c) 2026 David Brustein

//! The one door to the operating system.
//!
//! Everything platform-specific lives behind this module, and nothing outside
//! it may say `#[cfg(windows)]` about behavior. The interface is deliberately
//! narrow — a handful of free functions — because every one of them is a
//! promise all three platforms have to keep, and a wide interface is how a
//! port rots into a fork.
//!
//! What lives here, and why it is exactly this list:
//!
//!   * **window chrome** — dark title bars and rounded corners are DWM
//!     attributes on Windows and free everywhere else
//!   * **the window handle and restoring it** — the tray's Open needs to raise
//!     a hidden window from another thread, which no portable API does
//!   * **where files go** — `APPDATA` against `~/Library` against XDG
//!   * **fonts** — the interface reads its faces from the system it is on,
//!     rather than shipping what a platform already has
//!   * **loading libmpv and OpenGL** — `LoadLibrary` against `dlopen`, and
//!     each platform's own way of asking a GL context for a function
//!   * **a second GL context for the render thread** — `gl_share` on the
//!     interface thread, then `gl_worker_begin` and `gl_worker_end` on the
//!     worker. Three APIs for one idea, which is why it is here: EGL on
//!     Linux, WGL on Windows, CGL on macOS. Returning `None` is allowed and
//!     costs only speed — the player falls back to rendering inside the paint
//!   * **the live buffer's disk tricks** — sparse files and hole punching are
//!     ioctls on Windows, `fcntl` on macOS, `fallocate` on Linux
//!   * **the clock's opinion of local time** — see `local_utc_offset_seconds`
//!   * **thread accounting** — processor time for the render log
//!   * **waking a starved window** — `request_window_paint`, an
//!     `InvalidateRect` on the one platform whose paint can be starved by a
//!     busy sibling window; a no-op everywhere else
//!   * **who decides what stays on top** — `desktop_owns_stacking`, a fact
//!     about the desktop rather than a lever: the picture-in-picture window
//!     asks to float everywhere, and this says whether the ask lands, which
//!     is only ever "no" under Wayland
//!   * **dressing the picture's own window** — `dress_pip`, the DWM corner
//!     attribute on a window eframe never hands back a handle for. A window
//!     is created visible and is right from its first frame on every platform
//!     here; nothing shows, hides, or watches over it afterwards
//!
//! Each platform file implements the full set. A stub is an honest
//! implementation where the feature has nothing to do — macOS windows come
//! rounded — and a `None` where a capability is genuinely absent, which the
//! caller must already handle because Windows callers handled it first.

/// What an OpenGL implementation says it is, when asked before there is a
/// window to ask through.
///
/// Both halves are used and by different callers: `major` is what the choice
/// of renderer turns on — 1 is the software OpenGL Windows falls back to when
/// there is no driver, and nothing here can draw on it — and `identity` is the
/// vendor, renderer and version as three strings, for the log and for telling
/// somebody what their machine offered.
pub struct GlReport {
    pub identity: String,
    pub major: u32,
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

// The POSIX platforms share their bones — dlopen, localtime_r, thread clocks —
// and differ in paths, fonts, OpenGL, and how a file gives disk back.
#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
