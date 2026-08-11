// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native Windows client for Channels DVR
// Copyright (c) 2026 David Brustein

//! What every key does, and what it can be changed to.
//!
//! One table, read by three things that would otherwise disagree: the handler
//! that acts on a key, the settings page that lists and rebinds them, and the
//! defaults that a reset returns to. Adding an action means adding a line here
//! and handling it in `handle_keys`; nothing else has to be told.
//!
//! Bindings are stored as key *names* rather than as numbers, so a settings
//! file stays readable and survives egui renumbering its key enum.

use eframe::egui;

use crate::settings::Settings;

/// A thing a key can do.
///
/// `id` is what the settings file records and must never change. `label` is
/// what the settings page shows. `default` is the key it starts on, by egui's
/// own name for it.
pub struct Action {
    pub id: &'static str,
    pub label: &'static str,
    pub default: &'static str,
}

const fn action(id: &'static str, label: &'static str, default: &'static str) -> Action {
    Action { id, label, default }
}

pub const ACTIONS: &[Action] = &[
    // Two sets of defaults, because two desktops disagree about what a
    // shortcut looks like.
    //
    // On Windows and Linux a bare letter is normal in a media application and
    // there is no menu bar to contradict it. On a Mac there is, and it puts
    // views on Command-1 upward — so the default *is* that combination, and
    // the menu bar shows the same shortcut this page does. One action, one
    // shortcut, printed identically in both places; anything else means the
    // program disagreeing with its own menu about how to reach the guide.
    #[cfg(not(target_os = "macos"))]
    action("home", "Home", "H"),
    #[cfg(not(target_os = "macos"))]
    action("guide", "Guide", "G"),
    #[cfg(not(target_os = "macos"))]
    action("library", "Library", "L"),
    #[cfg(not(target_os = "macos"))]
    action("recordings", "Recordings", "R"),
    #[cfg(not(target_os = "macos"))]
    action("downloads", "Downloads", "D"),
    #[cfg(not(target_os = "macos"))]
    action("settings", "Settings", "S"),
    #[cfg(not(target_os = "macos"))]
    action("rail", "Show or hide the rail's labels", "Tab"),
    // Command and one key, throughout: the screens on their numbers, the rail
    // on the number below them, playback on the arrows. Nothing needs two
    // modifiers except Mute, and only because Command-M is the system's
    // Minimize and cannot be had.
    #[cfg(target_os = "macos")]
    action("home", "Home", "Cmd+1"),
    #[cfg(target_os = "macos")]
    action("guide", "Guide", "Cmd+2"),
    #[cfg(target_os = "macos")]
    action("library", "Library", "Cmd+3"),
    #[cfg(target_os = "macos")]
    action("recordings", "Recordings", "Cmd+4"),
    #[cfg(target_os = "macos")]
    action("downloads", "Downloads", "Cmd+5"),
    #[cfg(target_os = "macos")]
    action("settings", "Settings", "Cmd+Comma"),
    #[cfg(target_os = "macos")]
    action("rail", "Show or hide the rail's labels", "Cmd+0"),

    // Space, everywhere. The one shortcut with no menu accelerator: a menu
    // that owned Space would swallow it inside every text field, so the
    // application reads this key itself and stands aside while typing.
    action("play", "Play or pause", "Space"),

    #[cfg(not(target_os = "macos"))]
    action("back", "Skip back", "ArrowLeft"),
    #[cfg(not(target_os = "macos"))]
    action("forward", "Skip forward", "ArrowRight"),
    #[cfg(not(target_os = "macos"))]
    action("volume_up", "Volume up", "ArrowUp"),
    #[cfg(not(target_os = "macos"))]
    action("volume_down", "Volume down", "ArrowDown"),
    #[cfg(not(target_os = "macos"))]
    action("mute", "Mute", "M"),
    #[cfg(target_os = "macos")]
    action("back", "Skip back", "Cmd+ArrowLeft"),
    #[cfg(target_os = "macos")]
    action("forward", "Skip forward", "Cmd+ArrowRight"),
    #[cfg(target_os = "macos")]
    action("volume_up", "Volume up", "Cmd+ArrowUp"),
    #[cfg(target_os = "macos")]
    action("volume_down", "Volume down", "Cmd+ArrowDown"),
    #[cfg(target_os = "macos")]
    action("mute", "Mute", "Shift+Cmd+M"),

    // F11 on Windows and Linux; Command-F on a Mac. Apple's own full-screen
    // item is Control-Command-F, and that would be the conventional choice —
    // but this program has no Find for Command-F to clash with, one modifier
    // is easier to press than two, and the menu prints whichever it is.
    #[cfg(not(target_os = "macos"))]
    action("fullscreen", "Full screen", "F11"),
    #[cfg(target_os = "macos")]
    action("fullscreen", "Full screen", "Cmd+F"),

    #[cfg(not(target_os = "macos"))]
    action("stop", "Stop playback", "Backspace"),
    #[cfg(target_os = "macos")]
    action("stop", "Stop playback", "Cmd+Period"),

    // Page Up and Page Down on a desktop keyboard; brackets on a Mac, where
    // a laptop has neither key without holding fn.
    #[cfg(not(target_os = "macos"))]
    action("channel_up", "Previous channel", "PageUp"),
    #[cfg(not(target_os = "macos"))]
    action("channel_down", "Next channel", "PageDown"),
    #[cfg(target_os = "macos")]
    action("channel_up", "Previous channel", "Cmd+OpenBracket"),
    #[cfg(target_os = "macos")]
    action("channel_down", "Next channel", "Cmd+CloseBracket"),

    // Last, because it is the one that governs the others, and because a
    // reader arriving at it having read the rest understands immediately why
    // it has to keep working when they are all off.
    //
    // Not a function key on a Mac: F7 to F9 are the media keys there, so F8
    // is Play/Pause to the system before this program ever hears about it.
    #[cfg(not(target_os = "macos"))]
    action("toggle", "Turn shortcuts off or on", "F8"),
    // Command-backslash rather than a bare one, by the same rule every other
    // Mac binding follows — see `refusal`.
    #[cfg(target_os = "macos")]
    action("toggle", "Turn shortcuts off or on", "Cmd+Backslash"),
];


/// The action that enables and disables the rest.
///
/// Kept working even when shortcuts are disabled, for the obvious reason: a
/// switch that cannot be reached once it has been used is a trap, and the
/// alternative is telling someone to edit a JSON file to get their keyboard
/// back.
pub const TOGGLE: &str = "toggle";

/// A key, and the modifiers that must be held with it.
///
/// Modifiers are stored and compared *exactly*: a binding of Command-G does
/// not fire on Command-Shift-G, and a bare G does not fire when Command is
/// down. Anything looser produces shortcuts that go off while somebody is
/// using a different one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub key: egui::Key,
    /// Command on a Mac, Control everywhere else. One flag rather than two,
    /// because it is one idea — "the modifier this desktop uses for
    /// shortcuts" — and egui already reports it that way.
    pub command: bool,
    /// The literal Control key. Separate from `command` because on a Mac they
    /// are two different keys and Control-Command-F is a real shortcut; on
    /// Windows and Linux "Ctrl" *is* the command modifier, so a binding
    /// written with it there parses into `command` instead.
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Binding {
    pub fn has_modifier(self) -> bool {
        self.command || self.ctrl || self.shift || self.alt
    }

    /// How it is written into the settings file: `Cmd+Shift+G`, or `G`.
    ///
    /// Deliberately words rather than symbols. A settings file is read in a
    /// text editor, sometimes on the other platform, and ⌘ there is a box.
    pub fn to_setting(self) -> String {
        let mut out = String::new();
        if self.command {
            out.push_str("Cmd+");
        }
        if self.ctrl {
            out.push_str("Ctrl+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        out.push_str(self.key.name());
        out
    }

    /// The reverse, tolerant of what a person might type by hand and of the
    /// bare key names every settings file written before modifiers existed
    /// contains.
    pub fn from_setting(text: &str) -> Option<Self> {
        let mut binding =
            Self { key: egui::Key::Space, command: false, ctrl: false, shift: false, alt: false };
        let mut named = None;
        for part in text.split('+') {
            match part.trim().to_ascii_lowercase().as_str() {
                "" => continue,
                "cmd" | "command" | "super" | "win" => binding.command = true,
                // Control is its own key on a Mac and the command modifier
                // everywhere else, which is exactly how the two desktops
                // think of it.
                "ctrl" | "control" => {
                    if cfg!(target_os = "macos") {
                        binding.ctrl = true;
                    } else {
                        binding.command = true;
                    }
                }
                "shift" => binding.shift = true,
                "alt" | "option" | "opt" => binding.alt = true,
                _ => named = Some(part.trim().to_string()),
            }
        }
        binding.key = egui::Key::from_name(&named?)?;
        Some(binding)
    }

    /// How it reads on screen: `⌘⇧G` on a Mac, `Ctrl+Shift+G` elsewhere,
    /// because those are the two things people's eyes are trained on.
    pub fn display(self) -> String {
        // Apple's own order for the glyphs is ⌃⌥⇧⌘, and a Mac user reads a
        // shortcut wrong if they are in any other one.
        let key = key_label(self.key);
        if cfg!(target_os = "macos") {
            let mut out = String::new();
            if self.ctrl {
                out.push('\u{2303}');
            }
            if self.alt {
                out.push('\u{2325}');
            }
            if self.shift {
                out.push('\u{21e7}');
            }
            if self.command {
                out.push('\u{2318}');
            }
            out.push_str(&key);
            out
        } else {
            let mut out = String::new();
            if self.command {
                out.push_str("Ctrl+");
            }
            if self.alt {
                out.push_str("Alt+");
            }
            if self.shift {
                out.push_str("Shift+");
            }
            out.push_str(&key);
            out
        }
    }
}

/// A key as it should be printed: the arrows as arrows, everything else as
/// egui prints it. `ArrowLeft` beside a ⌘ reads as a word rather than a key.
fn key_label(key: egui::Key) -> String {
    match key {
        egui::Key::ArrowLeft => "\u{2190}".to_string(),
        egui::Key::ArrowRight => "\u{2192}".to_string(),
        egui::Key::ArrowUp => "\u{2191}".to_string(),
        egui::Key::ArrowDown => "\u{2193}".to_string(),
        other => other.symbol_or_name().to_string(),
    }
}

/// Why a binding is refused, or `None` if it is allowed.
///
/// The list is short and every entry is something that would take a working
/// keyboard away from somebody:
///
///   * **The desktop's own shortcuts.** Command-Q quits, Command-W closes,
///     Command-Tab switches applications. Binding one of those here does not
///     win — the system gets there first — so the action would simply never
///     fire, and the settings page would be listing a lie.
///   * **The clipboard.** Command-C, V, X, A and Z belong to the Edit menu
///     and to every text field in the program. Taking one would mean not
///     being able to paste a server address into the box that asks for one.
///   * **Command-comma**, which is Settings on every Mac ever made and is
///     already in the menu.
///
/// Not refused, deliberately: two actions on one key. That is shown rather
/// than blocked, because swapping two keys over passes through a clash on
/// the way and refusing the first half makes the swap impossible.
pub fn refusal(binding: Binding) -> Option<&'static str> {
    use egui::Key;

    // On a Mac, a shortcut without a modifier is refused outright.
    //
    // Not for tidiness. Every binding here is also a menu item, and a menu
    // accelerator belongs to the system, which fires it before any window
    // sees the keystroke — so a bare G on the Guide item would change screen
    // while somebody typed "Golf" into the search box. The alternative was to
    // accept bare keys and leave those menu items blank, which is the exact
    // disagreement between the menu and this page that the whole arrangement
    // exists to prevent.
    //
    // Space is the one exception, because play and pause is Space in every
    // video player ever made and nobody reaches for it holding Command. Its
    // menu item carries no key and says so by showing none.
    if cfg!(target_os = "macos") && !binding.has_modifier() && binding.key != Key::Space {
        return Some("a Mac shortcut needs Command, Control, Shift or Option");
    }

    // Modifier-free keys elsewhere cannot collide with the desktop, so
    // nothing below applies to them.
    if !binding.command {
        return None;
    }
    let plain_command = binding.command && !binding.shift && !binding.alt && !binding.ctrl;
    if !plain_command {
        return None;
    }
    match binding.key {
        Key::Q => Some("the desktop uses this to quit"),
        Key::W => Some("the desktop uses this to close a window"),
        Key::H => Some("the desktop uses this to hide the application"),
        Key::M => Some("the desktop uses this to minimize the window"),
        Key::Tab => Some("the desktop uses this to switch applications"),
        Key::Space => Some("the desktop uses this for search"),
        Key::C | Key::V | Key::X | Key::A | Key::Z => {
            Some("copy, paste and friends need this")
        }
        _ => None,
    }
}

/// The binding currently on an action, or none if it has been cleared.
pub fn binding(settings: &Settings, id: &str) -> Option<Binding> {
    let text = match settings.shortcut_keys.get(id) {
        // An empty string is a deliberate unbinding, not a missing entry, and
        // means the action has no key at all.
        Some(name) if name.trim().is_empty() => return None,
        Some(name) => name.clone(),
        None => default_for(id)?.to_string(),
    };
    let stored = Binding::from_setting(&text);

    // A settings file written before Mac bindings had to carry a modifier
    // can hold a bare key, and honouring it would put back the thing the
    // rule exists to prevent: a shortcut this page shows and the menu bar
    // cannot. Such a binding falls back to the default, which is one the
    // menu can carry. Nothing is rewritten on disk — the next rebinding
    // does that, and until then an old file still opens on the old build.
    if cfg!(target_os = "macos") {
        if let Some(binding) = stored {
            if refusal(binding).is_some() {
                return default_for(id).and_then(Binding::from_setting);
            }
        }
    }
    stored
}

pub fn default_for(id: &str) -> Option<&'static str> {
    ACTIONS.iter().find(|a| a.id == id).map(|a| a.default)
}

/// How a binding reads on the settings page.
pub fn display(settings: &Settings, id: &str) -> String {
    match binding(settings, id) {
        Some(binding) => binding.display(),
        None => "—".to_string(),
    }
}

/// Whether the binding on `id` was pressed this frame.
///
/// Respects the master switch, except for the switch itself. Everything else
/// about when a key counts — a text field having the keyboard, or something
/// playing — belongs to the caller, because it differs per action.
pub fn pressed(ctx: &egui::Context, settings: &Settings, id: &str) -> bool {
    if !settings.shortcuts_enabled && id != TOGGLE {
        return false;
    }
    let Some(binding) = binding(settings, id) else { return false };
    ctx.input(|i| {
        i.key_pressed(binding.key)
            && i.modifiers.command == binding.command
            && i.modifiers.shift == binding.shift
            && i.modifiers.alt == binding.alt
            // On a Mac `ctrl` is a key of its own; elsewhere egui reports it
            // as the command modifier too, and comparing it again would make
            // every Control binding impossible to press.
            && (!cfg!(target_os = "macos") || i.modifiers.ctrl == binding.ctrl)
    })
}

/// Which other actions are on the same binding, if any.
///
/// Two actions on one key is not refused: it is shown. Refusing means deciding
/// for someone that their remap is wrong, when they may be part way through
/// swapping two keys over, and a clash that is visible is a clash that gets
/// fixed.
pub fn conflicts(settings: &Settings, id: &str) -> Vec<&'static str> {
    let Some(binding) = binding(settings, id) else { return Vec::new() };
    ACTIONS
        .iter()
        .filter(|a| a.id != id && crate::keys::binding(settings, a.id) == Some(binding))
        .map(|a| a.label)
        .collect()
}
