# Third-party components

Clicker ships with, and is built against, software written by other people.
This file is the corresponding-source record the LGPL requires, and the honest
accounting the rest deserves.

## mpv — LGPL-2.1-or-later

Clicker plays all video through mpv: one unmodified library, with no plugin or
script directories and nothing loaded from outside the installation. Clicker
opens it by name at runtime and drives it through the public `libmpv` client
and render APIs, so it may be replaced with any compatible build.

Where it lives, per platform:

| Platform | The library | Where it sits |
|---|---|---|
| Windows | `libmpv-2.dll` | beside the executable |
| macOS | `libmpv.2.dylib` | `Clicker.app/Contents/Frameworks` |
| Linux | `libmpv.so.2` | `usr/lib` inside the AppImage |

* Source: <https://github.com/mpv-player/mpv>, tag `v0.41.0`
* Configuration: built from that tag by `scripts/build-mpv.ps1` (Windows) or
  `scripts/build-mpv.sh` (macOS and Linux) in the Clicker source tree, with
  `-Dgpl=false -Dlibmpv=true`. mpv is GPL-2.0-or-later by default and only
  LGPL when built this way. Both scripts read the license string back out of
  the finished library and refuse to stage a GPL one, and `build.ps1` checks
  again before packaging.
* Nothing is taken from a package manager on any platform. Homebrew's FFmpeg
  is built `--enable-gpl` and its mpv links librubberband; a distribution's
  FFmpeg is frequently GPL too. Using either would put a GPL-combined work
  inside an MIT application, so Clicker loads only the library its own build
  produced — see `mpv_candidates` in `src/platform`.
* License text: `LGPL-2.1.txt`, distributed alongside this file.

The other libraries shipped alongside are what mpv was linked against:
libass, libplacebo, FreeType, HarfBuzz, FriBidi, Fontconfig, GLib, libpng,
libjpeg, lcms2, Graphite, Brotli, expat, libiconv, gettext, libunibreak,
libdovi, zlib, bzip2, shaderc, SPIRV-Cross, and each platform's own compiler
runtime. They are permissively or weakly licensed (MIT, ISC, BSD, zlib,
LGPL-2.1, MPL-2.0, and the FreeType license), each carries its own terms in its
own source tree, and every one of them is a separate, replaceable file for the
same reason FFmpeg is. On macOS they sit in `Contents/Frameworks`; on Linux in
`usr/lib` inside the AppImage; on Windows beside the executable. Which of them
are present varies by platform, because each build links what that platform
needs and nothing else.

Not shipped anywhere: anything GPL. That is a build-time decision — `-Dgpl=false`
for mpv, `--disable-gpl --disable-nonfree --disable-autodetect` for FFmpeg — and
a packaging-time check, not a promise.

## FFmpeg — LGPL-2.1-or-later

FFmpeg does the decoding underneath mpv. It is distributed as separate,
unmodified libraries (`avcodec`, `avformat`, `avfilter`, `avutil`, `swscale`,
`swresample`) — DLLs on Windows, dylibs on macOS, shared objects on Linux —
which may be replaced with any compatible build; nothing about
Clicker prevents or penalizes that, and section 6 of the LGPL is what
guarantees it. Clicker does not link against FFmpeg itself; it reaches it only
through mpv.

* Source: <https://github.com/FFmpeg/FFmpeg>, tag `n7.1.1`
* Configuration: `--disable-gpl --disable-nonfree`, built by
  `scripts/build-mpv.ps1`. The exact configure line is recorded inside the
  binaries themselves and can be read back with `av_license()` /
  `FFMPEG_CONFIGURATION`, and Clicker shows it in Settings under About.
* License text: `LGPL-2.1.txt`, distributed alongside this file.

FFmpeg is a trademark of Fabrice Bellard, originator of the FFmpeg project.

## Section 6 of the LGPL

Section 6 requires that anyone receiving Clicker be permitted to modify the
LGPL libraries, relink Clicker against the result, and reverse engineer as
needed to debug that. Clicker's MIT license permits modification and reverse
engineering outright, so those requirements are met with nothing to carve out;
the separate, replaceable DLLs above are the relinking half of the bargain.

## Rust crates

The Rust dependencies in `Cargo.toml` and their transitive dependencies are
predominantly MIT OR Apache-2.0. `cargo license` in the source tree produces
the full inventory.

## Segoe UI Variable and Segoe Fluent Icons

Microsoft fonts, read from the operating system at runtime on Windows. Not
redistributed.

## Fluent UI System Icons

Microsoft's MIT-licensed icon set, used on the platforms that do not ship
Segoe. `assets/FluentIcons-Clicker.ttf` is a subset of
`FluentSystemIcons-Regular.ttf` cut down to the glyphs the interface draws,
and is compiled into the macOS and Linux binaries only.

* Source: <https://github.com/microsoft/fluentui-system-icons>
* License text: `FluentSystemIcons-MIT.txt`, distributed alongside this file.

## Channels DVR

Clicker is an independent, unofficial client for a Channels DVR server. It
contains no Channels code and is not derived from any Channels application. It
is not affiliated with, endorsed by, sponsored by, or supported by Fancy Bits,
LLC. "Channels" and "Channels DVR" are the property of their respective owners
and appear here only to identify the server this program talks to.
