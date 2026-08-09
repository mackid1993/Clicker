# Building Clicker on a new machine

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
| **Visual Studio 2022 Build Tools**, C++ workload | Rust's Windows target links through MSVC | `Microsoft.VisualStudio.2022.BuildTools` |
| **MSYS2** | mpv and FFmpeg are built with mingw-w64; mpv cannot be built with MSVC at all | `MSYS2.MSYS2` |
| **Git** | fetching mpv and FFmpeg at their pinned tags | `Git.Git` |
| **Inno Setup 6** | the installer. Only needed to package | `JRSoftware.InnoSetup` |

The Build Tools install must include **Desktop development with C++**. A
default Build Tools install has no linker at all, and `bootstrap.ps1` will
tell you so rather than failing later with something cryptic.

Once `third_party\mpv` exists, `cargo build` alone needs only Rust. libmpv is
loaded by name at runtime rather than linked, so nothing native is compiled and
no headers are looked for.

## What the mpv build does

`scripts/build-mpv.ps1` fetches FFmpeg `n7.1.1` and mpv `v0.41.0` and builds
both under mingw-w64. Three things about it are deliberate:

**Both are built from source rather than downloaded.** mpv is
GPL-2.0-or-later unless configured otherwise, and every prebuilt FFmpeg for
Windows — including the ones inside libVLC and libmpv — is configured with
`--enable-gpl`. Shipping either inside a noncommercially licensed application
would place the whole distribution under the GPL, overriding its terms
entirely. This passes `-Dgpl=false` to mpv and
`--disable-gpl --disable-nonfree` to FFmpeg, the script refuses to continue if
the configuration disagrees, and `build.ps1` re-reads both finished DLLs before
packaging them.

**It is one library, with no plugin modules.** `-Dlibmpv=true` and no CLI
player: nothing is loaded from a scripts directory, a config file or anywhere
else outside the installation folder.

**Only the encode side of FFmpeg is dropped** — programs, docs, avdevice. Every
decoder, demuxer, parser, protocol and hardware accelerator is included, because
a client that cannot open a recording is worse than one that is larger.

## Rebuilding

| Situation | Command |
|---|---|
| Changed Rust code | `.\build.ps1 -Target App` |
| Want the installer | `.\build.ps1` |
| Changed mpv's or FFmpeg's configuration | `scripts\build-mpv.ps1 -Clean` |

`build.ps1` also accepts `-Target Stage` to assemble `dist\Clicker` without
compiling an installer, which is the fastest way to test a real layout.

## Running from the build tree

`dist\Clicker` is a complete, runnable copy. To run `target\release\clicker.exe`
directly instead, libmpv and its libraries have to be findable:

```powershell
$env:PATH = "$PWD\third_party\mpv;$env:PATH"
.\target\release\clicker.exe
```

Clicker also looks in `third_party\mpv` itself, so the `PATH` line only matters
for the libraries mpv in turn depends on.

It logs its mpv and FFmpeg versions on startup, and shows them in Settings
under About — both read out of the libraries, not claimed in a config file.

## Privacy of the output

Build paths and your username are kept out of the shipped binaries:

- FFmpeg is configured with a neutral `--prefix=/clicker` (the real
  destination goes to `make install`), because FFmpeg records its entire
  configure line inside the binary where anyone can read it
- `RUSTFLAGS=--remap-path-prefix` rewrites the cargo registry, the repository
  root and the user profile
- `build.ps1` greps the finished binaries for your profile name and warns if
  any of that failed

## Reproducing on a clean machine

```powershell
git clone <repo> Clicker
cd Clicker
.\bootstrap.ps1 -Install
.\build.ps1
```

The mpv and FFmpeg source and build trees are gitignored; `bootstrap.ps1`
fetches them from their pinned tags.
