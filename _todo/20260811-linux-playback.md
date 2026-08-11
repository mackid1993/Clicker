# TPP: Linux playback — stutter, freezes, and the audio clock

Status: **Phase 5 in flight** — audio-clock theory awaiting the ten-second test.
Everything below Phase 5 is done and verified except where marked.

Test bench: David's Parallels VM "Ubuntu 24.04.3 ARM64" (now **4 cores**, 3.8G,
virtio-gpu/virgl, PipeWire). Reach it with
`prlctl exec "Ubuntu 24.04.3 ARM64" '<cmd>'` — read-only unless David says
otherwise ("don't poison my test"). App log:
`/home/parallels/.local/share/Clicker/player.log`, one block per
`=== Clicker 1.1.7 started ===`. The installed binary can be hash-compared
against a CI deb (`ar x`, `tar xf data.tar.zst`, md5) — done once to kill a
stale-binary theory; installs do land.

## Phases

- [x] 1. Packaging: .deb replaces AppImage/Flatpak (both deleted; reasons in
      linux.yml header). setup.sh one-liner menu: deb / source / uninstall.
      Makefile: deps/build/install/uninstall/deb/run. Uninstall in app menu.
- [x] 2. Audio missing entirely — mpv built with no AO. Fixed: alsa+pulse
      `enabled` in build-mpv.sh, headers everywhere, gate reads ldd of staged
      lib (`audio: libasound libpulse`), audio flags in the CI cache key.
- [x] 3. Shared-code render bugs (all platforms inherit, Windows+macOS builds
      green and behavior-neutral): wall-clock skip latch → CPU-based;
      stream-size FBO → window-size; independent round32 → aspect-true height;
      single-texture WAR hazard → double-buffered surface.
- [x] 4. mpv on its own thread + own GL context (Linux only, mpv.rs `mod
      worker`). Verified engaging in the VM: `[mpv] rendering on its own
      thread and context`, no fallback line.
- [ ] 5. **Audio clock** — CURRENT. David: "when there was no audio, video was
      fine". All stutter dates from the first build with sound. Linux uses
      `video-sync=audio` → video slaved to the audio clock; VM audio goes
      through pipewire-pulse shim (our mpv has NO native PipeWire AO — 22.04's
      too old to build against; the distro mpv that plays fine HAS it).
      `Audio device underrun detected` seen in log. CLICKER_AO env override
      shipped in a2541ab; deb building as run 31477352316.
- [ ] 6. If confirmed: build libpipewire from source in build-mpv.sh (like
      libass/libplacebo), enable mpv's pipewire AO, add to cache fingerprint
      + deps gate.
- [ ] 7. Cut release 1.1.7 (all three platforms already build green from
      a2541ab lineage; release.yml passes notarize:true; deb assets make
      setup.sh's fast path work — it currently finds no release assets).

## Next session: do this first

1. Check run 31477352316 (deb, arm64). Have David run, in order:
   - `clicker` — guide must be tear-free (vsync-off experiment was reverted
     in a6881c4; only run 31475220381's deb ever had it off).
   - `CLICKER_AO=null clicker` — **the decisive test**: silent + smooth video
     convicts the audio clock; silent + still bad acquits it.
   - `CLICKER_AO=alsa clicker` — may be smooth WITH sound (bypasses the shim).
2. If convicted → Phase 6. If acquitted → the remaining suspect list is empty;
   David said "if this doesn't work I hold off" — respect that, ship what
   works, revisit on real hardware.

## Lore (hard-won this session)

- **The VM is the harshest grader, and that was useful**: it surfaced 4 real
  shared-code bugs + libunibreak soname drift + the LuaJIT/hardened-runtime
  class of bug. But 2 cores couldn't decode 1080p60 + run egui (decode read
  55fps vs 60 → starvation → `Video: no video`, audio underruns). Now 4 cores.
- **Diagnosis beats theory**: /proc/PID/task/*/wchan + /syscall from outside
  the VM found the paint thread in `DRM_IOCTL_VIRTGPU_WAIT` (ioctl nr 0x48 on
  aarch64 = virtgpu WAIT, req 0xc0086448). A "freeze" with audio alive =
  stuck/starved paint thread, mpv threads never touch GL.
- **virgl punishes what desktop drivers hide**: same-texture write-after-read
  costs a host fence round-trip; Mesa throttles submissions against in-flight
  fences, so vsync swap fences backpressure ALL GL on that thread. 13-19ms
  wall at 0.02 core = waiting, not working. Skipping/downscaling can't fix
  waiting.
- **eframe hides its GL context** — that's why mpv rendered in-paint
  originally. Escape hatch: `eglGetCurrentContext`/`eglGetCurrentDisplay`
  during a paint, then `eglCreateContext(dpy, NO_CONFIG, share, {3,0})` +
  surfaceless make-current on the worker (Mesa allows both). Textures shared;
  FBOs are per-context containers (worker pair + UI pair over same textures);
  GLsync fences are shared — worker `glFenceSync`+`glFlush`, UI `glWaitSync`
  (server-side, costs nothing).
- **mpv render API internal lock**: `mpv_render_context_render` with
  `BLOCK_FOR_TARGET_TIME=1` holds mpv's render lock for the whole wait;
  `report_swap` takes the same lock → UI froze in a futex behind the worker's
  sleep (b9f7fd0 fixed: block=0 + no report_swap on threaded path;
  report_swap only feeds display-resample, Linux uses audio sync).
- **Bare condvars lose notifications**: mpv announces frames mid-render;
  notify with no waiter evaporates; worker slept its 50ms backstop → ~15fps
  "worse than before". Latched AtomicBool `pending`, consumed before sleeping
  (bededb0).
- **mpv allows ONE render context per handle**: the warm-up call in main.rs
  (~line 1126) creates the renderer at stream load, beating the first paint.
  `ensure_renderer` now yields to `threaded_active()` first (450fad9). The
  failure read `There is already a mpv_render_context set` then silent
  fallback.
- **`set -e` + `[[ test ]] && cmd` / `|| VAR=$(pipeline)` killed the script 3
  times** (uninstall dispatch, choose_action, audio gate). Audio gate also:
  anchored grep on a binary never matches (binaries have no lines) — ask
  `ldd` what's linked instead.
- **autoconf aux-dir trap**: a root-level file named `install.sh` (or
  install-sh/shtool) makes autoconf treat the REPO ROOT as libass's aux dir
  two levels up → `ltmain.sh not found`. Installer is `setup.sh` forever
  (note in build-mpv.sh, 74a2707).
- **CI cache keys**: fingerprint = `sed 's/#.*//'` then grep tags+flags of
  build-mpv.sh (comments must never bust or preserve the cache — both
  happened). Audio flags are IN the fingerprint now.
- **apt same-version reinstall works here** (dpkg log confirms all 6 landed;
  hash-verified) — but every dev build stamps 1.1.7, so consider
  `1.1.7+g<sha>` for dispatch builds someday.
- **`make deps` vs sudo**: `sudo make install` must never build (root-owned
  target/, cargo not on root's PATH) — install: `check-built` guard, not
  `build` dep. Uninstaller at `$PREFIX/lib/clicker/uninstall.sh`, prefix baked
  in, self-elevates via pkexec, survives source-tree deletion.
- **macOS floor**: universal .app verified (all 35 dylibs both arches,
  @rpath-clean) but Homebrew dylibs carry minos 15.0 vs plist's 12.0 promise —
  UNRESOLVED, David never chose truth-vs-rebuild. `dev-macos.sh` now builds
  mpv itself; there is NO Homebrew fallback in mpv_candidates on macOS.
- Freeze-catcher pattern (Monitor): poll pgrep + utime; on 15s flat CPU dump
  comm/state/wchan per thread. Re-arm next session if chasing hangs.

## Failed approaches (do not retry)

- Wall-clock frame skipping (03ff434 explains); escalating skip tiers.
- Downscaling under load (65cf548 → removed 947a509): quality cost, fixed
  nothing — the wait isn't pixel-bound; and it "squeezed" until b5cff1e.
- vsync off + hand-paced repaint (7dba906, reverted a6881c4): guide tearing.
- Double-buffering ALONE as the fence fix (cdf6798): right hazard class,
  wrong resource — the swap fence was the throttle, not the texture.
- report_swap on the threaded path; BLOCK_FOR_TARGET_TIME=1 on the worker.
- Blaming: stale binaries (hash-disproven), caches (disproven), the VM alone
  (mpv standalone plays fine there — that fact was the compass all day).

## Open loose ends

- #12 macOS local-network permission (os error 65) — old task, untouched.
- Guide tearing: attributed to the reverted vsync build; confirm next run.
- Windows installer (31475999301) + macOS universal (31476001506) built from
  the render-thread commit — David hasn't play-tested either yet.
- keys.rs still has 17 platform cfgs; check.yml pins deprecated macos-14;
  mpv tags live in both build-mpv.ps1 and .sh (drift risk) — all noted to
  David as the "4 → 3 maintainability" list, none started.
