<div align="center">

<img src="assets/clicker.png" width="144" alt="Clicker">

# Clicker

**A Unoffical native Windows client for [Channels DVR](https://getchannels.com/) Server.**
<br>
**Not affiliated or supported by Fancy Bits, LLC.**

Live TV, recordings and a guide, in a single Rust binary.

[![Release](https://img.shields.io/github/v/release/mackid1993/Clicker?style=flat-square&color=6ca5fa&label=release)](https://github.com/mackid1993/Clicker/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/mackid1993/Clicker/total?style=flat-square&color=6ca5fa)](https://github.com/mackid1993/Clicker/releases)
![Platform](https://img.shields.io/badge/windows-10%201809%2B-6ca5fa?style=flat-square)
![License](https://img.shields.io/badge/license-MIT-6ca5fa?style=flat-square)

</div>

> **Clicker is not affiliated with Fancy Bits, LLC and is an unofficial client
> to Channels DVR Server.**
> It speaks to a Channels DVR server over its public HTTP API. It is not
> endorsed by, sponsored by, supported by, or associated with Fancy Bits, LLC
> in any way, it contains no Channels code, and it is not derived from any
> Channels application. "Channels" and "Channels DVR" are the property of their
> respective owners and are used here only to say what this program talks to.
> Do not ask Fancy Bits to support it — anything wrong with it is wrong with
> this project.

A native Windows client for [Channels DVR](https://getchannels.com/).

Live TV, recordings and a guide, in a single Rust binary with a Fluent
interface. Windows 10 1809 and up, Windows 11 included.

> **Status: 1.0.0.** It plays live TV and recordings, schedules, downloads and
> seeks. It has been run on the machines it was written on and not much else,
> so expect the occasional rough edge.

Installers are on the
[Releases page](https://github.com/mackid1993/Clicker/releases).

## What it does

- **Live TV** with timeshift — pause, rewind and return to live. Original comes
  straight from the tuner with no server-side pipeline at all, and is buffered
  to disk here so it can still be rewound. The buffer recycles rather than
  filling up, and how large it is allowed to get is a setting
- **Guide** across every channel, with logos, collections, sources and search.
  Left click watches, right click records; a future program records on left
  click
- **Recordings**, split into what is scheduled and what already exists, because
  a DVR with 303 recordings and 7,233 imported files is unusable as one list
- **Library** for imported media, kept separate from what the tuner recorded
- **Stream quality** chosen per session from the player, or globally in
  settings — the DVR does the transcoding
- **Commercial skip** using the DVR's own comskip markers, shown on the scrub
  bar with a skip button
- **Downloads** for offline viewing, two at a time with the rest queued, with a
  screen of their own — pause, resume, cancel, remove, and play with the
  network unplugged. A transfer survives a pause, a failure, a crash or a quit:
  the partial file is kept and picked up with a `Range` request rather than
  started again
- **Closed captions** — CEA-608/708, decoded out of the picture itself
- **Works offline.** The library is cached to disk, so downloads keep their
  titles and artwork with no server to ask, and a banner says the DVR has gone
  and offers what still plays
- **Home screen** — continue watching, up next, and what was recorded recently
- **Multiple DVRs**, switchable at any time
- **Can keep running in the notification area** when the window is closed, so a
  download that is nine tenths transferred survives it — off until asked for
- **Reconnects by itself** after sleep, a network change or a DVR restart
- Full screen, an auto-hiding transport, and keyboard control throughout

## What it cannot do

Stated here rather than left to be discovered.

**It will not appear in the DVR's client list by name.** Channels identifies a
streaming client by its IP address and nothing else. That was checked against a
real server, which keyed its activity on the address and ignored the User-Agent
along with every one of `client`, `client_name`, `device`, `device_name`, `name`
and `player` as query parameters — the activity key was byte-identical in all
seven cases. Names in the client list presumably come from the Bonjour
registration that Channels' own applications do, which is a separate,
undocumented protocol. The device name set in Settings is therefore sent in the
User-Agent, where it reaches the logs and stops.

**Windows only, and Windows 10 1809 at the oldest.** The custom caption, the
resize handles and the dark window chrome are all Win32, and 1809 is the
release that added the dark-mode attribute — below it a light system border is
drawn around an application that is entirely dark.

## The backdrop

The interface is translucent over a Mica-like material, which on Windows 11 is
something an application can simply ask the compositor for. Asking is also what
made this Windows 11 only: the material does not exist before build 22000, and
a window shaped to reveal it on Windows 10 is a transparent hole to the
desktop.

So it is painted instead — a soft, dark, slightly blue-lifted gradient,
brightest at the top left and falling away across the window. Four pixels,
stretched across the whole window with linear filtering: the hardware
interpolates between them, which is a perfectly smooth gradient for the cost of
a four-pixel texture and no per-frame work at all. Every translucent surface in
the theme reads against it exactly as it read against the real material.

## Why it is native

The obvious way to build this is a web view. It cannot work.

US broadcast and satellite feeds carry AC-3 audio in an MPEG transport stream,
and Chromium's Media Source Extensions can decode neither. Not "poorly", at
all. Verified directly:

```js
MediaSource.isTypeSupported('audio/mp4;codecs=ac-3')       // false
MediaSource.isTypeSupported('audio/mp4;codecs=ec-3')       // false
document.createElement('video').canPlayType('video/mp2t')  // ''
```

No JavaScript player changes that. hls.js, mpegts.js, video.js, Shaka and
dash.js all feed MSE and hit the identical wall. Channels' own applications play
transport streams because they use platform decoders, not a browser engine.

So the picture is decoded by mpv and drawn as a texture, in the same render
pass as the interface. There is no second window, no z-order to manage, no
transparency trick and no hit-testing to get wrong.

## Building

Two steps. Nothing is installed on the machine and nothing is written outside
the repository.

```powershell
.\bootstrap.ps1 -Install     # toolchain, then libmpv and FFmpeg from source
.\build.ps1                  # app, staged runtime, installer
```

The installer lands in `dist\` as `Clicker-Setup-<version>.exe`.

`build.ps1 -Target App` builds only the executable; `-Target Stage` stops
before the installer.

Requires Visual Studio 2022 build tools with the C++ workload (Rust links
through MSVC), MSYS2 (mpv cannot be built with MSVC), Git, and Inno Setup 6 for
the installer. `bootstrap.ps1` checks for all of them and, with `-Install`,
fetches what is missing.

Once `third_party\mpv` exists, `cargo build` on its own needs nothing but a
Rust toolchain: libmpv is loaded by name at runtime rather than linked, so
there is no native compilation step and no headers to find.

## Playback

mpv plays. All of it: demuxing, decoding, audio output, timing, subtitles.
`src/mpv.rs` is the whole of the integration — the slice of libmpv's C API this
needs, a render thread, and the same set of questions the interface asks of any
player.

It was not always this way. There used to be a hand-rolled pipeline here over
FFmpeg directly, ffplay's structure with a clock of its own, and it was
genuinely faster: 28% of one core against mpv's 87% on the same 1080p60
recording, because mpv's software renderer converts every frame on the CPU. It
was also one person's implementation of a problem mpv has been having solved
for twenty years. Timestamp discontinuities, damaged segments, odd containers,
streams that stop and start — every one of those arrives as a bug report about
a file nobody here can reproduce, and the honest answer to most of them was
going to be "mpv handles this".

So mpv handles it. There is no setting to switch back, because two players
means two sets of those reports and a first question of "which one were you
using", which is the wrong thing to ask someone whose recording will not play.

The library is loaded by name at runtime rather than linked. mpv cannot be
built with MSVC, so the DLL comes from mingw with a mingw import library an
MSVC target cannot use; `LoadLibraryW` sidesteps that entirely.

### Why not just link FFmpeg

That was the previous answer, and the trade is worth writing down. The pipeline
here decoded well and drew well. What it did not have was twenty years of other
people's edge cases: the version of "playback keeps pausing every two seconds"
that turns out to be a discontinuity in one broadcaster's stream, on a
recording that plays perfectly on the machine where the bug was reported and
nowhere else. Vendoring a player is how that class of report stops being
unfixable.

### Closed captions

Broadcast captions are not a stream. CEA-608 and CEA-708 ride inside the video
— in H.264 SEI user-data, in MPEG-2 picture user-data — so there is nothing to
select and nothing in a stream list to find. They exist only once pictures are
being decoded. mpv finds them and exposes them as ordinary subtitle tracks; the
CC button appears only on streams where one has actually been seen, rather than
sitting there permanently on files that will never have any.

### Timeshift

Live rewind is the server's, not ours. Channels' HLS output keeps every segment
from the moment a channel was tuned: `EXT-X-MEDIA-SEQUENCE` stays at 1 while the
segment list grows, so the whole session stays addressable and there is no local
buffer to write, manage or clean up. The direct `stream.mpg` endpoint is the
opposite trade, lowest latency and no transcode, but one long HTTP response with
nothing to seek within.

Which of the two is in use is read from the stream once it is open rather than
guessed from the URL, because plenty of playlists elsewhere are sliding windows
that cannot seek backwards at all.

## License

Clicker is **open source** under the MIT License. Read it, build it, run it,
change it, fork it, share it, sell it — the only condition is that the
copyright notice travels with it. See [LICENSE.md](LICENSE.md), and
[NOTICE.md](NOTICE.md) for the third party components, which carry their own
licenses.

### mpv and FFmpeg

Clicker uses [mpv](https://mpv.io/) and [FFmpeg](https://ffmpeg.org/), both
licensed under the **GNU Lesser General Public License version 2.1 or later**.
Neither is covered by the terms above and neither is owned by this project.

Both are built from source rather than downloaded, because the license has to be
provable. mpv is GPL-2.0-or-later unless it is configured otherwise, and every
prebuilt libVLC and libmpv binary for Windows embeds an FFmpeg configured with
`--enable-gpl`; shipping one of those would place the entire distribution
under the GPL rather than the licenses written on it. These are built
`-Dgpl=false` and
`--disable-gpl --disable-nonfree`, the build script refuses to proceed if the
configuration says otherwise, and `build.ps1` re-reads both shipped libraries
before packaging them.

They ship as **separate, unmodified DLLs loaded at runtime**, never folded into
the executable and never renamed. That is what lets anyone receiving a copy
substitute their own build of them, as LGPL-2.1 section 6 requires. The MIT
License already permits private modification and reverse engineering outright,
so the rest of what that section requires is met with nothing to carve out.

The corresponding source is mpv `v0.41.0` from
<https://github.com/mpv-player/mpv> and FFmpeg `n7.1.1` from
<https://github.com/FFmpeg/FFmpeg>, both built with the configuration recorded
in `scripts/build-mpv.ps1`. The libraries mpv itself links against ship
alongside, each under its own license; `licenses/THIRD_PARTY.md` lists them.

### Everything else

Rust dependencies are licensed by their own authors, predominantly MIT and
Apache-2.0. Segoe UI Variable and Segoe Fluent Icons are Microsoft fonts read
from the operating system at runtime and are not redistributed.

Clicker is an independent client that speaks to a Channels DVR server over its
HTTP API. It is not affiliated with, endorsed by, or derived from Channels or
Fancy Bits LLC, and contains no Channels code.
