//! The notification area icon.
//!
//! Closing the window ends the process, and with it every download in flight —
//! which is the wrong trade for a file that is nine tenths transferred. With
//! this enabled the window hides instead and the work carries on, with an icon
//! in the tray as the only remaining evidence that it is still running.
//!
//! That "only remaining evidence" is the whole risk of the feature: an
//! application that ignores its own close button and leaves nothing visible is
//! indistinguishable from one that has crashed and leaked. So the icon is
//! created *before* the window is ever hidden, and if it cannot be created the
//! window is allowed to close normally.

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// What the tray is asking the application to do.
pub enum TrayCommand {
    /// Bring the window back.
    Show,
    /// Really exit, downloads and all.
    Quit,
}

pub struct Tray {
    /// Dropping this removes the icon from the notification area, which is how
    /// turning the setting off takes effect immediately.
    _icon: TrayIcon,
    show: MenuId,
    quit: MenuId,
}

impl Tray {
    /// Returns `None` if the notification area refused the icon, which the
    /// caller must treat as "closing quits", not as "hide anyway".
    pub fn new(rgba: Vec<u8>, width: u32, height: u32, tooltip: &str) -> Option<Self> {
        let icon = Icon::from_rgba(rgba, width, height).ok()?;

        let show = MenuItem::new("Open RustDVR", true, None);
        let quit = MenuItem::new("Quit", true, None);
        let menu = Menu::new();
        menu.append(&show).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&quit).ok()?;

        let (show_id, quit_id) = (show.id().clone(), quit.id().clone());
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(tooltip)
            .with_icon(icon)
            .build()
            .ok()?;

        Some(Self {
            _icon: icon,
            show: show_id,
            quit: quit_id,
        })
    }

    /// Drain whatever the tray has to say, called once a frame.
    ///
    /// Both receivers are global to the process rather than owned by this
    /// icon, so they are drained completely every time: anything left in them
    /// is delivered late, and a queued click that arrives a second after the
    /// icon was clicked reads as the tray being broken.
    pub fn poll(&self) -> Option<TrayCommand> {
        let mut command = None;

        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.quit {
                // Quit wins over anything else queued behind it.
                return Some(TrayCommand::Quit);
            }
            if event.id == self.show {
                command = Some(TrayCommand::Show);
            }
        }

        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            // Left click restores. It is what every Windows tray application
            // does and the first thing anyone tries; the menu is for the rest.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                command = Some(TrayCommand::Show);
            }
        }

        command
    }
}
