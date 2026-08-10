# License

Clicker is Copyright © 2026 David Brustein.

Clicker is **open source** under the MIT License, reproduced below. Read it,
build it, run it, change it, fork it, share it, sell it — the only condition is
that the copyright notice travels with it.

The word **Software** in those terms means Clicker itself: the source code in
this repository and the binaries built from it. It does not mean the third party
components Clicker is built against, which carry their own licenses and are
covered separately under "Third party components" at the end of this file.

## MIT License

Copyright © 2026 David Brustein

Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in
the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
of the Software, and to permit persons to whom the Software is furnished to do
so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

---

# Third party components

Clicker is built against and ships alongside software written by other people.
Those components are **not** licensed under the terms above. Each is governed by
its own license, and where those licenses conflict with the terms above, the
component's own license prevails for that component.

## mpv

Clicker plays all video using mpv, which is licensed here under the GNU Lesser
General Public License version 2.1 or later. mpv is **dynamically loaded and
shipped unmodified as a separate shared library**, `libmpv-2.dll`, placed beside
the executable and opened by name at runtime through its public client and
render APIs. It is never folded into Clicker's own binary and never renamed, and
no plugins, scripts or configuration are loaded from anywhere.

mpv is GPL-2.0-or-later by default and is only LGPL when configured that way.
The corresponding source is mpv `v0.41.0`, from
<https://github.com/mpv-player/mpv>, built with `-Dgpl=false -Dlibmpv=true`.
`scripts/build-mpv.ps1` reproduces that build from the pinned tag, and
`build.ps1` reads the license string back out of the finished library and
refuses to package it if it reports GPL.

The libraries mpv itself was linked against ship the same way, beside the
executable, unmodified, each under its own license.

## FFmpeg

FFmpeg does the decoding underneath mpv. It is licensed under the GNU Lesser
General Public License version 2.1 or later, and is **shipped unmodified as
separate shared libraries** — `avcodec`, `avformat`, `avfilter`, `avutil`,
`swscale` and `swresample` — placed beside the executable and loaded at
runtime. It is never folded into Clicker's own binary and never renamed.
Clicker does not link against it directly; it reaches FFmpeg only through mpv.

The LGPL requires that anyone who receives Clicker be free to modify mpv and
FFmpeg, to relink Clicker against their modified versions, and to reverse
engineer as necessary to debug that relinking. The MIT License above restricts
none of that: it permits modification and reverse engineering outright, so
section 6's requirements are met with nothing further to carve out.

The corresponding source is FFmpeg `n7.1.1`, from
<https://github.com/FFmpeg/FFmpeg>. It is built from that source rather than
downloaded prebuilt, because the license has to be provable: every prebuilt
FFmpeg-bearing media binary for Windows that was examined embeds a build
configured `--enable-gpl`, and shipping one of those would place the entire
distribution under the GPL rather than under the licenses written on it.

This build is configured `--disable-gpl --disable-nonfree`. The configure line
is recorded inside the libraries themselves and can be read back at runtime
through `av_license()` and `FFMPEG_CONFIGURATION`; Clicker prints it on startup
and shows it in Settings under About. `scripts/build-mpv.ps1` reproduces the
build from the pinned tag, and `build.ps1` re-reads the shipped library and
refuses to package it if it reports GPL.

## Rust crates

The Rust dependencies listed in `Cargo.toml`, and their own dependencies, are
licensed by their respective authors, predominantly under MIT and Apache-2.0.
Their terms apply to them, not the terms above.

## Fonts and system components

Segoe UI Variable and Segoe Fluent Icons are Microsoft fonts installed as part
of Windows. Clicker reads them from the operating system at runtime and does not
redistribute them.

## Channels DVR

Clicker is an independent, unofficial client that talks to a Channels DVR server
over its public HTTP API. It is not affiliated with, endorsed by, sponsored by,
supported by, or derived from Channels or Fancy Bits, LLC. No Channels code is
used. "Channels" and "Channels DVR" are the property of their respective owners
and appear here only to identify the server this program communicates with.
