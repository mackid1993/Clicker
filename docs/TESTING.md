# Testing a change, and testing a release

Three platforms, one codebase, and one failure mode that keeps costing whole
evenings: playback that is measurably broken while every counter in the
application reads healthy. This is how to catch it in a minute instead.

---

## The one command

```sh
scripts/smoke-play.sh ~/test60.mp4              # macOS and Linux
```
```powershell
.\scripts\smoke-play.ps1 C:\test60.mp4          # Windows
```

It plays the source for forty-five seconds with no interface interaction, reads
the log the player already writes, and answers with a number and an exit
status:

```
    drawn   58.5   decoder   60.0   mpv dropped 3
    drawn   60.3   decoder   60.0   mpv dropped 3
    drawn   60.1   decoder   60.0   mpv dropped 3

    100% of decoded frames reached the screen (4 samples)
    mpv dropped 0 frames during the test

PASS
```

Any file or address works; a live channel is
`http://<dvr>:8089/devices/ANY/channels/<number>/stream.mpg`. Use `-s` for a
longer run, `-b` to point at a particular binary — an installed one, or a
`.app` — rather than the build in the tree.

### Why that number and not another

**Frames that reached the screen, against frames the decoder produced.** Every
other figure this application keeps can read perfectly while playback is
visibly broken, and each of them has: a decoder holding a steady sixty, a
renderer reporting no error, a processor barely working. The renderer falling
behind the decoder is the one thing that cannot happen quietly, and it is
exactly what a person watching a picture cannot judge reliably.

The thresholds were set against real recordings rather than chosen: the
configuration that shipped broken scored 82% with mpv shedding 117 frames, and
the one that fixed it scored 100% with none. The bar is 95%, so a single
dropped frame does not fail a build, and the first sample is discarded because
it covers the seconds where the stream was still opening.

---

## Reading the log by hand

One line every five seconds while something plays, wherever the platform keeps
application data:

| | |
|---|---|
| Windows | `%LOCALAPPDATA%\Clicker\player.log` |
| macOS | `~/Library/Application Support/Clicker/player.log` |
| Linux | `~/.local/share/Clicker/player.log` |

```
[mpv] 59.8fps drawn, 70.0fps painted, at 1280x720; decoder 60.0fps,
      mpv dropped 3, late 0; render 4.6ms at 0.04 of a core
```

- **drawn** — pictures mpv actually rendered. This is playback.
- **painted** — times the interface put something on screen. Higher than
  *drawn* means the window is repainting faster than the video arrives, which
  is a responsive interface, not a fault.
- **decoder** — what the decoder managed. Below the source rate means frames
  are not arriving or not decoding: a network or a processor problem, upstream
  of anything the renderer does.
- **mpv dropped** — mpv's own count of frames it threw away as late.
- **render** — how long the render call took, and what share of a core it
  spent. **Both numbers or neither**: a long time at a low share is a thread
  waiting on a driver, which no amount of skipping frames or drawing fewer
  pixels will help; a long time at a share near one is work.

A startup line names the graphics stack, which is the first question to ask of
any rendering complaint:

```
[clicker] GL Mesa · virgl (Apple M4 Pro (Compat)) · 4.0 (Core Profile) Mesa 25.2.8
```

---

## Environment variables

All of them work on every platform. None is needed to use the application; each
exists because the alternative was rebuilding to answer a question.

| | |
|---|---|
| `CLICKER_PLAY=<file or URL>` | Open that source at startup. No server, no sign-in, no hand on the mouse — this is what makes playback drivable from a script. |
| `CLICKER_MPV_OPTS="profile=fast;hwdec=no"` | mpv's own options, applied last so they win. **Semicolons**, because commas are already inside half of mpv's values. |
| `CLICKER_AO=null` | Silence the audio output. Ten seconds to learn whether a sound device is what is ruining the picture, which on Linux it can be — video is paced by the audio clock there. |
| `CLICKER_RENDER_THREAD=1` / `=0` | Force mpv's rendering onto a thread of its own, or back into the interface's paint. A two-way switch on every platform, overriding the default described below — and the way that default was arrived at. |
| `CLICKER_VIDEO=window` | Hand mpv the window itself, with no offscreen target and no blit. It overdraws the interface; it is a measuring instrument, not a mode. |
| `CLICKER_OPENGL=<path>` | Windows only. A software OpenGL to use instead of the one the installer ships, named as a file or as the directory holding it. What it names is loaded only when Clicker is drawing with the software renderer — Settings, Video, **Draw with** — or when the machine turns out to have no OpenGL that can compile a shader. |
| `CLICKER_PROBE=none` / `=old` | Windows only. Make the startup probe report a machine with no OpenGL at all, or with the unshaded 1.1 a headless session gets. Only the probe lies — whatever actually loads still draws — so every decision downstream of it (the fallback, the flip away from **Graphics chip**, the settings row refusing it) can be walked on a machine whose graphics work. |

---

## Why Linux renders on its own thread and the others do not

Worth knowing before changing anything under `src/mpv.rs`, because the
architecture looks like over-engineering until the number is in front of you.

Where OpenGL is translated rather than native, a render call can hold for most
of a frame interval while doing no work at all — measured at 28ms while
spending 0.02 of a core, which is a thread waiting on a driver, not a slow
renderer. On the interface's own thread that is a window that stops answering
the mouse and a picture shedding half its frames. On a worker it is a thread
whose whole job is to wait.

Same binary, same live 1080p60 channel, back to back:

| | frames drawn | mpv dropped | render call |
|---|---|---|---|
| render thread on | 59.8–60.4 fps | 1 in 60s | 3–6ms |
| `CLICKER_RENDER_THREAD=0` | 44–53 fps | 221 in 45s | 0.3–1.1ms |

The render call did not get faster. The waiting moved.

The same code runs on all three platforms; what differs is the default. Where
video is paced against the display — Windows and macOS — the thread is opt-in
via `CLICKER_RENDER_THREAD=1`, because `display-resample` needs drawing and
presenting to be one loop it can time against and a worker makes them two. The
A/V offset that produces is small, visible in the stats card, and will not sit
still. Linux paces against the audio clock, needs no such feedback, and is
where the stalling driver is a measured problem, so there the thread is on.

The worker publishes only frames the GPU has finished, by calling `glFinish`
before handing one over. An earlier version passed a GL fence across the
threads instead and produced white flashes on a virtualised GPU, because a
fence is precisely the primitive that driver gets wrong. **No fence crosses
threads. Do not reintroduce one.**

---

## A fast loop for Linux

Linux is the platform most likely to need iteration and the least convenient to
build for from a Mac or a PC. A checkout in the guest with mpv already built
turns a twenty-minute package round trip into sixteen seconds:

```sh
# in the guest, once
git clone https://github.com/mackid1993/Clicker && cd Clicker
make deps && ./scripts/build-mpv.sh

# thereafter
git pull && cargo build --release
LD_LIBRARY_PATH=third_party/mpv ./target/release/clicker
```

To test a working tree that has not been pushed, serve it from the host and
fetch it in the guest:

```sh
tar czf src.tgz src Cargo.toml Cargo.lock build.rs
python3 -m http.server 8731 --bind <host-only address>
# in the guest:  curl -sf http://<host>:8731/src.tgz | tar xz && cargo build --release
```

---

## What CI proves, and what it does not

`Check` compiles the crate on Windows, macOS and Linux, and verifies that the
two mpv builders pin the same versions. It runs on every push to `main` that
touches code, and on dispatch for a branch. **It cannot see a picture.** A
squeezed frame, a dropped one, a window that has stopped answering — none of
that is visible to a compiler, which is why the smoke test exists and why it
has to be run by hand on a machine of each kind.

The platform builds — `Build Windows`, `Build macOS`, `Build Linux` — are
dispatch-only and produce installers to try. `Release` assembles a version from
all three.

---

## Before cutting a release

1. `Check` green on all three platforms.
2. `scripts/smoke-play.sh` (or `.ps1`) **PASS on each platform you ship**, on
   real hardware. A virtual machine is a fine place to find a bug and a poor
   place to judge one: its network and its translated GPU both fail in ways
   that look like application faults and are not.
3. A resize and a fullscreen toggle during playback on each platform. The
   framebuffer is sized to the on-screen rectangle and follows its aspect, so
   geometry faults show up here and nowhere else.
4. Play something with the guide open over it, and something with captions on.
5. Build the installers, and check that the version they stamp is the version
   you meant — CI stamps `Cargo.toml` from the workflow input at build time, so
   the number in the file is not the number that ships.
