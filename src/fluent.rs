//! Windows Fluent integration: Mica backdrop and rounded corners on
//! Windows 11, dark chrome alone for the win10 build.
//!
//! Mica is a system material, not something an app can paint. The window has to
//! be transparent where the material should show and DWM has to be told to draw
//! it, after which the desktop composition tints the window with the wallpaper
//! behind it. Nothing in egui can fake that convincingly, so it is asked for
//! properly.

#[cfg(windows)]
pub fn apply_mica(handle: isize, dark: bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};

    let hwnd = HWND(handle as *mut _);
    unsafe {
        // Dark mode first: it decides how the title-bar area and the material
        // are tinted, and setting it after the backdrop leaves a light flash.
        // This attribute exists on Windows 10 too, which is why it is not
        // gated with the rest.
        let dark_flag: i32 = if dark { 1 } else { 0 };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_flag as *const _ as *const _,
            std::mem::size_of::<i32>() as u32,
        );
    }

    // The material and the rounding are Windows 11 attributes. The win10
    // build does not ask for them — the calls would fail harmlessly, but a
    // build that exists *for* Windows 10 should not lean on failure to be
    // correct there.
    #[cfg(not(feature = "win10"))]
    unsafe {
        use windows::Win32::Graphics::Dwm::{
            DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
            DWM_SYSTEMBACKDROP_TYPE,
        };

        // DWMSBT_MAINWINDOW is the Mica used by File Explorer and Settings.
        const DWMSBT_MAINWINDOW: DWM_SYSTEMBACKDROP_TYPE = DWM_SYSTEMBACKDROP_TYPE(2);

        let backdrop = DWMSBT_MAINWINDOW;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            &backdrop as *const _ as *const _,
            std::mem::size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
        );

        // Windows 11 rounds app windows; say so explicitly so it holds even
        // with a custom (undecorated) frame.
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
pub fn apply_mica(_handle: isize, _dark: bool) {}
