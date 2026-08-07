# Building RustDVR on a new machine

Two commands, once you have the prerequisites:

```powershell
.\bootstrap.ps1     # checks tools, fetches and builds FFmpeg  (~15 min, once)
.\build.ps1         # builds the app and the installer         (~2 min)
```

`bootstrap.ps1 -Install` will install anything missing via winget rather than
just telling you about it.

---

## What you need, and why

Everything here is a build-time requirement. **Nothing needs to be installed on
a machine that only runs the app** — the installer carries its own FFmpeg.

| Tool | Why | winget id |
|---|---|---|
| **Rust** (MSVC toolchain) | the app | `Rustlang.Rustup` |
| **Visual Studio 2022 Build Tools**, C++ workload | FFmpeg is built with `cl.exe`, and the `cc` crate compiles the C shim | `Microsoft.VisualStudio.2022.BuildTools` |
| **Git for Windows** | FFmpeg's `configure` is a POSIX shell script and needs a real `bash` | `Git.Git` |
| **NASM** | FFmpeg's x86 assembly, which is most of its decode speed | `NASM.NASM` |
| **GNU make** | FFmpeg's build. Strawberry Perl ships it as `gmake` | `GnuWin32.Make` |
| **Inno Setup 6** | the installer. Only needed to package | `JRSoftware.InnoSetup` |

The Build Tools install must include **Desktop development with C++**. A
default Build Tools install has no compiler at all, and `bootstrap.ps1` will
tell you so rather than failing later with something cryptic.

## What the FFmpeg build does

`scripts/build-ffmpeg.ps1` fetches FFmpeg `n7.1.1` and builds it with MSVC.
Three things about it are deliberate:

**It is built from source rather than downloaded.** Every prebuilt FFmpeg for
Windows — including the ones inside libVLC and libmpv — is configured with
`--enable-gpl`. Shipping one of those inside a noncommercially licensed
application would place the whole distribution under the GPL, overriding its
terms entirely. This build passes
`--disable-gpl --disable-nonfree`, the script refuses to continue if
`config.h` disagrees, and `build.ps1` re-reads the finished DLL before
packaging it.

**Only the encode side is dropped** — programs, docs, avdevice. Every decoder,
demuxer, parser, protocol and hardware accelerator is included, because a
client that cannot open a recording is worse than one that is larger.

**Several Windows-specific workarounds live in that script**, each commented
where it is applied:

- GNU make on Windows falls back to `cmd.exe`, which caps a command line at
  8,191 characters and does not treat single quotes as quotes. Linking
  libavcodec passes ~26,000 characters of object paths in one go. The script
  hands make a real `sh.exe` by its 8.3 short name.
- Even with a proper shell, `makedef` and the linker both exceed the 32,767
  limit, so `config.mak` and `library.mak` are patched to pass object lists
  through response files written by GNU make's `$(file …)`.
- FFmpeg's dependency generation pipes through an `awk` program whose escaping
  cannot survive the trip; it is disabled, which means **`-Reconfigure`
  implies a clean**, or objects from two configurations get linked together.

None of this is guesswork carried in someone's head — it is all in
`scripts/build-ffmpeg.ps1` with the failure it prevents written next to it.

## Rebuilding

| Situation | Command |
|---|---|
| Changed Rust or C shim code | `.\build.ps1 -Target App` |
| Want the installer | `.\build.ps1` |
| Changed FFmpeg's configuration | `scripts\build-ffmpeg.ps1 -Reconfigure` |
| FFmpeg build is confused | `scripts\build-ffmpeg.ps1 -Clean -Reconfigure` |

`build.ps1` also accepts `-Target Stage` to assemble `dist\RustDVR` without
compiling an installer, which is the fastest way to test a real layout.

## Running from the build tree

`dist\RustDVR` is a complete, runnable copy. To run `target\release\rustdvr.exe`
directly instead, the FFmpeg DLLs have to be findable:

```powershell
$env:PATH = "$PWD\third_party\ffmpeg\bin;$env:PATH"
.\target\release\rustdvr.exe
```

It prints its FFmpeg version and license to stderr on startup — that line is
read out of the binary, not a claim in a config file.

## Privacy of the output

Build paths and your username are kept out of the shipped binaries:

- FFmpeg is configured with a neutral `--prefix=/rustdvr` (the real
  destination goes to `make install`), because FFmpeg records its entire
  configure line inside the binary where anyone can read it
- `--extra-cflags=/d1trimfile:` strips the source prefix MSVC otherwise bakes
  into `__FILE__` assertion strings
- `RUSTFLAGS=--remap-path-prefix` rewrites the cargo registry, the repository
  root and the user profile
- `build.ps1` greps the finished binaries for your profile name and warns if
  any of that failed

## Reproducing on a clean machine

```powershell
git clone <repo> RustDVR
cd RustDVR
.\bootstrap.ps1 -Install
.\build.ps1
```

The FFmpeg source and build tree are gitignored; `bootstrap.ps1` fetches them
from the pinned tag.
