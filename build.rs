//! Put the icon and version details into the executable.
//!
//! That is all there is to do here. Nothing native is compiled and nothing is
//! linked: mpv is the player, it is loaded by name at runtime rather than
//! linked against, and it brings its own FFmpeg. So `cargo build` needs a Rust
//! toolchain and nothing else, which it did not before.

use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    embed_icon(&root);

    // The version goes into a Win32 resource, so a version change has to re-run
    // this script. Once any rerun-if line is printed, those become the *only*
    // triggers — without this one, `build.ps1 --ver` would bump Cargo.toml and
    // relink the previous version's resource, and the installer would carry a
    // binary that disagrees with its own filename.
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");
}

/// Put the application icon and version details into the executable itself.
///
/// Naming the .ico in the Inno Setup script only gives the *installer* an
/// icon. Explorer, the taskbar and Alt+Tab read a Win32 resource compiled into
/// the binary, and without one they show the generic default no matter how
/// good the artwork is.
///
/// Skipped rather than fatal when the icon is missing or no resource compiler
/// is available: an ugly icon should never be the reason a build fails.
fn embed_icon(root: &std::path::Path) {
    #[cfg(windows)]
    {
        let icon = root.join("assets").join("clicker.ico");
        println!("cargo:rerun-if-changed={}", icon.display());
        if !icon.exists() {
            println!("cargo:warning=assets/clicker.ico not found; run scripts\\make-icon.ps1");
            return;
        }

        let mut res = winresource::WindowsResource::new();
        res.set_icon(&icon.to_string_lossy())
            .set("ProductName", "Clicker")
            .set(
                "FileDescription",
                "A native Windows Unofficial Client for Channels DVR Server",
            )
            .set("CompanyName", "Clicker")
            .set("LegalCopyright", "PolyForm Noncommercial 1.0.0");

        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the icon: {e}");
        }
    }
    #[cfg(not(windows))]
    {
        let _ = root;
    }
}
