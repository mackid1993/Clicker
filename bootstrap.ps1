<#
.SYNOPSIS
    Get a machine ready to build RustDVR, then build the vendored FFmpeg.

.DESCRIPTION
    Checks every build prerequisite, reports what is missing, and — with
    -Install — installs it via winget. Then fetches FFmpeg at its pinned tag
    and builds it.

    Run once per machine. Afterwards, .\build.ps1 is all that is needed.

    Nothing here is required to *run* RustDVR; the installer carries its own
    FFmpeg. These are build-time tools only.

.PARAMETER Install
    Install anything missing via winget instead of only reporting it.

.PARAMETER SkipFFmpeg
    Check the toolchain but do not fetch or build FFmpeg.

.EXAMPLE
    .\bootstrap.ps1              # tell me what I am missing
    .\bootstrap.ps1 -Install     # and fix it
#>

[CmdletBinding()]
param(
    [switch]$Install,
    [switch]$SkipFFmpeg
)

$ErrorActionPreference = 'Stop'
$Root = $PSScriptRoot
$FFmpegTag = 'n7.1.1'
$FFmpegSrc = Join-Path $Root 'third_party\ffmpeg-src'

function Say($text, $color = 'Gray') { Write-Host "  $text" -ForegroundColor $color }
function Ok($text)   { Write-Host "  [ok]   $text" -ForegroundColor Green }
function Miss($text) { Write-Host "  [need] $text" -ForegroundColor Yellow }
function Bad($text)  { Write-Host "  [fail] $text" -ForegroundColor Red }

Write-Host ''
Write-Host 'RustDVR build prerequisites' -ForegroundColor Cyan
Write-Host ''

$missing = @()

function Need($name, $found, $wingetId, $note = '') {
    if ($found) {
        Ok "$name$(if ($note) { "  ($note)" })"
    } else {
        Miss "$name  ->  winget install $wingetId"
        $script:missing += $wingetId
    }
    return $found
}

# ---------------------------------------------------------------- checks ----
$cargo = (Get-Command cargo -ErrorAction SilentlyContinue)
Need 'Rust (cargo)' ([bool]$cargo) 'Rustlang.Rustup' $(if ($cargo) { (cargo --version) }) | Out-Null

# The MSVC compiler, not merely Visual Studio. A Build Tools install without
# the C++ workload has no cl.exe, and saying "Visual Studio found" there would
# be a lie that only surfaces twenty minutes into an FFmpeg build.
$vcvars = @(
    "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat",
    "$env:ProgramFiles\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat",
    "$env:ProgramFiles\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat",
    "$env:ProgramFiles\Microsoft Visual Studio\2022\Professional\VC\Auxiliary\Build\vcvars64.bat",
    "$env:ProgramFiles\Microsoft Visual Studio\2022\Enterprise\VC\Auxiliary\Build\vcvars64.bat"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if ($vcvars) {
    $vcRoot = Split-Path (Split-Path (Split-Path $vcvars))
    $hasCl = Get-ChildItem (Join-Path $vcRoot 'Tools\MSVC') -Directory -ErrorAction SilentlyContinue |
             Where-Object { Test-Path (Join-Path $_.FullName 'bin\Hostx64\x64\cl.exe') } |
             Select-Object -First 1
    if ($hasCl) {
        Ok "MSVC C++ compiler  ($($hasCl.Name))"
    } else {
        Bad 'Visual Studio 2022 is installed but has no C++ compiler'
        Say 'Add the "Desktop development with C++" workload in the VS Installer.' 'Yellow'
        $missing += 'Microsoft.VisualStudio.2022.BuildTools'
    }
} else {
    Miss 'Visual Studio 2022 Build Tools  ->  winget install Microsoft.VisualStudio.2022.BuildTools'
    Say 'Then add the "Desktop development with C++" workload.' 'Yellow'
    $missing += 'Microsoft.VisualStudio.2022.BuildTools'
}

$git = @(
    "$env:ProgramFiles\Git\bin\bash.exe",
    "${env:ProgramFiles(x86)}\Git\bin\bash.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
Need 'Git for Windows (bash)' ([bool]$git) 'Git.Git' 'FFmpeg configure needs a POSIX shell' | Out-Null

$nasm = (Get-Command nasm -ErrorAction SilentlyContinue)
Need 'NASM' ([bool]$nasm) 'NASM.NASM' $(if ($nasm) { $nasm.Source }) | Out-Null

$make = @('make', 'gmake', 'mingw32-make') |
        ForEach-Object { Get-Command $_ -ErrorAction SilentlyContinue } |
        Select-Object -First 1
Need 'GNU make' ([bool]$make) 'GnuWin32.Make' $(if ($make) { $make.Name }) | Out-Null

$iscc = @(
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if ($iscc) { Ok 'Inno Setup 6' }
else { Say '[opt]  Inno Setup 6  ->  winget install JRSoftware.InnoSetup  (only needed to package)' 'DarkGray' }

# --------------------------------------------------------------- install ----
if ($missing.Count -gt 0) {
    Write-Host ''
    if (-not $Install) {
        Write-Host "  $($missing.Count) prerequisite(s) missing. Re-run with -Install to fix." -ForegroundColor Yellow
        Write-Host ''
        exit 1
    }

    if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
        Bad 'winget is not available; install the prerequisites above by hand.'
        exit 1
    }

    foreach ($id in ($missing | Select-Object -Unique)) {
        Write-Host "  installing $id" -ForegroundColor Cyan
        # --silent so a long bootstrap does not stall on a dialog nobody is
        # watching. VS Build Tools still shows its own progress window.
        winget install --id $id --accept-package-agreements --accept-source-agreements --silent
    }

    Write-Host ''
    Write-Host '  Installed. Open a NEW terminal so PATH changes take effect,' -ForegroundColor Yellow
    Write-Host '  then run .\bootstrap.ps1 again.' -ForegroundColor Yellow
    Write-Host ''
    exit 0
}

Write-Host ''
Write-Host '  Toolchain complete.' -ForegroundColor Green
Write-Host ''

if ($SkipFFmpeg) { exit 0 }

# ---------------------------------------------------------------- ffmpeg ----
if (-not (Test-Path (Join-Path $FFmpegSrc 'configure'))) {
    Write-Host "  Fetching FFmpeg $FFmpegTag" -ForegroundColor Cyan
    New-Item -ItemType Directory -Force -Path (Split-Path $FFmpegSrc) | Out-Null
    # Shallow, single tag: the full history is 600MB and nothing here needs it.
    git clone --depth 1 --branch $FFmpegTag https://github.com/FFmpeg/FFmpeg.git $FFmpegSrc
    if ($LASTEXITCODE -ne 0) { Bad 'git clone failed'; exit 1 }
} else {
    Say "FFmpeg source already present" 'DarkGray'
}

$built = Join-Path $Root 'third_party\ffmpeg\bin\avcodec-61.dll'
if (Test-Path $built) {
    Say 'FFmpeg already built (delete third_party\ffmpeg to force a rebuild)' 'DarkGray'
} else {
    Write-Host '  Building FFmpeg. This takes about fifteen minutes.' -ForegroundColor Cyan
    & (Join-Path $Root 'scripts\build-ffmpeg.ps1')
    if ($LASTEXITCODE -ne 0) { Bad 'FFmpeg build failed'; exit 1 }
}

Write-Host ''
Write-Host '  Ready. Run .\build.ps1 to build the app and the installer.' -ForegroundColor Green
Write-Host ''
