// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native Windows client for Channels DVR
// Copyright (c) 2026 David Brustein

//! The notification area icon.
//!
//! Closing the window ends the process, and with it every download in flight —
//! which is the wrong trade for a file that is nine tenths transferred. With
//! this enabled the window hides instead and the work carries on, with an icon
//! in the tray as the only remaining evidence that it is still running.
//!
//! **The tray is watched on its own thread, and acts through Win32 rather than
//! through egui.** That is not a preference, it is the only thing that works.
//! The first version polled the tray from inside `App::update`, which is only
//! called when a frame is drawn — and a hidden window is never asked to draw
//! one, whatever `request_repaint` says. So the moment the window went away the
//! polling stopped with it, and neither Open nor Quit was ever seen: the menu
//! appeared, because Windows draws it, and clicking it did nothing at all.
//!
//! A thread that owns nothing but the menu ids and the window handle has no
//! such dependency. Restoring calls `ShowWindow` directly, and quitting exits
//! the process, because by then there is no interface left to ask politely.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub struct Tray {
    /// Dropping this removes the icon from the notification area, which is how
    /// turning the setting off takes effect immediately.
    _icon: TrayIcon,
    /// Cleared when this is dropped, so the watcher thread stops with it.
    stop: Arc<AtomicBool>,
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Tray {
    /// Returns `None` if the notification area refused the icon, which the
    /// caller must treat as "closing quits", not as "hide anyway".
    ///
    /// `hwnd` is the application window, used to bring it back. Without a real
    /// handle there is no way to restore a hidden window from another thread,
    /// so a missing one is also a refusal.
    pub fn new(
        rgba: Vec<u8>,
        width: u32,
        height: u32,
        tooltip: &str,
        hwnd: Option<isize>,
    ) -> Option<Self> {
        let hwnd = hwnd?;
        let icon = Icon::from_rgba(rgba, width, height).ok()?;

        let open_item = MenuItem::new(format!("Open {}", crate::APP_NAME), true, None);
        let quit_item = MenuItem::new(format!("Quit {}", crate::APP_NAME), true, None);
        let menu = Menu::new();
        menu.append(&open_item).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&quit_item).ok()?;

        let (open_id, quit_id) = (open_item.id().clone(), quit_item.id().clone());
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(tooltip)
            .with_icon(icon)
            .build()
            .ok()?;

        let stop = Arc::new(AtomicBool::new(false));
        let watcher = Arc::clone(&stop);
        std::thread::spawn(move || {
            // Polled rather than blocked on, because there are two independent
            // receivers and selecting across both would mean taking a direct
            // dependency on the channel crate underneath them. A tenth of a
            // second is far below the threshold at which a menu click feels
            // like it was ignored.
            while !watcher.load(Ordering::SeqCst) {
                while let Ok(event) = MenuEvent::receiver().try_recv() {
                    if event.id == quit_id {
                        quit();
                    }
                    if event.id == open_id {
                        restore(hwnd);
                    }
                }
                while let Ok(event) = TrayIconEvent::receiver().try_recv() {
                    // Left click restores. It is what every Windows tray
                    // application does and the first thing anyone tries; the
                    // menu is for everything else.
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        restore(hwnd);
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });

        Some(Self { _icon: icon, stop })
    }
}

/// Bring the window back and put it in front.
fn restore(hwnd: isize) {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
        };

        let hwnd = HWND(hwnd as *mut _);
        // Both, in this order: SW_SHOW undoes the hide, SW_RESTORE undoes a
        // minimise. Whichever way it left the screen, it comes back.
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
    }
}

/// Exit, from a thread that is not the one running the event loop.
///
/// Blunt on purpose. Asking egui to close would mean the request being noticed
/// by a frame, and a hidden window does not draw frames — which is the exact
/// reason Quit did nothing before. Downloads in progress are abandoned, which
/// is what Quit means; their `.part` files are swept at the next start.
fn quit() -> ! {
    std::process::exit(0)
}
