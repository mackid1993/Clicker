// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native client for Channels DVR
// Copyright (c) 2026 David Brustein

//! Linux. Written to the XDG conventions and the two GL worlds — EGL under
//! Wayland, GLX under X11 — without trying to guess which one the session is;
//! the loader asks both.

use std::ffi::{c_char, c_int, c_void};
use std::path::PathBuf;

// --- window chrome -----------------------------------------------------------

/// The compositor's frame, not ours.
///
/// This was the other way round to begin with, on the reasoning that the free
/// desktops draw their own headers anyway. They do — but *they* do it, from
/// inside the toolkit, in cooperation with the compositor. An undecorated
/// window that provides its own buttons and its own resize edges depends on
/// asking the compositor to take over a drag, and what that costs is not
/// uniform: X11 and Wayland differ, GNOME and KDE differ, and a window whose
/// buttons do nothing or which cannot be resized is not a stylistic
/// disappointment, it is a broken window.
///
/// So the system decorates it. The title bar sits above a surface that is
/// otherwise entirely ours, which is what every other application on that
/// desktop looks like, and the buttons are the ones the user's own theme
/// draws — working, in the place they expect, on every compositor.
pub const NATIVE_FRAME: bool = true;

/// The frame is above the surface rather than over it, so nothing has to be
/// kept clear the way the traffic lights do on a Mac.
pub const CAPTION_INSET: f32 = 0.0;

/// A decorated window, with the interior ours.
pub fn shape_window(viewport: eframe::egui::ViewportBuilder) -> eframe::egui::ViewportBuilder {
    viewport.with_decorations(true)
}

/// Nothing here refuses the local network, so no failure is ever that.
pub fn permission_denied(_message: &str) -> bool {
    false
}

/// Never called: nothing to open, no permission to grant.
pub fn open_local_network_settings() {}

/// Nothing stands between this program and the local network here.
pub const LOCAL_NETWORK_HINT: &str = "";

/// Nothing to request: there is no local network permission here.
pub fn request_local_network() {}

/// No menu bar: this platform puts an application's commands inside its
/// window, which is where this one draws them.
pub fn install_menu_bar() {}

/// Hand a link to the desktop's browser.
pub fn open_url(url: &str) {
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

/// Nothing to keep in step: there is no menu bar here, so a rebinding
/// changes the settings page and nothing else.
pub fn sync_menu_shortcuts(_settings: &crate::settings::Settings) {}

/// Never anything, there being no menu to ask from.
pub fn menu_command() -> Option<String> {
    None
}

// --- where files go ----------------------------------------------------------

fn home() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?))
}

fn xdg(variable: &str, fallback: &[&str]) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(variable) {
        let dir = PathBuf::from(dir);
        if dir.is_absolute() {
            return Some(dir);
        }
    }
    let mut dir = home()?;
    for part in fallback {
        dir = dir.join(part);
    }
    Some(dir)
}

/// Settings, under `XDG_CONFIG_HOME`.
pub fn config_home() -> Option<PathBuf> {
    xdg("XDG_CONFIG_HOME", &[".config"])
}

/// Downloads, caches, buffers and logs, under `XDG_DATA_HOME` rather than
/// `XDG_CACHE_HOME`: offline downloads are whole recordings someone means to
/// keep, and the cache directory is fair game for every cleanup tool.
pub fn data_home() -> Option<PathBuf> {
    xdg("XDG_DATA_HOME", &[".local", "share"])
}

// --- fonts -------------------------------------------------------------------

/// egui's bundled face, by returning nothing. A Linux desktop has no one
/// system font to read the way Windows has Segoe and macOS has San
/// Francisco, and guessing at distribution font paths buys inconsistency.
pub fn text_font() -> Option<Vec<u8>> {
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

/// Nothing to do: every Linux filesystem this will meet — ext4, btrfs, xfs —
/// keeps files sparse by construction.
pub fn make_sparse(_file: &tokio::fs::File) {}

extern "C" {
    fn fallocate(fd: c_int, mode: c_int, offset: i64, len: i64) -> c_int;
    fn getrlimit(resource: c_int, rlim: *mut Rlimit) -> c_int;
    fn setrlimit(resource: c_int, rlim: *const Rlimit) -> c_int;
}

#[repr(C)]
struct Rlimit {
    cur: u64,
    max: u64,
}

const RLIMIT_NOFILE: c_int = 7;

/// Raise the file-descriptor ceiling to the hard limit.
///
/// Measured with 1024 of 1024 descriptors open one minute into playback, 958
/// of them `anon_inode:sync_file` — GPU fence descriptors the virtio graphics
/// driver exports every frame and never closes. The leak is the driver's, but
/// the death is ours: at the ceiling, sockets, images and the audio device
/// all fail at once and the application appears to have a stroke. Chromium
/// raises this limit at startup for the same class of reason; the hard limit
/// on a systemd desktop is around a million, which turns a one-minute cliff
/// into a horizon nobody meets.
pub fn raise_fd_limit() {
    unsafe {
        let mut limit = Rlimit { cur: 0, max: 0 };
        if getrlimit(RLIMIT_NOFILE, &mut limit) != 0 || limit.cur >= limit.max {
            return;
        }
        let was = limit.cur;
        limit.cur = limit.max;
        if setrlimit(RLIMIT_NOFILE, &limit) == 0 {
            crate::log::line(&format!(
                "[clicker] file descriptor limit raised {was} -> {}",
                limit.max
            ));
        }
    }
}

const FALLOC_FL_KEEP_SIZE: c_int = 0x01;
const FALLOC_FL_PUNCH_HOLE: c_int = 0x02;

/// Give a range at the front of the buffer back to the disk.
///
/// `fallocate` takes byte offsets and rounds to blocks itself — partial
/// blocks are zeroed, whole blocks released — so unlike the macOS call there
/// is no alignment to do here. KEEP_SIZE because the demuxer holds byte
/// offsets into this file and the file must not shrink under it.
pub fn punch_hole(file: &tokio::fs::File, from: u64, len: u64) -> bool {
    use std::os::unix::io::AsRawFd;

    if len == 0 {
        return false;
    }
    unsafe {
        fallocate(
            file.as_raw_fd(),
            FALLOC_FL_PUNCH_HOLE | FALLOC_FL_KEEP_SIZE,
            from as i64,
            len as i64,
        ) == 0
    }
}

// --- libmpv and OpenGL -------------------------------------------------------

/// What the mpv library is called here, for messages about not finding it.
pub const MPV_LIBRARY: &str = "libmpv.so.2";

/// Where libmpv might be, and deliberately nowhere else.
///
/// The AppImage's own lib directory first, then beside a bare binary, then
/// the repository's staged build for anyone running this out of `cargo`.
/// What is *not* here is the system loader's search path: a distribution's
/// FFmpeg is frequently built with GPL components, and this application ships
/// under MIT with an LGPL player. The only libmpv it loads is one built by
/// `scripts/build-mpv.sh` with `-Dgpl=false` and `--disable-gpl`.
pub fn mpv_candidates() -> Vec<String> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from));
    let mut candidates = Vec::new();
    if let Some(dir) = &exe_dir {
        // usr/bin/../lib, which is where the AppImage puts them.
        candidates.push(dir.join("../lib").join(MPV_LIBRARY).display().to_string());
        candidates.push(dir.join(MPV_LIBRARY).display().to_string());
        candidates.push(
            dir.join("../../third_party/mpv")
                .join(MPV_LIBRARY)
                .display()
                .to_string(),
        );
    }
    candidates
}

/// A GL loader function, resolved out of a library once.
fn loader(library: &str, symbol: &[u8]) -> Option<unsafe extern "C" fn(*const c_char) -> *mut c_void> {
    let module = super::open_library(library);
    if module.is_null() {
        return None;
    }
    let found = unsafe { super::library_symbol(module, symbol.as_ptr()) };
    if found.is_null() {
        return None;
    }
    Some(unsafe { std::mem::transmute(found) })
}

/// How mpv finds the OpenGL functions of the context eframe created.
///
/// The session decides whether that context is EGL or GLX, and this does not
/// know which, so it asks in order: `eglGetProcAddress`, `glXGetProcAddressARB`,
/// then a plain `dlsym` into libGL for the 1.1 entry points the older
/// loaders decline to return.
pub unsafe extern "C" fn gl_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    type Loader = unsafe extern "C" fn(*const c_char) -> *mut c_void;
    static LOADERS: std::sync::OnceLock<(Option<Loader>, Option<Loader>, usize)> =
        std::sync::OnceLock::new();
    let (egl, glx, libgl) = *LOADERS.get_or_init(|| {
        (
            loader("libEGL.so.1", b"eglGetProcAddress\0"),
            loader("libGL.so.1", b"glXGetProcAddressARB\0"),
            super::open_library("libGL.so.1") as usize,
        )
    });

    if let Some(egl) = egl {
        let found = egl(name);
        if !found.is_null() {
            return found;
        }
    }
    if let Some(glx) = glx {
        let found = glx(name);
        if !found.is_null() {
            return found;
        }
    }
    let libgl = libgl as *mut c_void;
    if libgl.is_null() {
        return std::ptr::null_mut();
    }
    super::library_symbol(libgl, name as *const u8)
}

/// Which hypervisor this is running under, if any.
///
/// Read from DMI, which every hypervisor stamps with its name, with the CPU's
/// hypervisor flag as a fallback for the ones that do not. Asked once.
pub fn virtualization() -> Option<&'static str> {
    static WHO: std::sync::OnceLock<Option<&'static str>> = std::sync::OnceLock::new();
    *WHO.get_or_init(|| {
        let dmi = |f: &str| std::fs::read_to_string(format!("/sys/devices/virtual/dmi/id/{f}"))
            .unwrap_or_default()
            .to_lowercase();
        // Vendor and product, plus the board and BIOS: Proxmox and plain QEMU
        // stamp different fields depending on age and configuration, and a
        // guest that says "Bochs" anywhere is as virtual as one that says
        // "QEMU". The label only chooses a word for the log; every hit gets
        // the same allowances.
        let id = format!(
            "{} {} {} {}",
            dmi("sys_vendor"),
            dmi("product_name"),
            dmi("board_vendor"),
            dmi("bios_vendor")
        );
        for (needle, name) in [
            ("parallels", "Parallels"),
            ("vmware", "VMware"),
            ("proxmox", "Proxmox"),
            ("qemu", "QEMU/KVM"),
            ("kvm", "QEMU/KVM"),
            ("bochs", "QEMU/KVM"),
            ("virtualbox", "VirtualBox"),
            ("innotek", "VirtualBox"),
            ("xen", "Xen"),
            ("microsoft", "Hyper-V"),
            ("amazon", "Amazon EC2"),
            ("google", "Google Compute Engine"),
        ] {
            if id.contains(needle) {
                return Some(name);
            }
        }
        // The flag every hypervisor sets even when DMI says nothing useful.
        if std::fs::read_to_string("/proc/cpuinfo")
            .unwrap_or_default()
            .contains("hypervisor")
        {
            return Some("an unidentified hypervisor");
        }
        None
    })
}

/// Say when this is a virtual machine, and make the one allowance that has
/// survived scrutiny (auto-copy decode happens in mpv.rs). Everything bundled
/// speaks to the sound server the machine's own way.
pub fn audio_environment() {
    if let Some(vm) = virtualization() {
        // No PIPEWIRE_LATENCY request here any more. Asking for a 2048-sample
        // quantum made playback worse, not better: the audio clock advances
        // once per quantum, so a large one turns the clock video is paced by
        // into 43ms lurches. mpv's autosync smooths the jitter instead — see
        // the option in mpv.rs — and buffer sizing belongs to the sound
        // server's own VM profile, not to one application.
        crate::log::line(&format!("[clicker] running under {vm}"));
        if !std::path::Path::new("/usr/share/wireplumber/wireplumber.conf.d/alsa-vm.conf").exists()
            && !std::path::Path::new("/etc/wireplumber/wireplumber.conf.d/alsa-vm.conf").exists()
        {
            crate::log::line(
                "[clicker] this system lacks PipeWire's VM audio profile (alsa-vm.conf: \
                 api.alsa.period-size=1024, api.alsa.headroom=8192); if audio still \
                 underruns, that file is the system-wide fix",
            );
        }
    }
}
