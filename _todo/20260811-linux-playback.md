# TPP: Linux playback — stutter, freezes, and the audio clock

Status: **Phase 7 — final verification pending.** Root cause identified via
research (not theory): the VM guest lacked PipeWire's VM audio profile, so its
emulated sound device underruns by design, and on Linux mpv paces video by the
audio clock. Guest fixed + app-side VM support shipped. Awaiting one clean test.

Test bench: David's Parallels VM "Ubuntu 24.04.3 ARM64" (**4 cores**, 3.8G,
virtio-gpu/virgl, PipeWire). `prlctl exec "Ubuntu 24.04.3 ARM64" '<cmd>'`.
App log: `/home/parallels/.local/share/Clicker/player.log`, one block per
`=== Clicker 1.1.7 started ===`. David sometimes installs/uninstalls while
you test — check `dpkg -s clicker` + `md5sum /usr/lib/clicker/clicker` vs the
CI deb before trusting any run's result. HTTP bridge to VM:
`python3 -m http.server 8731 --bind 10.211.55.2` in ~/Downloads/clicker-appimage.

## Phases

- [x] 1. Packaging: .deb (AppImage+Flatpak deleted). setup.sh menu:
      deb/source/uninstall. Makefile. Uninstall in app menu + self-elevating
      uninstall.sh.
- [x] 2. Audio existed at all: alsa+pulse enabled, ldd-based gate, cache key.
- [x] 3. Shared-code render bugs, all platforms: CPU-based skip; window-size
      FBO; aspect-true height; double-buffered surface. Windows+macOS compile
      green; behavior-neutral there.
- [x] 4. Render thread (own EGL context). WORKS but flashed white under virgl
      → **parked behind CLICKER_RENDER_THREAD=1** (default off) until real
      hardware tests it. Architecture is right; keep it.
- [x] 5. Audio clock convicted. Timeline fact: video was smooth in the mute
      builds; every stutter dates from the first build with sound. mpv logged
      "Audio/Video desynchronisation" + "Audio device underrun detected".
- [x] 6. Native PipeWire: libpipewire 1.0.7 built in build-mpv.sh
      (auto_features=disabled; spa-plugins MUST stay enabled — see lore),
      -Dpipewire=enabled, bundled (MIT), `ao=pipewire,` asked BY NAME (mpv
      probes pulse first!), SPA_PLUGIN_DIR/PIPEWIRE_MODULE_DIR fixup at
      startup. Gate prints `audio: libasound libpipewire libpulse`.
- [x] 6b. **The researched root cause** (Arch bbs 280654 + PipeWire docs
      "Stuttering audio in VMs"): guests need api.alsa.period-size=1024 +
      api.alsa.headroom=8192 (upstream alsa-vm.conf). Ubuntu 24.04 DOES NOT
      SHIP IT; `systemd-detect-virt`=parallels but no profile. **Written to
      David's VM** at /etc/wireplumber/wireplumber.conf.d/alsa-vm.conf +
      services restarted.
- [x] 6c. VM support in Clicker (Linux-only): `platform::virtualization()`
      reads 4 DMI fields + CPU hypervisor flag (Parallels/VMware/Proxmox/
      QEMU/KVM/Bochs/VirtualBox/innotek/Xen/Hyper-V/EC2/GCE); when virtual:
      PIPEWIRE_LATENCY=2048/48000 (if unset), mpv audio-buffer=1.0,
      hwdec=auto-copy (same as flatpak), log lines incl. hint when the system
      lacks alsa-vm.conf. `in_vm()` in mpv.rs, false off-Linux.
- [ ] 7. **VERIFY, then release 1.1.7.** Deb run 31480349575 (sha 2e76af7) has
      everything except the wider-hypervisor-net commit (next build). Windows
      31475999301 + macOS 31476001506 built, unplaytested. release.yml passes
      notarize:true; publishing debs makes setup.sh's fast path live.

## Next session: do this first

1. Deb from 31480349575 (or newer): install in VM, launch, play. Expect in
   log: `running under Parallels`, `AO: [pipewire]`, NO underrun lines. If
   David reports smooth: Phase 7 → cut release. Windows + macOS need one
   play-test each (builds listed above; both predate the VM-support commits
   but those are Linux-only — rebuild anyway for the release).
2. If still bad WITH the guest profile + VM support: David said hold — write
   the state down, ship what's proven, seek a real-hardware Linux tester.
   Remaining levers documented, none cheap: CLICKER_RENDER_THREAD=1 on real
   hardware; PipeWire quantum tuning; virgl is not fixable from userspace.

## Lore (hard-won; keep)

- **On Linux mpv paces VIDEO by the AUDIO clock** (`video-sync=audio`). A bad
  sound device presents as a video problem. "Fine with no audio" was the tell
  that unlocked everything; underruns in the log went uninvestigated for
  hours while the renderer was rebuilt twice. Read the whole log first.
- **mpv AO probe order tries pulse BEFORE pipewire** — a build can carry a
  native pipewire AO that is never used. `AO: [pulse]` + libpipewire on disk
  was the proof. Fixed with `ao=pipewire,` (trailing comma = fallback).
- **PipeWire 1.0.7 meson**: `-Dspa-plugins=disabled` is fatal (module refs
  `audioconvert_dep` unconditionally; fails at src/modules/meson.build:125).
  Keep plugins ON + `-Dauto_features=disabled`; stage only libpipewire-0.3.*.
- **Bundled libpipewire needs host paths**: SPA_PLUGIN_DIR/PIPEWIRE_MODULE_DIR
  set at startup (platform/linux.rs audio_environment) else the baked build
  prefix (nonexistent) is searched.
- **VM detection**: /sys/devices/virtual/dmi/id/{sys_vendor,product_name,
  board_vendor,bios_vendor} lowercased + needle list; /proc/cpuinfo
  "hypervisor" flag as catch-all. Proxmox stamps QEMU (old: Bochs);
  VirtualBox stamps innotek.
- **Render thread lore** (all still true, code parked behind env):
  eglGetCurrentContext during a paint → shared sibling ctx, surfaceless;
  FBOs per-context, textures+GLsync shared; mpv = ONE render ctx per handle
  (warm-up at main.rs ~1126 must yield via threaded_active());
  BLOCK_FOR_TARGET_TIME=1 holds mpv's internal lock → UI futex-froze on
  report_swap (threaded path: block=0, NO report_swap); bare condvar loses
  mid-render notifies → latched `pending` AtomicBool. White flashes on virgl
  = cross-context handoff; untested on real GPUs.
- **virgl**: paint thread caught in DRM_IOCTL_VIRTGPU_WAIT (0xc0086448, nr
  0x48 aarch64) at 0.02 core — waiting, not working; fences/swap throttle all
  GL on the thread. Skipping/downscaling cannot fix waiting.
- **Diagnosis kit**: /proc/PID/task/*/{comm,status,wchan,syscall} via prlctl;
  freeze-catcher Monitor = poll utime, 15s flat → dump; hash the installed
  binary vs CI deb before believing a test; /proc environ shows only the
  INITIAL env (set_var invisible).
- **set -e killed scripts 3×** via `[[ t ]] && cmd` / `|| v=$(pipe)`; grep -x
  on binaries never matches (no lines) — use ldd. **install.sh at repo root
  breaks autoconf** two dirs down (aux-dir probe) → installer is setup.sh.
- **CI**: cache fingerprint = sed comments away, grep tags+flags (now incl.
  PIPEWIRE_TAG + audio + Dpipewire). gh workflow run can RACE the push — a
  dispatch grabbed the pre-push sha once; verify run headSha, cancel+redo.
  Python heredoc replaces MUST assert; one silent miss shipped an unfixed
  build and a lying commit was barely avoided (second time it DID lie in the
  message body — commit cc106b9 note).
- **macOS floor unresolved**: Homebrew dylibs minos 15.0 vs plist 12.0.
  dev-macos.sh builds mpv itself; no Homebrew fallback exists on macOS.
- **apt reinstalls same-version fine**; consider 1.1.7+g<sha> stamps someday.

## Failed approaches (do not retry)

- Wall-clock skip; escalating tiers (03ff434). Downscaling under load
  (65cf548→947a509) — also squeezed until aspect fix b5cff1e. vsync off
  (7dba906→reverted a6881c4): guide tearing. Double-buffer as THE fence fix
  (cdf6798): right class, wrong resource. report_swap / block=1 on worker.
  Blaming stale binaries/caches (hash-disproven). Per-option spa-plugin
  disables in pipewire meson (2 identical failures before auto_features).
- Fixing VM audio from app code alone — device-level buffers are wireplumber
  config; app can only request its own quantum + buffer deep (done).

## Open loose ends

- #12 macOS local-network permission (os error 65) — untouched all session.
- Wider-hypervisor-net commit is on main but NOT in deb 31480349575 — next
  build carries it (behavior identical for Parallels).
- in_flatpak() path kept deliberately for a future Flathub attempt.
- David asked (interrupted, never done): dev-macos.sh fully brew-free — it
  still needs brew for meson/ninja/nasm/libass/libplacebo build deps.
- Maintainability trio: keys.rs 17 cfgs → platform::DEFAULT_BINDINGS;
  check.yml pins deprecated macos-14; mpv tags duplicated ps1/sh.
- README/NOTICE/THIRD_PARTY updated for PipeWire + VM support NOT yet
  documented in README (CLICKER_RENDER_THREAD + VM behavior undocumented).
