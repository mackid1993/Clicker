<#
.SYNOPSIS
    Build RustDVR and RustVCR, and produce a Windows installer for each.

.DESCRIPTION
    Three stages: compile the application, stage it with its runtime under
    dist\, and compile installers with Inno Setup.

    Every run produces two editions from the same source. RustDVR is the
    Windows 11 build, Mica backdrop and all. RustVCR — the VCR to RustDVR's
    DVR — is the same code compiled with the `win10` feature, which swaps the
    transparent, material-backed window base for an opaque one, because
    Windows 10 has no Mica to show through it. The RustDVR installer requires
    Windows 11, where Mica exists; RustVCR installs on Windows 10 or anything
    newer — built for 10, welcome anywhere.

    Nothing is installed on this machine and nothing is written outside the
    repository.

.PARAMETER Target
    App    build the executable only
    Stage  build and stage the runtime, no installer
    All    everything (default)

.PARAMETER Version
    The version to stamp into the executable, the installer, and the
    installer's filename. Written back to Cargo.toml, which is the single
    source of truth: the version resource in the binary comes from
    CARGO_PKG_VERSION, so setting it anywhere else would let the two drift.

    Omit it to build whatever Cargo.toml already says.

.EXAMPLE
    .\build.ps1
    .\build.ps1 -Target Stage
    .\build.ps1 --ver 0.0.1
    .\build.ps1 -Version 1.2.0 -Target Stage
#>

# PositionalBinding is off deliberately. With it on, PowerShell binds the first
# loose argument to the first parameter, so `--ver` was being handed to -Target
# and rejected against its ValidateSet before the script ever ran. Off, anything
# unrecognized falls through to $Rest, which is where --ver is parsed.
[CmdletBinding(PositionalBinding = $false)]
param(
    [ValidateSet('App', 'Stage', 'All')]
    [string]$Target = 'All',

    [string]$Version,

    # PowerShell rejects double-dashed arguments outright, so `--ver 0.0.1`
    # would fail before the script ever ran. Collecting the leftovers and
    # parsing them below is what makes that form work alongside -Version.
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Rest
)

$ErrorActionPreference = 'Stop'

$Root   = $PSScriptRoot
$OutDir = Join-Path $Root 'dist'
$FFmpeg = if ($env:FFMPEG_DIR) { $env:FFMPEG_DIR } else { Join-Path $Root 'third_party\ffmpeg' }

# The two editions, built from the same source on every run. Everything after
# this point loops over the table rather than knowing the names, so a third
# edition — should Windows 12 demand a RustLaserDisc — is one line here and a
# branch in installer\rustdvr.iss.
$Variants = @(
    @{ Name = 'RustDVR'; Bin = 'rustdvr'; Features = '';      IssDefines = @() }
    @{ Name = 'RustVCR'; Bin = 'rustvcr'; Features = 'win10'; IssDefines = @('/DWin10') }
)

function Fail($message) {
    Write-Host ''
    Write-Host "  $message" -ForegroundColor Red
    Write-Host ''
    exit 1
}

# --- version ------------------------------------------------------------------
#
# Accepts `--ver 0.0.1`, `--version 0.0.1`, and the `=` forms, on top of the
# native `-Version 0.0.1`.
for ($i = 0; $i -lt $Rest.Count; $i++) {
    $arg = $Rest[$i]
    if ($arg -match '^--?ver(sion)?=(.+)$') {
        $Version = $Matches[2]
    } elseif ($arg -match '^--?ver(sion)?$') {
        if ($i + 1 -ge $Rest.Count) { Fail "No value given after $arg" }
        $Version = $Rest[++$i]
    } else {
        Fail "Unrecognized argument: $arg"
    }
}

$CargoToml = Join-Path $Root 'Cargo.toml'
# The first `version = "..."` in the file is the package's own; the ones further
# down belong to dependencies and must not be touched, which is why this is a
# single, anchored, count-limited replacement rather than a blanket one.
$VersionLine = [regex]'(?m)^(version\s*=\s*")([^"]+)(")'
$manifest = Get-Content $CargoToml -Raw
$match = $VersionLine.Match($manifest)
if (-not $match.Success) { Fail "No version found in Cargo.toml" }
$current = $match.Groups[2].Value

if ($Version) {
    if ($Version -notmatch '^\d+\.\d+\.\d+$') {
        Fail "Version must look like 1.2.3, not '$Version'"
    }
    if ($Version -ne $current) {
        Set-Content -Path $CargoToml -NoNewline `
            -Value $VersionLine.Replace($manifest, "`${1}$Version`${3}", 1)
        Write-Host "      version $current -> $Version" -ForegroundColor DarkGray
    }
} else {
    $Version = $current
}

# ------------------------------------------------------------------ [1/3] ----
Write-Host "[1/3] Building rustdvr and rustvcr $Version (release)" -ForegroundColor Cyan

if (-not (Test-Path (Join-Path $FFmpeg 'include'))) {
    Fail @"
FFmpeg not found at $FFmpeg

  scripts\build-ffmpeg.ps1

Or set FFMPEG_DIR to an existing LGPL build.
"@
}
$env:FFMPEG_DIR = $FFmpeg

# Keep the machine's directory layout out of the binary. rustc records the path
# of every source file it compiles, including the crate registry under the
# user's profile, and those strings survive into the executable where anyone
# with a copy can read them. Remapping rewrites them to neutral names.
#
# `strip = true` in Cargo.toml removes the symbol table; this covers the paths
# that are baked into panic messages rather than into symbols.
$remaps = @(
    "--remap-path-prefix=$env:USERPROFILE\.cargo=/cargo"
    "--remap-path-prefix=$Root=/rustdvr"
    "--remap-path-prefix=$env:USERPROFILE=/home"
)
$env:RUSTFLAGS = ($remaps -join ' ')

# One invocation per edition rather than one for both. The feature set is what
# distinguishes them, and cargo applies features to everything in an
# invocation — asking for both binaries at once would build them identically.
# Dependencies are compiled once and shared; only the crate itself and its
# build script rerun for the second edition.
Push-Location $Root
try {
    foreach ($v in $Variants) {
        $flags = @('--release', '--bin', $v.Bin)
        if ($v.Features) { $flags += @('--features', $v.Features) }
        cargo build @flags
        if ($LASTEXITCODE -ne 0) { Fail "Build failed ($($v.Name))." }
    }
} finally {
    Pop-Location
}

if ($Target -eq 'App') { Write-Host ''; Write-Host '  Done.' -ForegroundColor Green; exit 0 }

# ------------------------------------------------------------------ [2/3] ----
Write-Host '[2/3] Staging into dist' -ForegroundColor Cyan

# The FFmpeg libraries sit beside the executable, unmodified and separately
# replaceable. That is not incidental: LGPL-2.1 section 6 requires that whoever
# receives this be able to substitute their own build of them, which is only
# true if they are shipped as ordinary DLLs rather than folded into the binary.
$dlls = Get-ChildItem (Join-Path $FFmpeg 'bin\*.dll') -ErrorAction SilentlyContinue
if (-not $dlls) { Fail "No FFmpeg DLLs found in $FFmpeg\bin" }

# The licence is read out of the binary that is actually being shipped, not
# taken on trust from the build script that produced it. Both editions ship
# the same DLLs, so one check covers them.
$avutil = $dlls | Where-Object { $_.Name -like 'avutil*' } | Select-Object -First 1
if ($avutil) {
    $text = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($avutil.FullName))
    if ($text -match '--enable-gpl' -or $text -match 'GPL version 2 or later') {
        Fail @"
REFUSING TO PACKAGE: $($avutil.Name) reports GPL.

This application is distributed under PolyForm Noncommercial, which is
incompatible with the GPL. Rebuild FFmpeg with scripts\build-ffmpeg.ps1, which configures
--disable-gpl --disable-nonfree.
"@
    }
    Write-Host '      FFmpeg licence verified: LGPL, no GPL components' -ForegroundColor DarkGray
}

$icon     = Join-Path $Root 'assets\rustdvr.ico'
$licenses = Join-Path $Root 'licenses'
$profileName = Split-Path -Leaf $env:USERPROFILE

foreach ($v in $Variants) {
    $Stage = Join-Path $OutDir $v.Name

    # A clean stage every time. Leftovers from a previous run are how a file
    # that is no longer shipped ends up inside an installer anyway.
    if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
    New-Item -ItemType Directory -Force -Path $Stage | Out-Null

    $exe = Join-Path $Root "target\release\$($v.Bin).exe"
    if (-not (Test-Path $exe)) { Fail "$($v.Bin).exe was not produced." }
    try {
        Copy-Item $exe $Stage -Force
    } catch {
        Fail "Could not stage $($v.Bin).exe. Is the app still running?"
    }
    Copy-Item (Join-Path $Root 'LICENSE.md') $Stage -Force

    # The icon travels with the app so shortcuts and the uninstall entry can
    # name it directly rather than relying on the embedded resource surviving.
    # Both editions carry it under the same filename; the installer stages by
    # name, not by product.
    if (Test-Path $icon) {
        Copy-Item $icon $Stage -Force
    } else {
        Write-Host '      WARNING: assets\rustdvr.ico missing; run scripts\make-icon.ps1' -ForegroundColor Yellow
    }

    # The LGPL text and the corresponding-source record travel with every copy.
    # Section 6 requires the license text; pointing at where the FFmpeg source
    # lives is what makes the "unmodified, replaceable" claim checkable.
    if (Test-Path $licenses) {
        Copy-Item (Join-Path $licenses '*') (New-Item -ItemType Directory -Force -Path (Join-Path $Stage 'licenses')).FullName -Force
    }

    Copy-Item $dlls.FullName $Stage -Force

    # Nothing shipped should name the person who built it.
    #
    # The test is the user profile directory specifically, not "any absolute
    # path". An earlier version flagged anything containing \Desktop\RustDVR,
    # which fired on remapped paths like "/home\Desktop\RustDVR\target\..."
    # that carry no personal information at all — a warning that cried wolf on
    # its own fix.
    $leaks = @()
    foreach ($file in Get-ChildItem $Stage -File -Include *.exe, *.dll -Recurse) {
        $text = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($file.FullName))
        if ($text -match [regex]::Escape("Users\$profileName") -or
            $text -match [regex]::Escape("Users/$profileName")) {
            $leaks += $file.Name
        }
    }
    if ($leaks) {
        Write-Host "      WARNING: '$profileName' appears in $($leaks -join ', ')" -ForegroundColor Yellow
    }

    $size = (Get-ChildItem $Stage -Recurse -File | Measure-Object Length -Sum).Sum / 1MB
    Write-Host ('      {0}: staged {1:N1} MB' -f $v.Name, $size)
}

if ($Target -eq 'Stage') {
    Write-Host ''
    Write-Host '  Done.' -ForegroundColor Green
    foreach ($v in $Variants) {
        Write-Host "    app  $(Join-Path $OutDir $v.Name)\$($v.Bin).exe"
    }
    exit 0
}

# ------------------------------------------------------------------ [3/3] ----
Write-Host '[3/3] Building the installers' -ForegroundColor Cyan

$iscc = @(
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $iscc) {
    Fail @"
Inno Setup 6 was not found.

The staged applications are in dist\ and can be run from there.
"@
}

# One script, compiled once per edition. /DWin10 is what turns it into the
# RustVCR installer; see installer\rustdvr.iss.
$iss = Join-Path $Root 'installer\rustdvr.iss'
foreach ($v in $Variants) {
    & $iscc /Qp "/O$OutDir" "/DAppVersion=$Version" @($v.IssDefines) $iss
    if ($LASTEXITCODE -ne 0) { Fail "The $($v.Name) installer failed to build." }
}

Write-Host ''
Write-Host '  Done.' -ForegroundColor Green
foreach ($v in $Variants) {
    Write-Host "    app        $(Join-Path $OutDir $v.Name)\$($v.Bin).exe"
}
Get-ChildItem (Join-Path $OutDir 'Rust*-Setup-*.exe') -ErrorAction SilentlyContinue |
    ForEach-Object { Write-Host "    installer  $($_.FullName)" }
Write-Host ''
