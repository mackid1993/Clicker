// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native client for Channels DVR
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
//!
//! Each platform file implements the full set. A stub is an honest
//! implementation where the feature has nothing to do — macOS windows come
//! rounded — and a `None` where a capability is genuinely absent, which the
//! caller must already handle because Windows callers handled it first.

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
