# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial client for Channels DVR Server
# Copyright (c) 2026 David Brustein

<#
.SYNOPSIS
    Build Clicker and produce a Windows installer.

.DESCRIPTION
    Three stages: compile the application, stage it with its runtime into
    dist\Clicker, and compile that into an installer with Inno Setup.

    One binary, Windows 10 1809 and up. The interface used to be drawn over
    Mica, which is Windows 11 only; it paints its own material now, so one
    build covers both.

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
$Stage  = Join-Path $Root 'dist\Clicker'
$OutDir = Join-Path $Root 'dist'

function Fail($message) {
    Write-Host ''
    Write-Host "  $message" -ForegroundColor Red
    Write-Host ''
    exit 1
}

# The processor a PE file was built for, read out of its header: 'x64',
# 'arm64', or the raw value for anything else. Offset 0x3C holds the offset of
# the PE signature, and the machine field is the two bytes after it.
#
# This is how the build learns its own architecture. Nothing passes it in,
# because anything that passed it in could be wrong: cargo builds for the
# machine it is running on, and the answer is already sitting in the file it
# produced.
function Get-PeMachine($path) {
    $stream = [IO.File]::OpenRead($path)
    try {
        $buffer = [byte[]]::new(4)
        $stream.Position = 0x3C
        if ($stream.Read($buffer, 0, 4) -ne 4) { return 'unreadable' }
        $stream.Position = [BitConverter]::ToInt32($buffer, 0) + 4
        if ($stream.Read($buffer, 0, 2) -ne 2) { return 'unreadable' }
    } finally {
        $stream.Dispose()
    }
    switch ([BitConverter]::ToUInt16($buffer, 0)) {
        0x8664  { 'x64' }
        0xAA64  { 'arm64' }
        default { '0x{0:X4}' -f $_ }
    }
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
        # Discard this crate's own artifacts, and only this crate's: the
        # version is compiled in twice, into a Win32 resource and into
        # CARGO_PKG_VERSION, and a cached binary from the previous version has
        # been observed surviving the change. Dependencies are untouched, so
        # this costs one crate's compile rather than a full rebuild.
        Push-Location $Root
        try { cargo clean --release -p clicker 2>&1 | Out-Null } finally { Pop-Location }
    }
} else {
    $Version = $current
}

# ------------------------------------------------------------------ [1/3] ----
Write-Host "[1/3] Building clicker $Version (release)" -ForegroundColor Cyan

# Keep the machine's directory layout out of the binary. rustc records the path
# of every source file it compiles, including the crate registry under the
# user's profile, and those strings survive into the executable where anyone
# with a copy can read them. Remapping rewrites them to neutral names.
#
# `strip = true` in Cargo.toml removes the symbol table; this covers the paths
# that are baked into panic messages rather than into symbols.
$remaps = @(
    "--remap-path-prefix=$env:USERPROFILE\.cargo=/cargo"
    "--remap-path-prefix=$Root=/clicker"
    "--remap-path-prefix=$env:USERPROFILE=/home"
)
$env:RUSTFLAGS = ($remaps -join ' ')

Push-Location $Root
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { Fail 'Build failed.' }
} finally {
    Pop-Location
}

if ($Target -eq 'App') { Write-Host ''; Write-Host '  Done.' -ForegroundColor Green; exit 0 }

# ------------------------------------------------------------------ [2/3] ----
Write-Host '[2/3] Staging into dist\Clicker' -ForegroundColor Cyan

# mpv and the libraries it was linked against, FFmpeg among them. They sit
# beside the executable, unmodified and separately replaceable. That is not
# incidental: LGPL-2.1 section 6 requires that whoever receives this be able to
# substitute their own build of them, which is only true if they are shipped as
# ordinary DLLs rather than folded into the binary.
$mpv = Join-Path $Root 'third_party\mpv'
$mpvDlls = @(Get-ChildItem (Join-Path $mpv '*.dll') -ErrorAction SilentlyContinue)
if (-not $mpvDlls) {
    Fail @"
libmpv was not found in $mpv

  scripts\build-mpv.ps1

It is the player. There is no second one to fall back to, so this refuses to
package an application that would install and then be unable to play anything.
"@
}

# A clean stage every time. Leftovers from a previous run are how a file that is
# no longer shipped ends up inside an installer anyway.
if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
New-Item -ItemType Directory -Force -Path $Stage | Out-Null

$exe = Join-Path $Root 'target\release\clicker.exe'
if (-not (Test-Path $exe)) { Fail 'clicker.exe was not produced.' }
try {
    Copy-Item $exe $Stage -Force
} catch {
    Fail 'Could not stage clicker.exe. Is the app still running?'
}
Copy-Item (Join-Path $Root 'LICENSE.md'), (Join-Path $Root 'NOTICE.md') $Stage -Force

# The icon travels with the app so shortcuts and the uninstall entry can name
# it directly rather than relying on the embedded resource surviving.
$icon = Join-Path $Root 'assets\clicker.ico'
if (Test-Path $icon) {
    Copy-Item $icon $Stage -Force
} else {
    Write-Host '      WARNING: assets\clicker.ico missing; run scripts\make-icon.ps1' -ForegroundColor Yellow
}

# The LGPL text and the corresponding-source record travel with every copy.
# Section 6 requires the license text; pointing at where the FFmpeg source
# lives is what makes the "unmodified, replaceable" claim checkable.
$licenses = Join-Path $Root 'licenses'
if (Test-Path $licenses) {
    Copy-Item (Join-Path $licenses '*') (New-Item -ItemType Directory -Force -Path (Join-Path $Stage 'licenses')).FullName -Force
}

Copy-Item $mpvDlls.FullName $Stage -Force
Write-Host ("      libmpv and its libraries staged: {0} files" -f $mpvDlls.Count) -ForegroundColor DarkGray

# Everything in the stage is for the same processor as the executable beside
# it, read out of the PE headers rather than taken on trust from the script
# that produced them.
#
# Nothing earlier can catch this. libmpv is loaded by name at runtime, so the
# compile never sees it; third_party\mpv holds one architecture at a time with
# nothing in the path saying which; and the license check below is happy
# either way, because an x64 libmpv is just as LGPL as an arm64 one. The
# result would install, launch, find no player it can load, and play nothing.
$AppArch = Get-PeMachine $exe
if ($AppArch -notin @('x64', 'arm64')) {
    Fail "clicker.exe reports an unrecognized machine type ($AppArch)."
}

$wrongArch = @(Get-ChildItem (Join-Path $Stage '*.dll') |
    Where-Object { (Get-PeMachine $_.FullName) -ne $AppArch })
if ($wrongArch) {
    $names = ($wrongArch | ForEach-Object {
        '{0} ({1})' -f $_.Name, (Get-PeMachine $_.FullName)
    }) -join "`n  "
    Fail @"
REFUSING TO PACKAGE: clicker.exe is $AppArch and these are not.

  $names

third_party\mpv holds a libmpv built for another processor. Rebuild it:

  scripts\build-mpv.ps1 -Arch $AppArch
"@
}
Write-Host "      architecture verified: $AppArch throughout" -ForegroundColor DarkGray

# Licenses are read out of the binaries that are actually being shipped, not
# taken on trust from the build scripts that produced them. Clicker is MIT and
# its media components are LGPL; a GPL library in the stage would silently
# place the whole distribution under the GPL rather than the licenses written
# on it, so a stage containing one is refused.
foreach ($name in 'avutil-59.dll', 'libmpv-2.dll') {
    $file = Join-Path $Stage $name
    if (-not (Test-Path $file)) { Fail "$name is missing from the stage." }
    $text = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($file))
    if ($text -match '--enable-gpl' -or $text -match 'GPL version 2 or later') {
        Fail @"
REFUSING TO PACKAGE: $name reports GPL.

Rebuild it. scripts\build-mpv.ps1 configures FFmpeg --disable-gpl
--disable-nonfree and mpv -Dgpl=false.
"@
    }
}
Write-Host '      licenses verified: LGPL, no GPL components' -ForegroundColor DarkGray

# Nothing shipped should name the person who built it.
#
# The test is the user profile directory specifically, not "any absolute path".
# An earlier version flagged anything containing the repository's own path,
# which fired on remapped paths like "/home\Desktop\..." that carry no personal
# information at all — a warning that cried wolf on its own fix.
$profileName = Split-Path -Leaf $env:USERPROFILE
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
} else {
    Write-Host '      no personal paths embedded' -ForegroundColor DarkGray
}

$size = (Get-ChildItem $Stage -Recurse -File | Measure-Object Length -Sum).Sum / 1MB
Write-Host ('      staged {0:N1} MB' -f $size)

if ($Target -eq 'Stage') {
    Write-Host ''
    Write-Host '  Done.' -ForegroundColor Green
    Write-Host "    app  $Stage\clicker.exe"
    exit 0
}

# ------------------------------------------------------------------ [3/3] ----
Write-Host "[3/3] Building the $AppArch installer" -ForegroundColor Cyan

$iscc = @(
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $iscc) {
    Fail @"
Inno Setup 6 was not found.

The staged application is in dist\Clicker and can be run from there.
"@
}

& $iscc /Qp "/O$OutDir" "/DAppVersion=$Version" "/DAppArch=$AppArch" (Join-Path $Root 'installer\clicker.iss')
if ($LASTEXITCODE -ne 0) { Fail 'The installer failed to build.' }

Write-Host ''
Write-Host '  Done.' -ForegroundColor Green
Write-Host "    app        $Stage\clicker.exe"
Get-ChildItem (Join-Path $OutDir 'Clicker-Setup-*.exe') -ErrorAction SilentlyContinue |
    ForEach-Object { Write-Host "    installer  $($_.FullName)" }
Write-Host ''
