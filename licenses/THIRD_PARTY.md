# Third-party components

RustDVR ships with, and is built against, software written by other people.
This file is the corresponding-source record the LGPL requires, and the honest
accounting the rest deserves.

## FFmpeg — LGPL-2.1-or-later

RustDVR decodes all audio and video with FFmpeg, distributed here as separate,
unmodified DLLs (`avcodec`, `avformat`, `avfilter`, `avutil`, `swscale`,
`swresample`) loaded at runtime. They may be replaced with any compatible
build; nothing about RustDVR prevents or penalizes that, and section 6 of the
LGPL is what guarantees it.

* Source: <https://github.com/FFmpeg/FFmpeg>, tag `n7.1.1`
* Configuration: built by `scripts/build-ffmpeg.ps1` in the RustDVR source
  tree, with `--disable-gpl --disable-nonfree`. The exact configure line is
  recorded inside the binaries themselves and can be read back with
  `av_license()` / `FFMPEG_CONFIGURATION`.
* License text: `LGPL-2.1.txt`, distributed alongside this file.
* Section 6 exception: RustDVR's own licence forbids modification and reverse
  engineering, which section 6 does not permit for the libraries it covers.
  `LICENSE.md` therefore carries an explicit exception allowing both, for the
  LGPL libraries, to the extent that section requires. Without it this
  combination could not be distributed at all.

FFmpeg is a trademark of Fabrice Bellard, originator of the FFmpeg project.

## Rust crates

The Rust dependencies in `Cargo.toml` and their transitive dependencies are
predominantly MIT OR Apache-2.0. `cargo license` in the source tree produces
the full inventory.

## Segoe UI Variable and Segoe Fluent Icons

Microsoft fonts, read from the operating system at runtime. Not redistributed.

## Channels DVR

RustDVR is an independent, unofficial client for a Channels DVR server. It
contains no Channels code and is not derived from any Channels application. It
is not affiliated with, endorsed by, sponsored by, or supported by Fancy Bits,
LLC. "Channels" and "Channels DVR" are the property of their respective owners
and appear here only to identify the server this program talks to.
