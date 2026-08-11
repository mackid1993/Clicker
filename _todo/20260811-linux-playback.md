# TPP: Linux playback — stutter, freezes, and the audio clock

Status: **Resolved on the test bench; release verification is what is left.**
The fix was already committed and had never been run: 6228580 turned the
render thread on by default, and the build being tested was the one before it,
with the thread parked. Measured on David's Parallels guest against live
channel 356 (h264 1080p60), same binary, same channel, back to back:

| | frames drawn | mpv dropped | render call |
|---|---|---|---|
| render thread on (default) | 59.8–60.4 fps | 1 in 60s | 3–6ms |
| `CLICKER_RENDER_THREAD=0` | 44–53 fps | 221 in 45s | 0.3–1.1ms |

A nine-minute soak held 60fps with nine dropped frames total and flat memory.
David's own verdict on a channel he tuned himself: stable.

Why it works is not that the render call got faster — it did not — but that
the waiting moved. The interface no longer sits inside a driver call it cannot
hurry, and mpv is no longer starved of the render cadence its audio-paced
timing expects.

## What the earlier theories were worth

- **video-timing-offset=0** (214e740): correct, and confirmed against mpv's own
  documentation — "this applies only to audio timing modes; in
  `--video-sync=display-...` this option is not used". So it is live on Linux
  and genuinely inert on Windows and macOS, which is what the earlier note
  claimed and what makes it safe. It was not, on its own, the fix: the build
  that carried it still stuttered.
- **The audio clock** was a real symptom and a false lead. Underruns still
  appear in the log and playback is now smooth anyway.
- **PipeWire bundling** (removed in 9c04c87): correctly removed. `AO: [pulse]`
  plays fine at 60fps.
- **The white flashes that parked the thread** were the cross-context fence
  handoff, not the architecture. The worker now calls `glFinish` and publishes
  only finished frames; no fence crosses threads, and the fence machinery is
  gone from the code entirely.

## The file descriptor leak is the driver's, and it is proven

~60 `anon_inode:sync_file` descriptors per second, one per swap. **An idle
window with no video at all leaks at exactly the same rate** — that is the
measurement that settles it. Nothing in this application creates them; it is
Mesa/virgl on this guest. 6a1e5e7's raise to the hard limit stands as the
right mitigation: it turns a 23-second cliff at the old 1024 soft limit into
roughly five hours. Not a fix, and not ours to fix. **Open question for a long
session**: five hours of continuous playback still ends at the ceiling. Worth
checking whether a newer Mesa closes them, and whether the X11 path leaks the
same way — that test was set up and never run.

## Test bench and how to drive it

Parallels VM "Ubuntu 24.04.3 ARM64" — now **6 cores, 7.9G**, virtio-gpu/virgl
(`virgl (Apple M4 Pro (Compat))`, GL 4.0), GNOME on Wayland, PipeWire.
`prlctl exec "Ubuntu 24.04.3 ARM64" '<cmd>'`; `prlctl capture` takes a screen
grab from the host, though its output bands badly and a moving black band in
one is the capture, not the app.

**The dev loop is the thing to keep.** A full checkout with mpv already built
lives at `~/.cache/clicker-build` in the guest; `git fetch && cargo build
--release` there is **16 seconds**, against twenty minutes for a CI deb. Run
it with `LD_LIBRARY_PATH=~/.cache/clicker-build/third_party/mpv`. To ship the
working tree without pushing: `tar czf` src and serve it over
`python3 -m http.server 8731 --bind 10.211.55.2`, curl it in the guest.

Three environment variables now make a playback question answerable in a
minute instead of an evening, and they are why this was resolved in one
session rather than another all-nighter:

- `CLICKER_PLAY=<file or URL>` — open that source at startup, no server, no
  hand on the mouse. A live channel is
  `http://<dvr>:8089/devices/ANY/channels/<n>/stream.mpg`.
- `CLICKER_MPV_OPTS="profile=fast;hwdec=no"` — mpv's own options, no rebuild.
  Semicolons, because mpv's own values contain commas.
- `CLICKER_VIDEO=window` — mpv draws into the window itself, no offscreen
  target and no blit. It overdraws the interface and is a measuring
  instrument, not a mode.

The log line to read is one per five seconds: frames drawn, frames painted,
where, what the decoder managed, what mpv threw away, and what the render call
cost. Frames drawn is the number that decides it.

## Left to do

- [x] Rebuild all three platforms. Windows, macOS and both Linux
      architectures built green off 620f5e4, with `check.yml` compiling all
      three first.
- [ ] **Playtest Windows and macOS, then cut 1.1.7.** The port's later
      commits changed four things in *shared* render code and neither
      platform has been played since: the framebuffer sized to the on-screen
      rectangle rather than the stream, its height following that rectangle's
      exact aspect, the double-buffered surface, and the processor-time frame
      skip. A fault there would show as geometry — a squeezed, letterboxed or
      wrong-sized picture, especially after a resize or a fullscreen toggle —
      rather than as stutter. `video-timing-offset=0` is the fifth shared
      change and needs no test: mpv's documentation is explicit that it is
      unused under `display-resample`, which is what both platforms run.

## Closed: white triangles, and the guide taking a moment

**White triangles on the guide: gone, cause never identified.** An agent spent
a long pass on it and could not reproduce it in any captured frame — not out
of the application's own framebuffer (915 frames read back with glReadPixels
before the swap) and not out of a recording of the screen itself (637 frames
at 57fps through Mutter, virgl and Parallels). Three theories died on
measurement rather than argument, and each is worth not re-running:

- **The GL context is virgl on every launch, never llvmpipe.** The
  `Suspected software renderer or indirect context` line that looked like a
  lead is not one: mpv prints it when `glGetString` returns NULL as well as on
  a software renderer, and mpv's render context does not exist until playback
  starts, so it can say nothing about a guide drawn before any video.
- **The font atlas never grows.** Fixed at 8192x32 across thousands of frames,
  because roughly 550 glyphs fit in one row of it. The fallback font added in
  bd37a84 does not change that, and there is no mid-frame re-upload to race.
- **The image cache never evicts on the guide.** It peaked at 176 textures and
  65MB against limits of 300 and 192MB, so the artwork-flicker bug class of
  1.1.2 through 1.1.5 cannot fire there.

Worth keeping for next time: a missing logo texture *would* draw as a white
rounded quad in the channel column — `image_cover` builds a white `RectShape`
and hangs the texture off it — so that is the shape to look for and the place
to look. Untested: real pointer input, and dragging or resizing the window.
A photograph of it would be worth more than another agent.

**The guide taking a moment to load is Parallels, not the application.**
Settled by comparison rather than theory: a native Mac shows it immediately,
the same build in a VM does not, and both guests reach the server through the
same host network path. Measured from the guest: 5.1MB in 5.7s cold, 13.3MB in
1.4s warm. The application asks for a 24-hour window, which is a 25MB payload
and the size of the on-disk cache. If a cold guide is ever slow on real
hardware, `GUIDE_HOURS` in main.rs is the first knob — and it should not be
touched on the evidence of a virtual machine.

## Known and deliberately not fixed here

- **Recording the programme you are watching has never worked.** `Msg::Program`
  has had no sender since the first commit — `git log -S` proves it — so
  `self.airing` is always `None` and the record button in the transport always
  answers "Still loading the guide for this channel". `api::current_airing`
  and `api::padding` are the unused wiring it wants. Predates the port; left
  alone rather than deleted, because deleting it would remove the parts the
  fix needs.
- Guide fields parsed and never read (`hd`, `favorite`, `program_id`,
  `is_movie`), and `ui::Action::WatchLive`, which nothing constructs.
- macOS: Homebrew dylibs minos 15.0 against a plist floor of 12.0;
  `dev-macos.sh` still needs brew for meson/ninja/nasm.
- #12 macOS local-network permission (os error 65).
- Maintainability: keys.rs's 17 cfgs want a `platform::DEFAULT_BINDINGS`;
  check.yml pins a deprecated macos-14; mpv tags are duplicated between the
  .ps1 and .sh builders.

## Lore worth keeping

- **On Linux mpv paces video by the audio clock** (`video-sync=audio`), so a
  bad sound device presents as a video problem. Read the whole log first.
- **A slow render call and a blocked one look identical on the clock.** Wall
  time with the processor share beside it is what tells them apart: 28ms at
  0.02 of a core is a thread waiting, and no amount of skipping frames or
  rendering fewer pixels helps a thread that is waiting.
- **PipeWire 1.0.7 meson**: `-Dspa-plugins=disabled` is fatal. Keep plugins on
  with `-Dauto_features=disabled` if it is ever built again.
- **A virtual machine is not a special case**, and the code that decided it
  was is gone. It bought one option, hwdec=auto-copy, and with the render
  thread working, auto-safe measured identically: sixty frames a second, one
  lost in fifty. If it is ever wanted again: DMI's four fields plus the
  cpuinfo hypervisor flag, and Proxmox stamps QEMU while VirtualBox stamps
  innotek. A Flatpak still gets auto-copy, where the Mesa mismatch is real.
- **egui's bundled face stops not far past Latin.** On Linux nothing sits in
  front of it, so `→` drew as an empty box. Ask fontconfig for a face
  containing the glyph rather than guessing at distribution font paths.
- `set -e` has killed these scripts three times via `[[ t ]] && cmd`. CI cache
  fingerprints are grep'd out of the build scripts; a Python heredoc that
  edits a file **must assert** it matched.
- **Do not retry**: wall-clock frame skipping and escalating skip tiers;
  downscaling under load; vsync off (guide tearing); double buffering as the
  fence fix; `report_swap` on the worker; blaming stale binaries.
