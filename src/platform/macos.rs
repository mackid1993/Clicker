// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native client for Channels DVR
// Copyright (c) 2026 David Brustein

//! macOS, Apple silicon only. The Intel Macs are on their way out and a
//! universal binary would double the untested surface; one architecture,
//! actually run, beats two shipped on faith.

use std::ffi::{c_char, c_int, c_void};
use std::path::PathBuf;

// --- window chrome -----------------------------------------------------------

/// The system owns the frame here. Traffic lights, rounded corners, the
/// shadow, edge resizing — a Mac window without them reads as a Windows app
/// visiting, however good the interior is.
pub const NATIVE_FRAME: bool = true;

/// Room for the traffic lights, which sit over the top-left of our surface.
/// The title text starts past them rather than under them.
pub const CAPTION_INSET: f32 = 80.0;

/// Native decorations with the titlebar dissolved into the content: the
/// traffic lights float over our surface, the system draws the corners and
/// shadow and handles edge resizing, and everything else on screen is still
/// ours. This is the same hybrid every polished Mac port uses, and it is the
/// difference between "a Windows app running on a Mac" and a Mac window with
/// the same soul.
pub fn shape_window(viewport: eframe::egui::ViewportBuilder) -> eframe::egui::ViewportBuilder {
    viewport
        .with_decorations(true)
        .with_fullsize_content_view(true)
        // "Not shown" maps to a transparent titlebar, not a missing one:
        // the buttons stay, the material behind them goes.
        .with_titlebar_shown(false)
        .with_title_shown(false)
}

// --- the menu bar ------------------------------------------------------------

/// What a menu item asked for, when it is not something the system handles
/// on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCommand {
    OpenSettings,
    Refresh,
}

/// Build the application menu and hand it to the running NSApplication.
///
/// macOS puts the menu above the screen rather than inside the window, and a
/// window whose application has no menus is not a minimal Mac app — it is a
/// broken one. There is no Quit, no ⌘W, no ⌘Q, no Services, and the menu bar
/// shows the name with nothing under it, which is the single loudest way to
/// say "ported without looking".
///
/// Almost every item here is one of the system's own: Apple implements Hide,
/// Minimize, Full Screen, the clipboard verbs and Quit, and they behave
/// exactly as they do in every other application because they *are* the same
/// items. Only Settings and Refresh belong to this program, and those come
/// back through [`menu_command`].
///
/// Must be called on the main thread, after the window exists.
pub fn install_menu_bar() {
    use muda::accelerator::{Accelerator, Code, Modifiers};
    use muda::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};

    let about = AboutMetadata {
        name: Some(crate::APP_NAME.into()),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        copyright: Some("Copyright (c) 2026 David Brustein. MIT.".into()),
        ..Default::default()
    };

    let settings = MenuItem::with_id(
        SETTINGS_ID,
        "Settings…",
        true,
        Some(Accelerator::new(Some(Modifiers::META), Code::Comma)),
    );
    let refresh = MenuItem::with_id(
        REFRESH_ID,
        "Refresh from the DVR",
        true,
        Some(Accelerator::new(Some(Modifiers::META), Code::KeyR)),
    );

    let app = Submenu::new(crate::APP_NAME, true);
    let _ = app.append_items(&[
        &PredefinedMenuItem::about(Some(&format!("About {}", crate::APP_NAME)), Some(about)),
        &PredefinedMenuItem::separator(),
        &settings,
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::services(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::hide(None),
        &PredefinedMenuItem::hide_others(None),
        &PredefinedMenuItem::show_all(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::quit(None),
    ]);

    // The clipboard verbs. egui handles the keystrokes itself, but a Mac
    // application without an Edit menu cannot be driven by anything that
    // reads the menus — Services, scripting, or a person who learned the
    // interface through them.
    let edit = Submenu::new("Edit", true);
    let _ = edit.append_items(&[
        &PredefinedMenuItem::undo(None),
        &PredefinedMenuItem::redo(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::cut(None),
        &PredefinedMenuItem::copy(None),
        &PredefinedMenuItem::paste(None),
        &PredefinedMenuItem::select_all(None),
    ]);

    let view = Submenu::new("View", true);
    let _ = view.append_items(&[&refresh, &PredefinedMenuItem::separator(), &PredefinedMenuItem::fullscreen(None)]);

    let window = Submenu::new("Window", true);
    let _ = window.append_items(&[
        &PredefinedMenuItem::minimize(None),
        &PredefinedMenuItem::maximize(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::close_window(None),
    ]);

    let menu = Menu::new();
    let _ = menu.append_items(&[&app, &edit, &view, &window]);
    let _ = menu.init_for_nsapp();
    // The Window menu has to be *named* to the system, or the window list and
    // its Bring All to Front stay empty.
    window.set_as_windows_menu_for_nsapp();

    // Leaked deliberately. These own the native menu objects, and dropping
    // them at the end of this function would tear the menu bar down again.
    std::mem::forget((menu, app, edit, view, window, settings, refresh));
}

const SETTINGS_ID: &str = "clicker.settings";
const REFRESH_ID: &str = "clicker.refresh";

/// Whatever the menu was asked for since the last frame, if anything.
///
/// Polled rather than delivered, because the menu's channel and egui's frame
/// loop are separate worlds and this is the seam between them.
pub fn menu_command() -> Option<MenuCommand> {
    let event = muda::MenuEvent::receiver().try_recv().ok()?;
    match event.id.0.as_str() {
        SETTINGS_ID => Some(MenuCommand::OpenSettings),
        REFRESH_ID => Some(MenuCommand::Refresh),
        _ => None,
    }
}

/// Appended to a failed connection attempt.
///
/// macOS gates the local network behind a permission prompt, and a probe
/// that runs before the dialog is answered fails on the user's behalf.
/// `request_local_network` below asks early so this rarely happens, but
/// someone who dismissed the dialog still deserves an explanation over a
/// mystery.
pub const LOCAL_NETWORK_HINT: &str =
    " If macOS asked about the local network, allow it and try again.";

/// Make macOS raise its local network permission prompt now, at launch.
///
/// The system shows that dialog on the first packet an app aims at the
/// local network — which, left alone, is the first attempt to reach the
/// DVR: Connect fails, and *then* the question appears, in that order. One
/// throwaway datagram at the mDNS multicast address is unambiguously
/// "local network" to the classifier, costs nothing, and moves the question
/// to while the welcome card is still being read. Asked once per launch;
/// the system only ever prompts the first time.
pub fn request_local_network() {
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        let _ = socket.send_to(&[0u8], "224.0.0.251:5353");
    }
}

// --- where files go ----------------------------------------------------------

fn home() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?))
}

/// Settings live in Application Support, which is where a Mac keeps a
/// program's own files.
pub fn config_home() -> Option<PathBuf> {
    Some(home()?.join("Library").join("Application Support"))
}

/// The same root as the settings, deliberately not `~/Library/Caches`. The
/// "data" here includes offline downloads — whole recordings someone means to
/// watch on a plane — and Caches is the one folder every cleanup utility
/// feels entitled to empty.
pub fn data_home() -> Option<PathBuf> {
    config_home()
}

// --- fonts -------------------------------------------------------------------

/// The system face, read from the system. San Francisco is what every other
/// window on this desktop is set in; falling back to egui's bundled face is
/// legible, just visibly a guest.
pub fn text_font() -> Option<Vec<u8>> {
    for candidate in [
        "/System/Library/Fonts/SFNS.ttf",
        "/System/Library/Fonts/HelveticaNeue.ttc",
    ] {
        if let Ok(bytes) = std::fs::read(candidate) {
            return Some(bytes);
        }
    }
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

/// Nothing to do: APFS files are sparse by construction. Writing at an offset
/// past the end allocates nothing for the gap, which is the property the
/// Windows ioctl has to ask for.
pub fn make_sparse(_file: &tokio::fs::File) {}

extern "C" {
    fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
}

/// `F_PUNCHHOLE`, and the struct it reads its range from.
const F_PUNCHHOLE: c_int = 99;

#[repr(C)]
struct PunchHole {
    fp_flags: u32,
    reserved: u32,
    fp_offset: i64,
    fp_length: i64,
}

/// Give a range at the front of the buffer back to the disk.
///
/// APFS insists both edges land on filesystem blocks, where the Windows call
/// takes any byte range, so the range is shrunk inward to block boundaries.
/// Punching slightly less than asked is fine — release is already best
/// effort — and four kilobytes of un-released tail is not worth an EINVAL
/// that releases nothing.
pub fn punch_hole(file: &tokio::fs::File, from: u64, len: u64) -> bool {
    use std::os::unix::io::AsRawFd;

    const BLOCK: u64 = 4096;
    let start = from.next_multiple_of(BLOCK);
    let end = (from + len) / BLOCK * BLOCK;
    if end <= start {
        return false;
    }

    let hole = PunchHole {
        fp_flags: 0,
        reserved: 0,
        fp_offset: start as i64,
        fp_length: (end - start) as i64,
    };
    unsafe { fcntl(file.as_raw_fd(), F_PUNCHHOLE, &hole as *const PunchHole) == 0 }
}

// --- libmpv and OpenGL -------------------------------------------------------

/// What the mpv library is called here, for messages about not finding it.
pub const MPV_LIBRARY: &str = "libmpv.2.dylib";

/// Where libmpv might be, most specific first: inside the app bundle, beside
/// a bare binary, then Homebrew — which on Apple silicon lives under
/// /opt/homebrew and nowhere else — and finally wherever the loader's own
/// search paths reach.
pub fn mpv_candidates() -> Vec<String> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from));
    let mut candidates = Vec::new();
    if let Some(dir) = &exe_dir {
        // Contents/MacOS/../Frameworks is where a .app carries its libraries.
        candidates.push(dir.join("../Frameworks").join(MPV_LIBRARY).display().to_string());
        candidates.push(dir.join(MPV_LIBRARY).display().to_string());
    }
    candidates.push(format!("/opt/homebrew/lib/{MPV_LIBRARY}"));
    candidates.push(MPV_LIBRARY.to_string());
    candidates.push("libmpv.dylib".to_string());
    candidates
}

/// How mpv finds the OpenGL functions of the context eframe created.
///
/// One door: every GL symbol on macOS, 1.1 and modern alike, lives in the
/// OpenGL framework, and `dlsym` against it answers for all of them. No
/// wgl/glX split to reconcile. Deprecated, yes — Apple froze GL at 4.1 — but
/// frozen is exactly what a lookup table wants to be.
pub unsafe extern "C" fn gl_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    static OPENGL: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let module = *OPENGL.get_or_init(|| {
        super::open_library("/System/Library/Frameworks/OpenGL.framework/Versions/A/OpenGL")
            as usize
    }) as *mut c_void;
    if module.is_null() {
        return std::ptr::null_mut();
    }
    super::library_symbol(module, name as *const u8)
}
