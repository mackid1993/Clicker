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

/// The menu bar's accelerators, by the action id they belong to.
///
/// One table, read twice: [`install_menu_bar`] turns each into a real
/// accelerator, and the settings page prints it beside the rebindable key, so
/// the two lists cannot describe different keyboards.
///
/// The strings are what a Mac shows — ⌘ ⇧ ⌃ ⌥ — because that is what has to
/// appear on screen; [`accelerator`] reads them back to build the menu.
pub const MENU_SHORTCUTS: &[(&str, &str)] = &[
    ("home", "\u{2318}1"),
    ("guide", "\u{2318}2"),
    ("library", "\u{2318}3"),
    ("recordings", "\u{2318}4"),
    ("downloads", "\u{2318}5"),
    ("settings", "\u{2318},"),
    ("refresh", "\u{2318}R"),
    ("back", "\u{2318}\u{2190}"),
    ("forward", "\u{2318}\u{2192}"),
    ("volume_up", "\u{2318}\u{2191}"),
    ("volume_down", "\u{2318}\u{2193}"),
    ("mute", "\u{21e7}\u{2318}M"),
    ("channel_up", "\u{2318}["),
    ("channel_down", "\u{2318}]"),
    ("stop", "\u{2318}."),
    ("rail", "\u{2303}\u{2318}S"),
    // The system's own item provides this one; it is here so the settings
    // page can say so.
    ("fullscreen", "\u{2303}\u{2318}F"),
];

/// What the menu bar offers for an action, if anything.
pub fn menu_shortcut(id: &str) -> Option<&'static str> {
    MENU_SHORTCUTS
        .iter()
        .find(|(action, _)| *action == id)
        .map(|(_, keys)| *keys)
}

/// Turn a printed shortcut back into an accelerator.
///
/// Reading the display string rather than keeping a second list of key codes
/// beside it: two lists is how a menu ends up promising ⌘4 and doing ⌘5.
fn accelerator(id: &str) -> Option<muda::accelerator::Accelerator> {
    use muda::accelerator::{Accelerator, Code, Modifiers};

    let printed = menu_shortcut(id)?;
    let mut modifiers = Modifiers::empty();
    let mut last = None;
    for character in printed.chars() {
        match character {
            '\u{2318}' => modifiers |= Modifiers::META,
            '\u{21e7}' => modifiers |= Modifiers::SHIFT,
            '\u{2303}' => modifiers |= Modifiers::CONTROL,
            '\u{2325}' => modifiers |= Modifiers::ALT,
            other => last = Some(other),
        }
    }
    let code = match last? {
        '1' => Code::Digit1,
        '2' => Code::Digit2,
        '3' => Code::Digit3,
        '4' => Code::Digit4,
        '5' => Code::Digit5,
        ',' => Code::Comma,
        '.' => Code::Period,
        '[' => Code::BracketLeft,
        ']' => Code::BracketRight,
        '\u{2190}' => Code::ArrowLeft,
        '\u{2192}' => Code::ArrowRight,
        '\u{2191}' => Code::ArrowUp,
        '\u{2193}' => Code::ArrowDown,
        'F' => Code::KeyF,
        'M' => Code::KeyM,
        'R' => Code::KeyR,
        'S' => Code::KeyS,
        _ => return None,
    };
    Some(Accelerator::new(Some(modifiers), code))
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
/// items. The rest come back through [`menu_command`], identified by the same
/// action ids `keys::ACTIONS` uses — so a menu click and the key it names
/// travel one path through `handle_keys` and cannot disagree.
///
/// **The accelerators here are not the rebindable shortcuts**, and the two
/// are deliberately different things:
///
///   * The settings page rebinds *bare keys* — G for the guide, Space for
///     pause. They are read by the application, which knows when something is
///     being typed into and stands aside.
///   * A menu accelerator is owned by the system, which presses it before any
///     window sees the keystroke. That is why every one carries Command: an
///     unmodified menu accelerator would eat the key everywhere, including
///     inside the search box. It is also why Play or Pause has none — its key
///     is Space, and a menu that owned Space would mean no spaces typed
///     anywhere in the program.
///
/// So an action can have both, and usually does: Command-2 from the menu bar
/// and whatever the settings page says, side by side. Rebinding cannot break
/// the menu, and the menu cannot break typing. What the menu deliberately
/// does *not* do is relabel itself when a binding changes — a Mac menu that
/// renamed its own shortcuts would be lying about what the system will do.
///
/// Must be called on the main thread, after the window exists.
pub fn install_menu_bar() {
    use muda::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};

    fn item(id: &str, label: &str) -> MenuItem {
        MenuItem::with_id(id, label, true, accelerator(id))
    }

    let about = AboutMetadata {
        name: Some(crate::APP_NAME.into()),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        copyright: Some("Copyright (c) 2026 David Brustein. MIT.".into()),
        ..Default::default()
    };

    let settings = item("settings", "Settings\u{2026}");
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

    // The screens, on the numbers. Every Mac application with tabs or views
    // puts them on Command-1 upward, and five screens are exactly that shape.
    let go_items = [
        item("home", "Home"),
        item("guide", "Guide"),
        item("library", "Library"),
        item("recordings", "Recordings"),
        item("downloads", "Downloads"),
    ];
    let go = Submenu::new("Go", true);
    for entry in &go_items {
        let _ = go.append(entry);
    }

    let play_items = [
        item("play", "Play or Pause"),
        item("back", "Skip Back"),
        item("forward", "Skip Forward"),
        item("volume_up", "Volume Up"),
        item("volume_down", "Volume Down"),
        item("mute", "Mute"),
        item("channel_up", "Previous Channel"),
        item("channel_down", "Next Channel"),
        item("stop", "Stop Playback"),
    ];
    let playback = Submenu::new("Playback", true);
    let _ = playback.append(&play_items[0]);
    let _ = playback.append(&PredefinedMenuItem::separator());
    for entry in &play_items[1..6] {
        let _ = playback.append(entry);
    }
    let _ = playback.append(&PredefinedMenuItem::separator());
    for entry in &play_items[6..] {
        let _ = playback.append(entry);
    }

    let refresh = item("refresh", "Refresh from the DVR");
    let rail = item("rail", "Show or Hide Labels");
    let view = Submenu::new("View", true);
    let _ = view.append_items(&[
        &refresh,
        &PredefinedMenuItem::separator(),
        &rail,
        &PredefinedMenuItem::separator(),
        // Control-Command-F, from the system, and the same item every other
        // Mac application has.
        &PredefinedMenuItem::fullscreen(None),
    ]);

    let window = Submenu::new("Window", true);
    let _ = window.append_items(&[
        &PredefinedMenuItem::minimize(None),
        &PredefinedMenuItem::maximize(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::close_window(None),
    ]);

    let github = MenuItem::with_id(
        "github",
        &format!("{} on GitHub", crate::APP_NAME),
        true,
        None,
    );
    let help = Submenu::new("Help", true);
    let _ = help.append(&github);

    let menu = Menu::new();
    let _ = menu.append_items(&[&app, &edit, &go, &playback, &view, &window, &help]);
    let _ = menu.init_for_nsapp();
    // The Window menu has to be *named* to the system, or the window list and
    // its Bring All to Front stay empty.
    window.set_as_windows_menu_for_nsapp();

    // Leaked deliberately. These own the native menu objects, and dropping
    // them at the end of this function would tear the menu bar down again.
    std::mem::forget((menu, app, edit, go, playback, view, window, help));
    std::mem::forget((settings, refresh, rail, github, go_items, play_items));
}

/// Whatever the menu was asked for since the last frame, if anything.
///
/// The string is an action id from `keys::ACTIONS`, or one of the few this
/// module invents. Polled rather than delivered, because the menu's channel
/// and egui's frame loop are separate worlds and this is the seam.
pub fn menu_command() -> Option<String> {
    let event = muda::MenuEvent::receiver().try_recv().ok()?;
    Some(event.id.0)
}

/// Hand a link to the desktop's browser.
pub fn open_url(url: &str) {
    let _ = std::process::Command::new("open").arg(url).spawn();
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
