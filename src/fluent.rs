// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native Windows client for Channels DVR
// Copyright (c) 2026 David Brustein

//! The window's own chrome: dark title-bar tinting, and rounded corners where
//! the system has them.
//!
//! This used to ask DWM for Mica as well, which is what made the application
//! Windows 11 only — the material does not exist before build 22000, and a
//! window shaped for it on Windows 10 is a transparent hole to the desktop.
//! The material is painted in `backdrop` now, on every version alike. What is
//! left here are the two attributes worth asking the system for, both of which
//! degrade quietly on their own.

#[cfg(windows)]
pub fn apply_chrome(handle: isize, dark: bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_ROUND,
    };

    let hwnd = HWND(handle as *mut _);
    unsafe {
        // Dark mode. Windows 10 1809 was the release that added this, which is
        // where the installer's floor comes from: below it the shadow and the
        // resize border around the window are drawn light, against an
        // application that is entirely dark.
        let dark_flag: i32 = if dark { 1 } else { 0 };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_flag as *const _ as *const _,
            std::mem::size_of::<i32>() as u32,
        );

        // Rounded corners, on the versions that round. Windows 11 rounds app
        // windows and says so explicitly here because this window draws its
        // own frame; Windows 10 has no such attribute and rejects the call,
        // which is why the result is discarded rather than checked. Square
        // corners there are correct — every other window on that desktop has
        // them.
        let corner = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const _ as *const _,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

#[cfg(not(windows))]
pub fn apply_chrome(_handle: isize, _dark: bool) {}
