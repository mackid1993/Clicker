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

- [ ] Rebuild all three platforms and cut 1.1.7. Windows and macOS are
      untouched by this session's changes except the font fallback, which is
      `None` on both, and the compile is proven by `check.yml`.
- [ ] Playtest Windows and macOS once before release. Neither has been run
      since the port's later commits.

## Open: white triangles in the guide, on first launch only

Reported on the shipped .deb: the guide tore with white triangles on the very
first launch, and was clean after a video had played. Transient, self-curing,
and not the video path — nothing draws video on the guide, and the worker is
not spawned until something plays. The shape of it says egui's font atlas is
being drawn from before the first upload of it has landed, which a driver
translating OpenGL is exactly where you would expect to see. Not reproduced
from a script yet; the next step is to launch cold, capture the first second,
and see whether an empty atlas is what is on screen.

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
