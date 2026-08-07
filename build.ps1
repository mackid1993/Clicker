<#
.SYNOPSIS
    Build RustDVR and produce a Windows installer.

.DESCRIPTION
    Three stages: compile the application, stage it with its runtime into
    dist\RustDVR, and compile that into an installer with Inno Setup.

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
$Stage  = Join-Path $Root 'dist\RustDVR'
$OutDir = Join-Path $Root 'dist'
$FFmpeg = if ($env:FFMPEG_DIR) { $env:FFMPEG_DIR } else { Join-Path $Root 'third_party\ffmpeg' }

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
Write-Host "[1/3] Building rustdvr $Version (release)" -ForegroundColor Cyan

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

Push-Location $Root
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { Fail 'Build failed.' }
} finally {
    Pop-Location
}

if ($Target -eq 'App') { Write-Host ''; Write-Host '  Done.' -ForegroundColor Green; exit 0 }

# ------------------------------------------------------------------ [2/3] ----
Write-Host '[2/3] Staging into dist\RustDVR' -ForegroundColor Cyan

# A clean stage every time. Leftovers from a previous run are how a file that is
# no longer shipped ends up inside an installer anyway.
if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
New-Item -ItemType Directory -Force -Path $Stage | Out-Null

$exe = Join-Path $Root 'target\release\rustdvr.exe'
if (-not (Test-Path $exe)) { Fail 'rustdvr.exe was not produced.' }
try {
    Copy-Item $exe $Stage -Force
} catch {
    Fail 'Could not stage rustdvr.exe. Is the app still running?'
}
Copy-Item (Join-Path $Root 'LICENSE.md') $Stage -Force

# The icon travels with the app so shortcuts and the uninstall entry can name
# it directly rather than relying on the embedded resource surviving.
$icon = Join-Path $Root 'assets\rustdvr.ico'
if (Test-Path $icon) {
    Copy-Item $icon $Stage -Force
} else {
    Write-Host '      WARNING: assets\rustdvr.ico missing; run scripts\make-icon.ps1' -ForegroundColor Yellow
}

# The LGPL text and the corresponding-source record travel with every copy.
# Section 6 requires the license text; pointing at where the FFmpeg source
# lives is what makes the "unmodified, replaceable" claim checkable.
$licenses = Join-Path $Root 'licenses'
if (Test-Path $licenses) {
    Copy-Item (Join-Path $licenses '*') (New-Item -ItemType Directory -Force -Path (Join-Path $Stage 'licenses')).FullName -Force
}

# The FFmpeg libraries sit beside the executable, unmodified and separately
# replaceable. That is not incidental: LGPL-2.1 section 6 requires that whoever
# receives this be able to substitute their own build of them, which is only
# true if they are shipped as ordinary DLLs rather than folded into the binary.
$dlls = Get-ChildItem (Join-Path $FFmpeg 'bin\*.dll') -ErrorAction SilentlyContinue
if (-not $dlls) { Fail "No FFmpeg DLLs found in $FFmpeg\bin" }
Copy-Item $dlls.FullName $Stage -Force

# The licence is read out of the binary that is actually being shipped, not
# taken on trust from the build script that produced it.
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

# Nothing shipped should name the person who built it.
#
# The test is the user profile directory specifically, not "any absolute path".
# An earlier version flagged anything containing \Desktop\RustDVR, which fired
# on remapped paths like "/home\Desktop\RustDVR\target\..." that carry no
# personal information at all — a warning that cried wolf on its own fix.
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
    Write-Host "    app  $Stage\rustdvr.exe"
    exit 0
}

# ------------------------------------------------------------------ [3/3] ----
Write-Host '[3/3] Building the installer' -ForegroundColor Cyan

$iscc = @(
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $iscc) {
    Fail @"
Inno Setup 6 was not found.

The staged application is in dist\RustDVR and can be run from there.
"@
}

& $iscc /Qp "/O$OutDir" "/DAppVersion=$Version" (Join-Path $Root 'installer\rustdvr.iss')
if ($LASTEXITCODE -ne 0) { Fail 'The installer failed to build.' }

Write-Host ''
Write-Host '  Done.' -ForegroundColor Green
Write-Host "    app        $Stage\rustdvr.exe"
Get-ChildItem (Join-Path $OutDir 'RustDVR-Setup-*.exe') -ErrorAction SilentlyContinue |
    ForEach-Object { Write-Host "    installer  $($_.FullName)" }
Write-Host ''
