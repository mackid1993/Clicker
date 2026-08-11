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

// The menu items this program owns, kept so their accelerators can be
// changed when somebody rebinds one. A menu is built once and lives for the
// life of the application, so the items have to be reachable afterwards;
// without this the menu could only ever show the shortcuts it was born with.
//
// Thread-local rather than static, because a menu item is not `Send`: it
// belongs to the main thread, which is the only thread that may touch a menu
// at all. Both the building and the syncing happen there.
thread_local! {
    static ITEMS: std::cell::RefCell<Vec<(String, muda::MenuItem)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The two accelerators this program does not let go of.
///
/// Command-comma is Settings on every Mac ever made, and Command-R is
/// refresh in everything that fetches. They are not in the rebindable table
/// at all — Refresh has no key of its own — so they are written here.
fn fixed(id: &str) -> Option<(muda::accelerator::Modifiers, muda::accelerator::Code)> {
    use muda::accelerator::{Code, Modifiers};
    match id {
        "settings" => Some((Modifiers::META, Code::Comma)),
        "refresh" => Some((Modifiers::META, Code::KeyR)),
        _ => None,
    }
}

/// egui's name for a key, as muda's code for the same key.
///
/// Only the keys a person might reasonably put a shortcut on. Anything not
/// here simply gets no menu accelerator, which is a menu item that still
/// works by clicking — the honest outcome for a key the menu cannot express.
fn code_for(key: eframe::egui::Key) -> Option<muda::accelerator::Code> {
    use eframe::egui::Key;
    use muda::accelerator::Code;
    Some(match key {
        Key::A => Code::KeyA, Key::B => Code::KeyB, Key::C => Code::KeyC,
        Key::D => Code::KeyD, Key::E => Code::KeyE, Key::F => Code::KeyF,
        Key::G => Code::KeyG, Key::H => Code::KeyH, Key::I => Code::KeyI,
        Key::J => Code::KeyJ, Key::K => Code::KeyK, Key::L => Code::KeyL,
        Key::M => Code::KeyM, Key::N => Code::KeyN, Key::O => Code::KeyO,
        Key::P => Code::KeyP, Key::Q => Code::KeyQ, Key::R => Code::KeyR,
        Key::S => Code::KeyS, Key::T => Code::KeyT, Key::U => Code::KeyU,
        Key::V => Code::KeyV, Key::W => Code::KeyW, Key::X => Code::KeyX,
        Key::Y => Code::KeyY, Key::Z => Code::KeyZ,
        Key::Num0 => Code::Digit0, Key::Num1 => Code::Digit1,
        Key::Num2 => Code::Digit2, Key::Num3 => Code::Digit3,
        Key::Num4 => Code::Digit4, Key::Num5 => Code::Digit5,
        Key::Num6 => Code::Digit6, Key::Num7 => Code::Digit7,
        Key::Num8 => Code::Digit8, Key::Num9 => Code::Digit9,
        Key::ArrowLeft => Code::ArrowLeft, Key::ArrowRight => Code::ArrowRight,
        Key::ArrowUp => Code::ArrowUp, Key::ArrowDown => Code::ArrowDown,
        Key::OpenBracket => Code::BracketLeft, Key::CloseBracket => Code::BracketRight,
        Key::Comma => Code::Comma, Key::Period => Code::Period,
        Key::Semicolon => Code::Semicolon, Key::Slash => Code::Slash,
        Key::Backslash => Code::Backslash, Key::Minus => Code::Minus,
        Key::Equals => Code::Equal, Key::Backtick => Code::Backquote,
        Key::PageUp => Code::PageUp, Key::PageDown => Code::PageDown,
        Key::Home => Code::Home, Key::End => Code::End,
        Key::Delete => Code::Delete, Key::Backspace => Code::Backspace,
        Key::Enter => Code::Enter, Key::Tab => Code::Tab,
        Key::F1 => Code::F1, Key::F2 => Code::F2, Key::F3 => Code::F3,
        Key::F4 => Code::F4, Key::F5 => Code::F5, Key::F6 => Code::F6,
        Key::F7 => Code::F7, Key::F8 => Code::F8, Key::F9 => Code::F9,
        Key::F10 => Code::F10, Key::F11 => Code::F11, Key::F12 => Code::F12,
        _ => return None,
    })
}

/// Put each menu item's accelerator in step with what the settings page says.
///
/// Called once the menu exists and again every time a binding changes, so the
/// menu bar shows the shortcut that will actually happen rather than the one
/// it was built with.
///
/// **A bare key never becomes an accelerator.** A menu accelerator belongs to
/// the system, which fires it before any window sees the keystroke; put a
/// plain G on the Guide item and typing "Golf" into the search box changes
/// screen twice. Bare bindings still work — the application reads them
/// itself, and knows when something is being typed into — they simply are
/// not printed beside the menu item, because the menu is not the thing that
/// would deliver them.
pub fn sync_menu_shortcuts(settings: &crate::settings::Settings) {
    use muda::accelerator::Accelerator;

    ITEMS.with_borrow(|items| {
    for (id, item) in items.iter() {
        let accelerator = if let Some((modifiers, code)) = fixed(id) {
            Some(Accelerator::new(Some(modifiers), code))
        } else {
            crate::keys::binding(settings, id)
                .filter(|binding| binding.has_modifier())
                .and_then(|binding| {
                    let mut modifiers = muda::accelerator::Modifiers::empty();
                    if binding.command {
                        modifiers |= muda::accelerator::Modifiers::META;
                    }
                    if binding.shift {
                        modifiers |= muda::accelerator::Modifiers::SHIFT;
                    }
                    if binding.alt {
                        modifiers |= muda::accelerator::Modifiers::ALT;
                    }
                    Some(Accelerator::new(Some(modifiers), code_for(binding.key)?))
                })
        };
        let _ = item.set_accelerator(accelerator);
    }
    });
}

/// What the menu bar will show for an action, for the settings page to print
/// beside the binding. `None` where the menu offers nothing.
pub fn menu_shortcut(id: &str) -> Option<&'static str> {
    match id {
        "settings" => Some("\u{2318},"),
        "fullscreen" => Some("\u{2303}\u{2318}F"),
        _ => None,
    }
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

    // Built with no accelerator; `sync_menu_shortcuts` fills them in from the
    // bindings a moment later, and again whenever those change.
    fn item(id: &str, label: &str, registry: &mut Vec<(String, MenuItem)>) -> MenuItem {
        let item = MenuItem::with_id(id, label, true, None);
        registry.push((id.to_string(), item.clone()));
        item
    }

    let mut registry: Vec<(String, MenuItem)> = Vec::new();

    let about = AboutMetadata {
        name: Some(crate::APP_NAME.into()),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        copyright: Some("Copyright (c) 2026 David Brustein. MIT.".into()),
        ..Default::default()
    };

    let settings = item("settings", "Settings\u{2026}", &mut registry);
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
        item("home", "Home", &mut registry),
        item("guide", "Guide", &mut registry),
        item("library", "Library", &mut registry),
        item("recordings", "Recordings", &mut registry),
        item("downloads", "Downloads", &mut registry),
    ];
    let go = Submenu::new("Go", true);
    for entry in &go_items {
        let _ = go.append(entry);
    }

    let play_items = [
        item("play", "Play or Pause", &mut registry),
        item("back", "Skip Back", &mut registry),
        item("forward", "Skip Forward", &mut registry),
        item("volume_up", "Volume Up", &mut registry),
        item("volume_down", "Volume Down", &mut registry),
        item("mute", "Mute", &mut registry),
        item("channel_up", "Previous Channel", &mut registry),
        item("channel_down", "Next Channel", &mut registry),
        item("stop", "Stop Playback", &mut registry),
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

    let refresh = item("refresh", "Refresh from the DVR", &mut registry);
    let rail = item("rail", "Show or Hide Labels", &mut registry);
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

    // The items are kept, not leaked: their accelerators are set from the
    // bindings now and reset whenever those change.
    ITEMS.with_borrow_mut(|kept| *kept = registry);

    // The menus themselves are leaked deliberately — they own the native menu
    // objects, and dropping them here would tear the menu bar down again.
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
    // On its own thread, and more than once.
    //
    // A single packet at startup is not enough to rely on: it is sent in the
    // same breath as the window being created, and if the interface is not
    // ready yet — a laptop a second after waking, a machine still associating
    // with the network — it goes nowhere and the system never registers that
    // this program wants the local network. No prompt appears, and the first
    // Connect fails with a permission that was never asked for.
    //
    // Three tries a second apart covers a slow interface without delaying
    // anything: nothing waits on this thread, and the packets are one byte to
    // a multicast address nobody is listening on.
    std::thread::spawn(|| {
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") else {
                continue;
            };
            // mDNS, which is unambiguously "local network" to the classifier
            // that decides whether to raise the dialog.
            let asked = socket.send_to(&[0u8], "224.0.0.251:5353");
            // And the all-hosts group, for a network where multicast to the
            // mDNS address is filtered.
            let _ = socket.send_to(&[0u8], "224.0.0.1:5353");
            crate::log::line(&match &asked {
                Ok(_) => format!("[macos] local network requested (try {})", attempt + 1),
                Err(e) => format!(
                    "[macos] local network request failed (try {}): {e}",
                    attempt + 1
                ),
            });
            if asked.is_ok() {
                break;
            }
        }
    });
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
