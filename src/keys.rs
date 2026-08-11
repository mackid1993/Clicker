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
    action("home", "Home", "H"),
    action("guide", "Guide", "G"),
    action("library", "Library", "L"),
    action("recordings", "Recordings", "R"),
    action("downloads", "Downloads", "D"),
    action("settings", "Settings", "S"),
    action("rail", "Show or hide the rail's labels", "Tab"),
    action("play", "Play or pause", "Space"),
    action("back", "Skip back", "ArrowLeft"),
    action("forward", "Skip forward", "ArrowRight"),
    action("volume_up", "Volume up", "ArrowUp"),
    action("volume_down", "Volume down", "ArrowDown"),
    action("mute", "Mute", "M"),
    // F11 on Windows and Linux, F on a Mac. The function keys there belong to
    // the system — F11 shows the desktop — and every Mac video player uses F
    // for this anyway. The menu bar's Control-Command-F stands beside it.
    #[cfg(not(target_os = "macos"))]
    action("fullscreen", "Full screen", "F11"),
    #[cfg(target_os = "macos")]
    action("fullscreen", "Full screen", "F"),
    action("stop", "Stop playback", "Backspace"),
    // Page Up and Page Down on a desktop keyboard; brackets on a Mac, where
    // a laptop has no such keys and reaching them means holding fn and an
    // arrow. The brackets are also what the menu bar puts these on, so the
    // two agree.
    #[cfg(not(target_os = "macos"))]
    action("channel_up", "Previous channel", "PageUp"),
    #[cfg(not(target_os = "macos"))]
    action("channel_down", "Next channel", "PageDown"),
    #[cfg(target_os = "macos")]
    action("channel_up", "Previous channel", "["),
    #[cfg(target_os = "macos")]
    action("channel_down", "Next channel", "]"),
    // Last, because it is the one that governs the others, and because a
    // reader arriving at it having read the rest understands immediately why
    // it has to keep working when they are all off.
    //
    // Not a function key on a Mac: F7 to F9 are the media keys there, so F8
    // is Play/Pause to the system before this program ever hears about it.
    #[cfg(not(target_os = "macos"))]
    action("toggle", "Turn shortcuts off or on", "F8"),
    #[cfg(target_os = "macos")]
    action("toggle", "Turn shortcuts off or on", "\\"),
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
    pub shift: bool,
    pub alt: bool,
}

impl Binding {
    pub fn bare(key: egui::Key) -> Self {
        Self { key, command: false, shift: false, alt: false }
    }

    pub fn has_modifier(self) -> bool {
        self.command || self.shift || self.alt
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
        let mut binding = Self { key: egui::Key::Space, command: false, shift: false, alt: false };
        let mut named = None;
        for part in text.split('+') {
            match part.trim().to_ascii_lowercase().as_str() {
                "" => continue,
                "cmd" | "command" | "ctrl" | "control" | "super" | "win" => binding.command = true,
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
        let key = self.key.name();
        if cfg!(target_os = "macos") {
            let mut out = String::new();
            if self.command {
                out.push('\u{2318}');
            }
            if self.alt {
                out.push('\u{2325}');
            }
            if self.shift {
                out.push('\u{21e7}');
            }
            out.push_str(key);
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
            out.push_str(key);
            out
        }
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

    // Modifier-free keys cannot collide with the desktop, so nothing here
    // applies to them.
    if !binding.command {
        return None;
    }
    let plain_command = binding.command && !binding.shift && !binding.alt;
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
        Key::Comma => Some("this is Settings, and already in the menu"),
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
    Binding::from_setting(&text)
}

pub fn default_for(id: &str) -> Option<&'static str> {
    ACTIONS.iter().find(|a| a.id == id).map(|a| a.default)
}

pub fn label_for(id: &str) -> &str {
    ACTIONS
        .iter()
        .find(|a| a.id == id)
        .map(|a| a.label)
        .unwrap_or(id)
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
