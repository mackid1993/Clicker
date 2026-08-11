// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native client for Channels DVR
// Copyright (c) 2026 David Brustein

//! Linux. Written to the XDG conventions and the two GL worlds — EGL under
//! Wayland, GLX under X11 — without trying to guess which one the session is;
//! the loader asks both.

use std::ffi::{c_char, c_int, c_void};
use std::path::PathBuf;

// --- window chrome -----------------------------------------------------------

/// Drawn by the application, as on Windows. Client-side decorations are the
/// norm on the free desktops anyway — GNOME apps draw their own headers —
/// so a window providing its own frame is at home here.
pub const NATIVE_FRAME: bool = false;

/// Nothing sits in the top-left corner but our own title.
pub const CAPTION_INSET: f32 = 0.0;

/// An undecorated window, everything inside it ours to draw.
pub fn shape_window(viewport: eframe::egui::ViewportBuilder) -> eframe::egui::ViewportBuilder {
    viewport.with_decorations(false)
}

/// Nothing stands between this program and the local network here.
pub const LOCAL_NETWORK_HINT: &str = "";

/// Nothing to request: there is no local network permission here.
pub fn request_local_network() {}

// --- where files go ----------------------------------------------------------

fn home() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?))
}

fn xdg(variable: &str, fallback: &[&str]) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(variable) {
        let dir = PathBuf::from(dir);
        if dir.is_absolute() {
            return Some(dir);
        }
    }
    let mut dir = home()?;
    for part in fallback {
        dir = dir.join(part);
    }
    Some(dir)
}

/// Settings, under `XDG_CONFIG_HOME`.
pub fn config_home() -> Option<PathBuf> {
    xdg("XDG_CONFIG_HOME", &[".config"])
}

/// Downloads, caches, buffers and logs, under `XDG_DATA_HOME` rather than
/// `XDG_CACHE_HOME`: offline downloads are whole recordings someone means to
/// keep, and the cache directory is fair game for every cleanup tool.
pub fn data_home() -> Option<PathBuf> {
    xdg("XDG_DATA_HOME", &[".local", "share"])
}

// --- fonts -------------------------------------------------------------------

/// egui's bundled face, by returning nothing. A Linux desktop has no one
/// system font to read the way Windows has Segoe and macOS has San
/// Francisco, and guessing at distribution font paths buys inconsistency.
pub fn text_font() -> Option<Vec<u8>> {
    None
}

/// The bundled Fluent UI System Icons subset — Microsoft's own icon set,
/// MIT-licensed, cut down to the twenty-eight glyphs the interface draws.
/// See `theme::icon` for the codepoint table it must stay in step with, and
/// `licenses/FluentSystemIcons-MIT.txt` for its terms.
pub fn icon_font() -> Option<Vec<u8>> {
    Some(include_bytes!("../../assets/FluentIcons-Clicker.ttf").to_vec())
}

// --- the live buffer's disk tricks -------------------------------------------

/// Nothing to do: every Linux filesystem this will meet — ext4, btrfs, xfs —
/// keeps files sparse by construction.
pub fn make_sparse(_file: &tokio::fs::File) {}

extern "C" {
    fn fallocate(fd: c_int, mode: c_int, offset: i64, len: i64) -> c_int;
}

const FALLOC_FL_KEEP_SIZE: c_int = 0x01;
const FALLOC_FL_PUNCH_HOLE: c_int = 0x02;

/// Give a range at the front of the buffer back to the disk.
///
/// `fallocate` takes byte offsets and rounds to blocks itself — partial
/// blocks are zeroed, whole blocks released — so unlike the macOS call there
/// is no alignment to do here. KEEP_SIZE because the demuxer holds byte
/// offsets into this file and the file must not shrink under it.
pub fn punch_hole(file: &tokio::fs::File, from: u64, len: u64) -> bool {
    use std::os::unix::io::AsRawFd;

    if len == 0 {
        return false;
    }
    unsafe {
        fallocate(
            file.as_raw_fd(),
            FALLOC_FL_PUNCH_HOLE | FALLOC_FL_KEEP_SIZE,
            from as i64,
            len as i64,
        ) == 0
    }
}

// --- libmpv and OpenGL -------------------------------------------------------

/// What the mpv library is called here, for messages about not finding it.
pub const MPV_LIBRARY: &str = "libmpv.so.2";

/// Where libmpv might be: beside the binary, then the system linker paths,
/// which is where a distribution package or a Flatpak runtime puts it.
pub fn mpv_candidates() -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)) {
        candidates.push(dir.join(MPV_LIBRARY).display().to_string());
    }
    candidates.push(MPV_LIBRARY.to_string());
    candidates.push("libmpv.so".to_string());
    candidates
}

/// A GL loader function, resolved out of a library once.
fn loader(library: &str, symbol: &[u8]) -> Option<unsafe extern "C" fn(*const c_char) -> *mut c_void> {
    let module = super::open_library(library);
    if module.is_null() {
        return None;
    }
    let found = unsafe { super::library_symbol(module, symbol.as_ptr()) };
    if found.is_null() {
        return None;
    }
    Some(unsafe { std::mem::transmute(found) })
}

/// How mpv finds the OpenGL functions of the context eframe created.
///
/// The session decides whether that context is EGL or GLX, and this does not
/// know which, so it asks in order: `eglGetProcAddress`, `glXGetProcAddressARB`,
/// then a plain `dlsym` into libGL for the 1.1 entry points the older
/// loaders decline to return.
pub unsafe extern "C" fn gl_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    type Loader = unsafe extern "C" fn(*const c_char) -> *mut c_void;
    static LOADERS: std::sync::OnceLock<(Option<Loader>, Option<Loader>, usize)> =
        std::sync::OnceLock::new();
    let (egl, glx, libgl) = *LOADERS.get_or_init(|| {
        (
            loader("libEGL.so.1", b"eglGetProcAddress\0"),
            loader("libGL.so.1", b"glXGetProcAddressARB\0"),
            super::open_library("libGL.so.1") as usize,
        )
    });

    if let Some(egl) = egl {
        let found = egl(name);
        if !found.is_null() {
            return found;
        }
    }
    if let Some(glx) = glx {
        let found = glx(name);
        if !found.is_null() {
            return found;
        }
    }
    let libgl = libgl as *mut c_void;
    if libgl.is_null() {
        return std::ptr::null_mut();
    }
    super::library_symbol(libgl, name as *const u8)
}
