# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial client for Channels DVR Server
# Copyright (c) 2026 David Brustein

<#
.SYNOPSIS
    Build libmpv from source, LGPL only, for Clicker to load at runtime.

.DESCRIPTION
    Three stages: build FFmpeg for mingw, build mpv against it, and stage the
    result into third_party\mpv.

    Why a second toolchain. mpv cannot be built with MSVC; upstream supports
    mingw-w64 and nothing else on Windows. Consuming the result from MSVC is
    fine, which is why the application loads the DLL by name at runtime rather
    than linking an import library. So this script drives an MSYS2 shell, and
    the rest of the repository carries on building with MSVC exactly as before.

    Why from source. The prebuilt LGPL packages that exist are built by someone
    who states plainly that they cannot guarantee every LGPL-incompatible
    component is disabled, and their FFmpeg has been stripped of the strings
    that would let anyone check. This project's license position is that the
    license has to be provable from the binary being shipped, which means
    building it here, from a pinned tag, with the flags written down.

    FFmpeg is built again rather than reusing third_party\ffmpeg: that one is
    MSVC and this one has to be gcc. It uses the same pinned source and the
    same licensing flags, and the libraries stay separate replaceable DLLs
    beside libmpv rather than being folded into it, which is what LGPL-2.1
    section 6 asks for.

    Two architectures, one script. x64 builds under MINGW64 with gcc, arm64
    under CLANGARM64 with clang, and everything between those two facts is the
    same source at the same tags with the same licensing flags. MSYS2 has no
    aarch64 port of its own runtime, so on an ARM machine bash, pacman and
    configure all run under x64 emulation while the compiler they drive is
    native and its output is native. Slow to configure, correct to ship.

.PARAMETER Arch
    x64    the MINGW64 environment, gcc, FFmpeg --arch=x86_64
    arm64  the CLANGARM64 environment, clang, FFmpeg --arch=aarch64

    Defaults to the architecture of the machine it is run on. There is no
    cross-compiling here: an aarch64 clang cannot run on an x64 host at all,
    and while an ARM machine can build the x64 stage under emulation, it has
    no reason to.

.PARAMETER Clean
    Throw away both build trees and start over.

.PARAMETER Reconfigure
    Re-run configure and meson setup without discarding the source.

.EXAMPLE
    scripts\build-mpv.ps1
    scripts\build-mpv.ps1 -Arch arm64
    scripts\build-mpv.ps1 -Reconfigure
#>

[CmdletBinding()]
param(
    [ValidateSet('x64', 'arm64')]
    [string]$Arch = $(
        if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'Arm64') {
            'arm64'
        } else {
            'x64'
        }
    ),
    [switch]$Clean,
    [switch]$Reconfigure,
    [string]$Msys = 'C:\msys64'
)

$ErrorActionPreference = 'Stop'

$Root      = Split-Path -Parent $PSScriptRoot
# Its own FFmpeg checkout, deliberately, and one per architecture. The first
# reason is the toolchain: third_party\ffmpeg-src is configured for MSVC and
# carries two patches for MSVC response files, and configuring the same tree
# for gcc leaves it in a state where the next build-ffmpeg.ps1 run skips
# reconfiguring and builds the application against a mingw config.
#
# The second is the same trap one level down. FFmpeg builds in-tree, so an x64
# and an arm64 build cannot share a checkout: the second one would find the
# first one's config.mak, decide it had nothing to configure, and link objects
# of the wrong machine type. mpv builds out of tree, so it needs only its own
# build directory and the source is shared.
$FFmpegSrc = Join-Path $Root "third_party\ffmpeg-mingw-src-$Arch"
$MpvSrc    = Join-Path $Root 'third_party\mpv-src'
$MpvBuild  = Join-Path $MpvSrc "build-$Arch"
$Deps      = Join-Path $Root "third_party\mpv-deps-$Arch"
# The stage is not per-architecture: build.ps1 packages whatever is in here,
# and it re-reads the machine type of every file before it does, so a stage
# left over from the other architecture is refused rather than shipped.
$Stage     = Join-Path $Root 'third_party\mpv'

# Everything that differs between the two, in one place. MSYS2 names its
# environments by the toolchain rather than the target, so CLANGARM64 is both
# "aarch64" and "clang" at once: a different prefix on disk, a different
# package namespace, and a different compiler behind the same package names.
$Toolchain = @{
    x64 = @{
        MSystem = 'MINGW64'
        Prefix  = 'mingw64'
        Package = 'mingw-w64-x86_64'
        CC      = 'gcc'
        CXX     = 'g++'
        ObjDump = 'objdump'
        FFArch  = 'x86_64'
        # nasm assembles FFmpeg's x86 SIMD. There is nothing to install in its
        # place for aarch64: clang assembles FFmpeg's NEON with the integrated
        # assembler it already has.
        Extra   = @('nasm')
    }
    arm64 = @{
        MSystem = 'CLANGARM64'
        Prefix  = 'clangarm64'
        Package = 'mingw-w64-clang-aarch64'
        CC      = 'clang'
        CXX     = 'clang++'
        # binutils is not part of a clang toolchain; llvm-objdump comes with
        # clang itself and reads a PE import table the same way.
        ObjDump = 'llvm-objdump'
        FFArch  = 'aarch64'
        Extra   = @()
    }
}[$Arch]

$FFmpegTag = 'n7.1.1'

# The tag mpv is pinned to. Moving this is a deliberate act: read mpv's release
# notes first, because the render API is the one part of it this depends on.
$MpvTag = 'v0.41.0'

function Step($message) {
    Write-Host ''
    Write-Host "  $message" -ForegroundColor Cyan
}

function Fail($message) {
    Write-Host ''
    Write-Host "  $message" -ForegroundColor Red
    Write-Host ''
    exit 1
}

$Bash = Join-Path $Msys 'usr\bin\bash.exe'
if (-not (Test-Path $Bash)) {
    Fail @"
MSYS2 was not found at $Msys

  winget install --id MSYS2.MSYS2

Then run this again. Pass -Msys if it lives somewhere else.
"@
}

# A path Windows understands, as one MSYS2 understands.
function Unix($path) {
    $full = (Resolve-Path -LiteralPath $path -ErrorAction SilentlyContinue)?.Path
    if (-not $full) { $full = $path }
    '/' + $full.Substring(0, 1).ToLower() + $full.Substring(2).Replace('\', '/')
}

# Everything runs in the target's own environment, never the MSYS one: the
# difference is which compiler is on PATH, and the MSYS compiler produces
# binaries that depend on msys-2.0.dll, which is both a Cygwin runtime and GPL.
#
# CC and CXX are exported rather than left to be detected. FFmpeg is told its
# compiler on the configure line, but meson takes it from the environment, and
# on CLANGARM64 there is no `cc` and no `gcc` unless the virtual package
# happens to be installed. Naming it here means neither build has to guess.
function Invoke-Bash($command) {
    $prelude = "export MSYSTEM=$($Toolchain.MSystem); " +
               "export PATH=/$($Toolchain.Prefix)/bin:`$PATH; " +
               "export CC=$($Toolchain.CC); export CXX=$($Toolchain.CXX); set -e; "
    & $Bash -lc "$prelude$command"
    if ($LASTEXITCODE -ne 0) { Fail "Failed: $command" }
}

if (-not (Test-Path (Join-Path $FFmpegSrc 'configure'))) {
    Step "fetching FFmpeg $FFmpegTag for $Arch"
    git clone --depth 1 --branch $FFmpegTag https://github.com/FFmpeg/FFmpeg.git $FFmpegSrc
    if ($LASTEXITCODE -ne 0) { Fail 'Could not clone FFmpeg.' }
}

if ($Clean) {
    Step 'cleaning'
    Remove-Item -Recurse -Force $Deps, $Stage, $MpvBuild -ErrorAction SilentlyContinue
    Invoke-Bash "cd '$(Unix $FFmpegSrc)' && make distclean >/dev/null 2>&1 || true"
}

# ------------------------------------------------------------------ mpv src ---
if (-not (Test-Path (Join-Path $MpvSrc 'meson.build'))) {
    Step "fetching mpv $MpvTag"
    # core.autocrlf=false, and it matters for more than tidiness. Windows git
    # checks out CRLF by default; the mingw git that mpv's own version script
    # runs under compares against LF, sees all 820 files as modified, and
    # stamps "-dirty" into the version string embedded in the library. The
    # source is the pinned tag either way, but the About panel would be saying
    # otherwise.
    git -c core.autocrlf=false clone --depth 1 --branch $MpvTag `
        https://github.com/mpv-player/mpv.git $MpvSrc
    if ($LASTEXITCODE -ne 0) { Fail 'Could not clone mpv.' }
    git -C $MpvSrc config core.autocrlf false
}

# An existing tree checked out before that was fixed. Line endings only: the
# content is the tag's, so re-checking it out costs nothing and stops the
# library claiming to be built from a modified source tree.
if ((git -C $MpvSrc config core.autocrlf) -ne 'false') {
    Step 'normalizing line endings in the mpv checkout'
    git -C $MpvSrc config core.autocrlf false
    git -C $MpvSrc rm --cached -r -q . | Out-Null
    git -C $MpvSrc reset --hard -q
    if ($LASTEXITCODE -ne 0) { Fail 'Could not normalize the mpv checkout.' }
}

# ------------------------------------------------------------------- FFmpeg ---
#
# The same licensing flags as scripts\build-ffmpeg.ps1, for the same reasons.
# The prefix is neutral because FFmpeg bakes its entire configure line into the
# binary, where anyone with a copy can read it; the real destination is given
# to `make install` instead, where it is not recorded.
$FFmpegBuilt = Join-Path $Deps 'clicker\lib\pkgconfig\libavcodec.pc'
if ($Reconfigure -or -not (Test-Path $FFmpegBuilt)) {
    Step "configuring FFmpeg for $Arch (LGPL, decode only)"
    $options = @(
        '--prefix=/clicker'
        '--target-os=mingw32'
        "--arch=$($Toolchain.FFArch)"
        # Named rather than left to the default, which is gcc. On CLANGARM64
        # there may be no gcc at all, and a configure that falls back to one
        # that does exist would build for the wrong machine.
        "--cc=$($Toolchain.CC)"
        '--enable-shared'
        '--disable-static'
        # The license position of this entire application rests on these two.
        '--disable-gpl'
        '--disable-nonfree'
        # Nothing gets picked up off this machine and silently becomes a
        # dependency or a license problem.
        '--disable-autodetect'
        '--enable-schannel'
        '--enable-d3d11va'
        '--enable-dxva2'
        '--disable-programs'
        '--disable-doc'
        '--disable-avdevice'
        '--disable-debug'
    ) -join ' '

    Invoke-Bash "cd '$(Unix $FFmpegSrc)' && ./configure $options"
    Step 'building FFmpeg'
    Invoke-Bash "cd '$(Unix $FFmpegSrc)' && make -j$([Environment]::ProcessorCount) && make install DESTDIR='$(Unix $Deps)'"
}

# ---------------------------------------------------------------------- mpv ---
#
# -Dgpl=false is the whole point: it is what makes libmpv LGPLv2.1+ and usable
# by an application that is not GPL. It disables features whose copyright
# holders could not all be reached for relicensing, nearly all of which are
# Linux and BSD specific and none of which exist on Windows anyway.
#
# libass and libplacebo are not optional in mpv 0.41 — both are plain
# `dependency()` calls in its meson.build with no `required: false`, so there
# is no flag to turn them off. Both are license-compatible: libass is ISC and
# libplacebo is LGPLv2.1+. They come from MSYS2 packages along with freetype,
# fribidi and harfbuzz, and they ship as DLLs beside libmpv.
$BuildName = Split-Path -Leaf $MpvBuild
if ($Reconfigure -or -not (Test-Path (Join-Path $MpvBuild 'build.ninja'))) {
    Step "configuring mpv $MpvTag for $Arch (LGPL, libmpv only)"
    $pkg = Unix (Join-Path $Deps 'clicker\lib\pkgconfig')
    $setup = @(
        "meson setup $BuildName"
        '--buildtype=release'
        '-Dgpl=false'
        '-Dlibmpv=true'
        '-Dcplayer=false'
        '-Ddefault_library=shared'
        '-Dlua=disabled'
        '-Djavascript=disabled'
        '-Dmanpage-build=disabled'
        '-Dtests=false'
    ) -join ' '
    # PKG_CONFIG_PATH is prepended, not replaced: mpv needs to find our FFmpeg
    # first and libass and libplacebo from the MSYS2 packages after it.
    $sys = "/$($Toolchain.Prefix)/lib/pkgconfig"
    Invoke-Bash "cd '$(Unix $MpvSrc)' && export PKG_CONFIG_PATH='${pkg}:${sys}' && rm -rf $BuildName && $setup"
}

Step "building mpv for $Arch"
Invoke-Bash "cd '$(Unix $MpvSrc)' && ninja -C $BuildName"

# -------------------------------------------------------------------- stage ---
Step "staging $Arch into third_party\mpv"
# Emptied first, not merged into. The stage is shared between the two
# architectures, and the runtime libraries are not the same set on both: gcc
# leaves libgcc_s_seh-1.dll and libstdc++-6.dll behind, clang leaves libunwind
# and libc++. Copying over the top would keep whichever of those the other
# toolchain left, and they are exactly the files nothing would overwrite.
if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Stage 'include\mpv') | Out-Null

Get-ChildItem $MpvBuild -Filter '*mpv*.dll' -Recurse |
    ForEach-Object { Copy-Item $_.FullName $Stage -Force }
Copy-Item (Join-Path $MpvSrc 'include\mpv\*.h') (Join-Path $Stage 'include\mpv') -Force

# FFmpeg beside it, unmodified and individually replaceable, which is what
# LGPL-2.1 section 6 requires and the reason they are not statically folded
# into libmpv.
Get-ChildItem (Join-Path $Deps 'clicker\bin\*.dll') -ErrorAction SilentlyContinue |
    ForEach-Object { Copy-Item $_.FullName $Stage -Force }

# Everything else libmpv was linked against, resolved rather than listed.
#
# There are two dozen of these — libass and libplacebo, their font and shader
# stacks, and the compiler runtime — and the set changes with the packages
# MSYS2 ships. A hand-written list would be wrong the first time someone
# updated a package, and wrong in the worst way: the DLL loads on the machine
# that built it and fails on every other one.
#
# Read out of the import tables, not asked of the loader. `ldd` answers by
# loading the library and reporting what came with it, which cannot work when
# the shell is an emulated x64 one and the libraries are aarch64: it returned
# seven of the twenty-six and said nothing about the rest, so an arm64
# installer shipped without libass and libmpv would not load on any machine.
# An import table is a structure in a file. Reading it does not care what
# processor either side is for.
#
# Recursive, because a direct dependency has dependencies of its own, and
# whether a name is ours is decided by whether the toolchain ships it: what is
# in the environment's bin directory travels with us, what is in System32 is
# Windows' own and must not, and a name that is in neither stops the build.
Step 'resolving dependencies'
$prefix = $Toolchain.Prefix
$binDir = Join-Path $Msys "$prefix\bin"
$system = Join-Path $env:SystemRoot 'System32'

$queue = [System.Collections.Generic.Queue[string]]::new()
$seen = @{}
foreach ($file in Get-ChildItem (Join-Path $Stage '*.dll')) {
    $queue.Enqueue($file.FullName)
    $seen[$file.Name.ToLower()] = $true
}

$added = 0
$missing = @()
while ($queue.Count -gt 0) {
    $file = $queue.Dequeue()
    $imports = & $Bash -lc "export PATH=/$prefix/bin:`$PATH; $($Toolchain.ObjDump) -p '$(Unix $file)' | sed -n 's/.*DLL Name: *//p'"
    if ($LASTEXITCODE -ne 0) { Fail "Could not read the imports of $file" }

    foreach ($name in $imports) {
        $name = $name.Trim()
        if (-not $name -or $seen.ContainsKey($name.ToLower())) { continue }
        $seen[$name.ToLower()] = $true

        # Windows first, and the order matters. vulkan-1.dll is in System32
        # because the graphics driver put it there, and it is also in the
        # toolchain's bin because a package brought a copy; carrying that copy
        # would put a loader beside the executable that wins over the driver's
        # own. Anything Windows provides is Windows' to provide.
        #
        # The api-ms-win- and ext-ms-win- names are matched rather than looked
        # for. They are API set contracts, resolved by the loader through a
        # schema, and no file of that name exists anywhere: looking for one
        # and failing the build over it would reject a perfectly good stage.
        $source = Join-Path $binDir $name
        if ($name -like 'api-ms-win-*' -or $name -like 'ext-ms-win-*') {
            continue
        } elseif (Test-Path (Join-Path $system $name)) {
            continue
        } elseif (Test-Path $source) {
            Copy-Item $source $Stage -Force
            $queue.Enqueue((Join-Path $Stage $name))
            $added++
        } else {
            # Neither ours nor Windows'. Shipping this would be an installer
            # that fails on every machine but the one that built it.
            $missing += "$name (imported by $(Split-Path -Leaf $file))"
        }
    }
}

if ($missing) {
    Fail @"
REFUSING: these were imported and could not be found.

  $($missing -join "`n  ")

They are in neither $binDir nor System32, so the staged libraries would not
load anywhere. Check the packages this environment has installed.
"@
}
Write-Host "      $added libraries carried in, resolved from the import tables" -ForegroundColor DarkGray

# ------------------------------------------------------------------- verify ---
#
# The license is read out of the binaries actually produced, not taken on trust
# from the flags above. Same check build.ps1 runs before packaging.
Step 'verifying'
$bad = @()
foreach ($file in Get-ChildItem $Stage -Filter *.dll) {
    $text = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($file.FullName))
    if ($text -match '--enable-gpl' -or $text -match 'GPL version 2 or later') {
        $bad += $file.Name
    }
}
if ($bad) {
    Fail "REFUSING: $($bad -join ', ') report GPL. This build may not be distributed with Clicker."
}

$size = (Get-ChildItem $Stage -Recurse -File | Measure-Object Length -Sum).Sum / 1MB
Write-Host ''
Write-Host '  Done.' -ForegroundColor Green
Write-Host ('    {0}  ({1}, {2:N1} MB, no GPL components)' -f $Stage, $Arch, $size)
Get-ChildItem $Stage -Filter *.dll | ForEach-Object { Write-Host ('    {0,-28} {1,7:N1} MB' -f $_.Name, ($_.Length / 1MB)) }
Write-Host ''
