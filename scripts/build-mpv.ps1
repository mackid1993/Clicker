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

.PARAMETER Clean
    Throw away both build trees and start over.

.PARAMETER Reconfigure
    Re-run configure and meson setup without discarding the source.

.EXAMPLE
    scripts\build-mpv.ps1
    scripts\build-mpv.ps1 -Reconfigure
#>

[CmdletBinding()]
param(
    [switch]$Clean,
    [switch]$Reconfigure,
    [string]$Msys = 'C:\msys64'
)

$ErrorActionPreference = 'Stop'

$Root      = Split-Path -Parent $PSScriptRoot
# Its own FFmpeg checkout, deliberately. third_party\ffmpeg-src is configured
# for MSVC and carries two patches for MSVC response files; configuring the
# same tree for gcc leaves it in a state where the next build-ffmpeg.ps1 run
# skips reconfiguring and builds the application against a mingw config.
$FFmpegSrc = Join-Path $Root 'third_party\ffmpeg-mingw-src'
$MpvSrc    = Join-Path $Root 'third_party\mpv-src'
$Deps      = Join-Path $Root 'third_party\mpv-deps'
$Stage     = Join-Path $Root 'third_party\mpv'

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

# Everything runs in the mingw64 environment, not the MSYS one: the difference
# is which gcc is on PATH, and the MSYS gcc produces binaries that depend on
# msys-2.0.dll, which is both a Cygwin runtime and GPL.
function Invoke-Bash($command) {
    & $Bash -lc "export MSYSTEM=MINGW64; export PATH=/mingw64/bin:`$PATH; set -e; $command"
    if ($LASTEXITCODE -ne 0) { Fail "Failed: $command" }
}

if (-not (Test-Path (Join-Path $FFmpegSrc 'configure'))) {
    Step "fetching FFmpeg $FFmpegTag for mingw"
    git clone --depth 1 --branch $FFmpegTag https://github.com/FFmpeg/FFmpeg.git $FFmpegSrc
    if ($LASTEXITCODE -ne 0) { Fail 'Could not clone FFmpeg.' }
}

if ($Clean) {
    Step 'cleaning'
    Remove-Item -Recurse -Force $Deps, $Stage, (Join-Path $MpvSrc 'build') -ErrorAction SilentlyContinue
    Invoke-Bash "cd '$(Unix $FFmpegSrc)' && make distclean >/dev/null 2>&1 || true"
}

# ------------------------------------------------------------------ mpv src ---
if (-not (Test-Path (Join-Path $MpvSrc 'meson.build'))) {
    Step "fetching mpv $MpvTag"
    git clone --depth 1 --branch $MpvTag https://github.com/mpv-player/mpv.git $MpvSrc
    if ($LASTEXITCODE -ne 0) { Fail 'Could not clone mpv.' }
}

# ------------------------------------------------------------------- FFmpeg ---
#
# The same licensing flags as scripts\build-ffmpeg.ps1, for the same reasons.
# The prefix is neutral because FFmpeg bakes its entire configure line into the
# binary, where anyone with a copy can read it; the real destination is given
# to `make install` instead, where it is not recorded.
$FFmpegBuilt = Join-Path $Deps 'clicker\lib\pkgconfig\libavcodec.pc'
if ($Reconfigure -or -not (Test-Path $FFmpegBuilt)) {
    Step 'configuring FFmpeg for mingw (LGPL, decode only)'
    $options = @(
        '--prefix=/clicker'
        '--target-os=mingw32'
        '--arch=x86_64'
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
$MpvBuild = Join-Path $MpvSrc 'build'
if ($Reconfigure -or -not (Test-Path (Join-Path $MpvBuild 'build.ninja'))) {
    Step "configuring mpv $MpvTag (LGPL, libmpv only)"
    $pkg = Unix (Join-Path $Deps 'clicker\lib\pkgconfig')
    $setup = @(
        'meson setup build'
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
    Invoke-Bash "cd '$(Unix $MpvSrc)' && export PKG_CONFIG_PATH='${pkg}:/mingw64/lib/pkgconfig' && rm -rf build && $setup"
}

Step 'building mpv'
Invoke-Bash "cd '$(Unix $MpvSrc)' && ninja -C build"

# -------------------------------------------------------------------- stage ---
Step 'staging into third_party\mpv'
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Stage 'include\mpv') | Out-Null

Get-ChildItem (Join-Path $MpvSrc 'build') -Filter '*mpv*.dll' -Recurse |
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
# stacks, and the gcc runtime — and the set changes with the packages MSYS2
# ships. A hand-written list would be wrong the first time someone updated a
# package, and wrong in the worst way: the DLL loads on the machine that built
# it and fails on every other one. Asking the linker what it actually needs
# cannot drift.
Step 'resolving dependencies'
$deps = & $Bash -lc "export PATH=/mingw64/bin:`$PATH; ldd '$(Unix (Join-Path $Stage 'libmpv-2.dll'))' | grep -i mingw64 | awk '{print `$3}' | sort -u"
foreach ($dep in $deps) {
    if (-not $dep) { continue }
    # /mingw64/bin/foo.dll as MSYS2 reports it, back to a Windows path.
    $windows = Join-Path $Msys ($dep -replace '^/mingw64/', 'mingw64/').Replace('/', '\')
    if (Test-Path $windows) {
        Copy-Item $windows $Stage -Force
    } else {
        Write-Host "      WARNING: $dep was linked but not found" -ForegroundColor Yellow
    }
}

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
Write-Host ('    {0}  ({1:N1} MB, no GPL components)' -f $Stage, $size)
Get-ChildItem $Stage -Filter *.dll | ForEach-Object { Write-Host ('    {0,-28} {1,7:N1} MB' -f $_.Name, ($_.Length / 1MB)) }
Write-Host ''
