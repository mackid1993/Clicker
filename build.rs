//! Compile the FFmpeg shim and link the libraries it calls.
//!
//! FFmpeg is expected at `third_party/ffmpeg`, built by `scripts/build-ffmpeg.bat`.
//! Set `FFMPEG_DIR` to use a copy from somewhere else.

use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let ffmpeg = std::env::var("FFMPEG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root.join("third_party").join("ffmpeg"));

    let include = ffmpeg.join("include");
    if !include.is_dir() {
        panic!(
            "FFmpeg headers not found at {}.\n\
             Build it first:  scripts\\build-ffmpeg.bat\n\
             Or point FFMPEG_DIR at an existing LGPL build.",
            include.display()
        );
    }

    cc::Build::new()
        .file("csrc/rd_media.c")
        .include(&include)
        // FFmpeg's headers use C99 designated initializers and inline; MSVC is
        // fine with those but warns extravagantly about its own CRT.
        .define("_CRT_SECURE_NO_WARNINGS", None)
        .warnings(false)
        .compile("rd_media");

    // Both lib/ and bin/. An MSVC build of FFmpeg puts the import libraries
    // next to the DLLs in bin/ rather than in lib/, so searching only the
    // conventional location fails with "cannot open input file avformat.lib"
    // even though the file is plainly there.
    for dir in ["lib", "bin"] {
        let path = ffmpeg.join(dir);
        if path.is_dir() {
            println!("cargo:rustc-link-search=native={}", path.display());
        }
    }
    for lib in ["avformat", "avcodec", "avutil", "swscale", "swresample"] {
        println!("cargo:rustc-link-lib=dylib={lib}");
    }

    embed_icon(&root);

    println!("cargo:rerun-if-changed=csrc/rd_media.c");
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
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
        let icon = root.join("assets").join("rustdvr.ico");
        println!("cargo:rerun-if-changed={}", icon.display());
        if !icon.exists() {
            println!("cargo:warning=assets/rustdvr.ico not found; run scripts\\make-icon.ps1");
            return;
        }

        // Which edition this compile is. Cargo tells the build script about
        // enabled features through the environment, and `--features win10` is
        // how the rustvcr binary — and only that binary — is built. The name
        // in the version resource is what Task Manager and the file's
        // Properties dialog show, so it has to match the product installed.
        let product = if std::env::var_os("CARGO_FEATURE_WIN10").is_some() {
            "RustVCR"
        } else {
            "RustDVR"
        };

        let mut res = winresource::WindowsResource::new();
        res.set_icon(&icon.to_string_lossy())
            .set("ProductName", product)
            .set("FileDescription", product)
            .set("CompanyName", "RustDVR")
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
