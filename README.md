# RustDVR

> **Not affiliated with Channels or Fancy Bits, LLC.**
> RustDVR is an independent, unofficial client that speaks to a Channels DVR
> server over its public HTTP API. It is not endorsed by, sponsored by,
> supported by, or associated with Fancy Bits, LLC in any way, it contains no
> Channels code, and it is not derived from any Channels application. "Channels"
> and "Channels DVR" are the property of their respective owners and are used
> here only to say what this program talks to. Do not ask Fancy Bits to support
> it — anything wrong with it is wrong with this project.

A native Windows client for [Channels DVR](https://getchannels.com/).

Live TV, recordings and a guide, in a single Rust binary with a Fluent
interface. Windows 11 only, deliberately.

> **Status: 0.0.1, pre-release.** It plays live TV and recordings, schedules,
> downloads and seeks. It has not been run anywhere except the machine it was
> written on, and there will be rough edges.

## What it does

- **Live TV** with timeshift — pause, rewind and return to live
- **Guide** across every channel, with logos, collections, sources and search.
  Left click watches, right click records; a future programme records on left
  click
- **Recordings**, split into what is scheduled and what already exists, because
  a DVR with 303 recordings and 7,233 imported files is unusable as one list
- **Library** for imported media, kept separate from what the tuner recorded
- **Stream quality** chosen per session from the player, or globally in
  settings — the DVR does the transcoding
- **Commercial skip** using the DVR's own comskip markers, shown on the scrub
  bar with a skip button
- **Downloads** for offline viewing, two at a time with the rest queued, with a
  screen of their own — cancel, remove, and play with the network unplugged
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

**Downloads do not resume.** The whole file is requested each time, so a
cancelled or interrupted transfer starts again from the beginning. The partial
file is deleted rather than left occupying disk.

**Only Windows 11.** Mica, the custom caption and the resize handles are all
Win32.

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

So the picture is decoded by FFmpeg and drawn as a texture, in the same render
pass as the interface. There is no second window, no z-order to manage, no
transparency trick and no hit-testing to get wrong.

## Building

Two steps. Nothing is installed on the machine and nothing is written outside
the repository.

```powershell
git clone --depth 1 --branch n7.1.1 https://github.com/FFmpeg/FFmpeg.git third_party\ffmpeg-src
scripts\build-ffmpeg.ps1     # vendors FFmpeg, about ten minutes
.\build.ps1                  # app, staged runtime, installer
```

`build.ps1 -Target App` builds only the executable; `-Target Stage` stops before
the installer.

Requires Visual Studio 2022 build tools with the C++ workload, Git for Windows
(FFmpeg's `configure` is a shell script), nasm, and Inno Setup 6 for the
installer.

## Playback

FFmpeg decodes. Everything else is in `src/player/`, structured the way ffplay
is, for the reason ffplay is:

- one thread owns the demuxer and both decoders, because an `AVFormatContext` is
  not thread safe
- decoded frames go into bounded queues
- a **monotonic clock of the player's own**, and both streams are presented
  against it

A player that shows each frame as it is decoded looks correct for a minute and
then drifts, because the decoder does not run at exactly the frame rate and
nothing pulls it back. Something has to be the clock.

The textbook answer is the sound card: it consumes samples at precisely its own
rate and cannot be argued with, so what it has played is by definition where
playback has reached. That is what ffplay does, and this player deliberately
does not, because on Windows that rate is only observable through the audio
callback and the callback's arrival is at the mercy of DPC latency. A driver
servicing an interrupt late does not make the clock late — it makes it *jump*,
and every frame scheduled against it lands wrong.

So the master clock is `QueryPerformanceCounter`, free-running, and it is
corrected *towards* the audio device rather than driven by it: at most 0.2%
every two seconds, computed from how much audio is still buffered rather than
from when a callback happened to fire. Drift is bounded by the correction and
jitter never reaches the clock at all. Measured over a two-minute live tune:
7,453 frames, none dropped, no underruns, A/V skew inside ±5ms.

FFmpeg is reached through a small C shim in `csrc/` rather than through
generated bindings. Binding its structs directly would mean either dragging in
libclang for the sake of forty function signatures, or hand-transcribing
`AVFrame`, where one wrong field offset is silent memory corruption instead of a
compile error. The shim's surface is opaque pointers and scalars, so a mistake
is a link failure.

### Closed captions

Broadcast captions are not a stream. CEA-608 and CEA-708 ride inside the video —
in H.264 SEI user-data, in MPEG-2 picture user-data — so there is nothing for
`av_find_best_stream` to select and nothing in `nb_streams` to find. They exist
only once pictures are being decoded.

The shim pulls the A53 side data off each decoded frame and feeds it to FFmpeg's
own EIA-608 decoder, which is what turns control codes and roll-up positioning
into lines of text. The CC button appears only on streams where that data has
actually been seen, rather than sitting there permanently on files that will
never have any.

### Timeshift

Live rewind is the server's, not ours. Channels' HLS output keeps every segment
from the moment a channel was tuned: `EXT-X-MEDIA-SEQUENCE` stays at 1 while the
segment list grows, so the whole session stays addressable and there is no local
buffer to write, manage or clean up. The direct `stream.mpg` endpoint is the
opposite trade, lowest latency and no transcode, but one long HTTP response with
nothing to seek within.

Which of the two is in use is read from the demuxer rather than guessed from the
URL, because plenty of playlists elsewhere are sliding windows that cannot seek
backwards at all.

## Licence

RustDVR is **source available**: PolyForm Noncommercial 1.0.0. Read it, build
it, run it, change it, fork it and share it — for any noncommercial purpose.
What you may not do is sell it or use it to make money. See
[LICENSE.md](LICENSE.md).

That is not "open source" in the OSI sense, and the difference is worth stating
plainly rather than letting the badge imply otherwise: the Open Source
Definition forbids discriminating against any field of endeavour, and a
noncommercial restriction is exactly that. Forks, patches and redistribution are
all fine. Making money from it is not.

### FFmpeg

RustDVR uses [FFmpeg](https://ffmpeg.org/), licensed under the **GNU Lesser
General Public License version 2.1 or later**. FFmpeg is not covered by the
terms above and is not owned by this project.

It is built from source rather than downloaded, because the licence has to be
provable. Every prebuilt libVLC and libmpv binary for Windows embeds an FFmpeg
configured with `--enable-gpl`; shipping one of those inside a noncommercially
licensed application would relicense the entire distribution under the GPL and
override the terms above entirely. This build is
configured `--disable-gpl --disable-nonfree`, the build script refuses to
proceed if `config.h` says otherwise, and `build.ps1` re-reads the shipped DLL
before packaging it.

The libraries ship as **separate, unmodified DLLs loaded at runtime**, never
folded into the executable and never renamed. That is what lets anyone receiving
a copy substitute their own build of them, as LGPL-2.1 section 6 requires.
`LICENSE.md` carries the exception that section also requires, granting private
modification and reverse engineering for debugging unconditionally — including
to someone whose purpose is commercial, and who therefore holds no licence to
RustDVR itself.

The corresponding source is FFmpeg `n7.1.1`, from
<https://github.com/FFmpeg/FFmpeg>, built with the configuration recorded in
`scripts/build-ffmpeg.ps1`.

### Everything else

Rust dependencies are licensed by their own authors, predominantly MIT and
Apache-2.0. Segoe UI Variable and Segoe Fluent Icons are Microsoft fonts read
from the operating system at runtime and are not redistributed.

RustDVR is an independent client that speaks to a Channels DVR server over its
HTTP API. It is not affiliated with, endorsed by, or derived from Channels or
Fancy Bits LLC, and contains no Channels code.
