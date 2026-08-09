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
    action("fullscreen", "Full screen", "F11"),
    action("stop", "Stop playback", "Backspace"),
    action("channel_up", "Previous channel", "PageUp"),
    action("channel_down", "Next channel", "PageDown"),
    // Last, because it is the one that governs the others, and because a
    // reader arriving at it having read the rest understands immediately why
    // it has to keep working when they are all off.
    action("toggle", "Turn shortcuts off or on", "F8"),
];

/// The action that enables and disables the rest.
///
/// Kept working even when shortcuts are disabled, for the obvious reason: a
/// switch that cannot be reached once it has been used is a trap, and the
/// alternative is telling someone to edit a JSON file to get their keyboard
/// back.
pub const TOGGLE: &str = "toggle";

/// The key currently bound to an action, or none if it has been cleared.
pub fn binding(settings: &Settings, id: &str) -> Option<egui::Key> {
    let name = match settings.shortcut_keys.get(id) {
        // An empty string is a deliberate unbinding, not a missing entry, and
        // means the action has no key at all.
        Some(name) if name.trim().is_empty() => return None,
        Some(name) => name.clone(),
        None => default_for(id)?.to_string(),
    };
    egui::Key::from_name(&name)
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
        Some(key) => key.name().to_string(),
        None => "—".to_string(),
    }
}

/// Whether the key bound to `id` was pressed this frame.
///
/// Respects the master switch, except for the switch itself. Everything else
/// about when a key counts — a text field having the keyboard, or something
/// playing — belongs to the caller, because it differs per action.
pub fn pressed(ctx: &egui::Context, settings: &Settings, id: &str) -> bool {
    if !settings.shortcuts_enabled && id != TOGGLE {
        return false;
    }
    let Some(key) = binding(settings, id) else { return false };
    ctx.input(|i| i.key_pressed(key))
}

/// Which other actions are on the same key, if any.
///
/// Two actions on one key is not refused: it is shown. Refusing means deciding
/// for someone that their remap is wrong, when they may be part way through
/// swapping two keys over, and a clash that is visible is a clash that gets
/// fixed.
pub fn conflicts(settings: &Settings, id: &str) -> Vec<&'static str> {
    let Some(key) = binding(settings, id) else { return Vec::new() };
    ACTIONS
        .iter()
        .filter(|a| a.id != id && binding(settings, a.id) == Some(key))
        .map(|a| a.label)
        .collect()
}
