# Third-party components

Clicker ships with, and is built against, software written by other people.
This file is the corresponding-source record the LGPL requires, and the honest
accounting the rest deserves.

## mpv — LGPL-2.1-or-later

Clicker plays all video through mpv, distributed here as `libmpv-2.dll`: one
unmodified library beside the executable, with no plugin or script directories
and nothing loaded from outside the installation folder. Clicker opens it by
name at runtime and drives it through the public `libmpv` client and render
APIs, so it may be replaced with any compatible build.

* Source: <https://github.com/mpv-player/mpv>, tag `v0.41.0`
* Configuration: built by `scripts/build-mpv.ps1` in the Clicker source tree,
  with `-Dgpl=false -Dlibmpv=true`. mpv is GPL-2.0-or-later by default and only
  LGPL when built this way; `build.ps1` reads the license string out of the
  finished DLL and refuses to package a GPL one.
* License text: `LGPL-2.1.txt`, distributed alongside this file.

The other libraries beside the executable are what mpv was linked against:
libass, libplacebo, FreeType, HarfBuzz, FriBidi, Fontconfig, GLib, libpng,
libjpeg, lcms2, Graphite, Brotli, expat, libiconv, gettext, libunibreak,
libdovi, zlib, bzip2, shaderc, SPIRV-Cross, and the GCC and winpthreads
runtimes. They are permissively or weakly licensed (MIT, ISC, BSD, zlib,
LGPL-2.1, MPL-2.0, and the FreeType license), each carries its own terms in its
own source tree, and every one of them is a separate, replaceable file here for
the same reason FFmpeg is.

## FFmpeg — LGPL-2.1-or-later

FFmpeg does the decoding underneath mpv. It is distributed here as separate,
unmodified DLLs (`avcodec`, `avformat`, `avfilter`, `avutil`, `swscale`,
`swresample`) which may be replaced with any compatible build; nothing about
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

Clicker's own license forbids modification and reverse engineering, which
section 6 does not permit for the libraries it covers. `LICENSE.md` therefore
carries an explicit exception allowing both, for the LGPL libraries, to the
extent that section requires. Without it neither mpv nor FFmpeg could be
distributed here at all.

## Rust crates

The Rust dependencies in `Cargo.toml` and their transitive dependencies are
predominantly MIT OR Apache-2.0. `cargo license` in the source tree produces
the full inventory.

## Segoe UI Variable and Segoe Fluent Icons

Microsoft fonts, read from the operating system at runtime. Not redistributed.

## Channels DVR

Clicker is an independent, unofficial client for a Channels DVR server. It
contains no Channels code and is not derived from any Channels application. It
is not affiliated with, endorsed by, sponsored by, or supported by Fancy Bits,
LLC. "Channels" and "Channels DVR" are the property of their respective owners
and appear here only to identify the server this program talks to.
