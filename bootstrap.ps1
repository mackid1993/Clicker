<#
.SYNOPSIS
    Get a machine ready to build Clicker, then build the vendored FFmpeg.

.DESCRIPTION
    Checks every build prerequisite, reports what is missing, and — with
    -Install — installs it via winget. Then fetches FFmpeg at its pinned tag
    and builds it.

    Run once per machine. Afterwards, .\build.ps1 is all that is needed.

    Nothing here is required to *run* Clicker; the installer carries its own
    FFmpeg. These are build-time tools only.

.PARAMETER Install
    Install anything missing via winget instead of only reporting it.

.PARAMETER SkipFFmpeg
    Check the toolchain but do not fetch or build FFmpeg.

.PARAMETER SkipMpv
    Check the toolchain but do not build libmpv.

.EXAMPLE
    .\bootstrap.ps1              # tell me what I am missing
    .\bootstrap.ps1 -Install     # and fix it
#>

[CmdletBinding()]
param(
    [switch]$Install,
    [switch]$SkipFFmpeg,
    [switch]$SkipMpv
)

$ErrorActionPreference = 'Stop'
$Root = $PSScriptRoot
$FFmpegTag = 'n7.1.1'
$FFmpegSrc = Join-Path $Root 'third_party\ffmpeg-src'
$Msys = 'C:\msys64'

# The mingw packages libmpv needs. libass and libplacebo are not optional in
# mpv: both are plain dependencies in its meson.build with no way to turn them
# off. Both are license-compatible, ISC and LGPLv2.1+ respectively.
$MingwPackages = @(
    'mingw-w64-x86_64-gcc'
    'mingw-w64-x86_64-meson'
    'mingw-w64-x86_64-ninja'
    'mingw-w64-x86_64-pkgconf'
    'mingw-w64-x86_64-libass'
    'mingw-w64-x86_64-libplacebo'
    'make'
    'diffutils'
    'nasm'
)

function Say($text, $color = 'Gray') { Write-Host "  $text" -ForegroundColor $color }
function Ok($text)   { Write-Host "  [ok]   $text" -ForegroundColor Green }
function Miss($text) { Write-Host "  [need] $text" -ForegroundColor Yellow }
function Bad($text)  { Write-Host "  [fail] $text" -ForegroundColor Red }

Write-Host ''
Write-Host 'Clicker build prerequisites' -ForegroundColor Cyan
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

# MSYS2, for libmpv only.
#
# The application and its C shim are built with MSVC and always will be. mpv
# cannot be built with MSVC at all — upstream supports mingw-w64 and nothing
# else on Windows — so a second toolchain has to exist to produce the DLL.
# Nothing else in this repository uses it, and the DLL it produces is loaded by
# name at runtime, so no part of the MSVC build ever links against it.
$msysBash = Join-Path $Msys 'usr\bin\bash.exe'
$hasMsys = Need 'MSYS2 (for libmpv)' (Test-Path $msysBash) 'MSYS2.MSYS2' 'mpv cannot be built with MSVC'

# The packages inside it. Checked separately from MSYS2 itself, because a bare
# MSYS2 install has none of them and reporting "MSYS2 found" there would be the
# same lie as reporting Visual Studio without a C++ compiler.
$mingwReady = $false
if ($hasMsys) {
    $probe = & $msysBash -lc 'ls /mingw64/bin/gcc.exe /mingw64/bin/meson /mingw64/bin/ninja.exe /mingw64/lib/pkgconfig/libass.pc /mingw64/lib/pkgconfig/libplacebo.pc 2>&1'
    $mingwReady = ($LASTEXITCODE -eq 0)
    if ($mingwReady) {
        Ok 'mingw toolchain  (gcc, meson, ninja, libass, libplacebo)'
    } else {
        Miss 'mingw packages  ->  bootstrap.ps1 -Install will add them'
    }
}

# --------------------------------------------------------------- install ----
if ($missing.Count -gt 0 -or ($hasMsys -and -not $mingwReady)) {
    Write-Host ''
    if (-not $Install) {
        $count = $missing.Count + $(if ($hasMsys -and -not $mingwReady) { 1 } else { 0 })
        Write-Host "  $count prerequisite(s) missing. Re-run with -Install to fix." -ForegroundColor Yellow
        Write-Host ''
        exit 1
    }

    if ($missing.Count -gt 0 -and -not (Get-Command winget -ErrorAction SilentlyContinue)) {
        Bad 'winget is not available; install the prerequisites above by hand.'
        exit 1
    }

    foreach ($id in ($missing | Select-Object -Unique)) {
        Write-Host "  installing $id" -ForegroundColor Cyan
        # --silent so a long bootstrap does not stall on a dialog nobody is
        # watching. VS Build Tools still shows its own progress window.
        winget install --id $id --accept-package-agreements --accept-source-agreements --silent
    }

    # The mingw packages, once MSYS2 itself exists. Only reachable on a run
    # where MSYS2 was already installed; a run that just installed it exits
    # below and picks these up on the next pass, when its shell is in place.
    if ($hasMsys -and -not $mingwReady) {
        Write-Host '  installing mingw packages (gcc, meson, ninja, libass, libplacebo)' -ForegroundColor Cyan
        & $msysBash -lc "pacman-key --init >/dev/null 2>&1; pacman -Sy --noconfirm >/dev/null 2>&1; pacman -S --needed --noconfirm $($MingwPackages -join ' ')"
        if ($LASTEXITCODE -ne 0) { Bad 'pacman failed; run it by hand in the MSYS2 shell'; exit 1 }
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

# ------------------------------------------------------------------- mpv ----
#
# A second FFmpeg gets built here, by gcc, because that is what libmpv links
# against. It does not replace the one above: the application and its C shim
# are MSVC and use that one. The two never share a source tree.
if (-not $SkipMpv) {
    $mpvBuilt = Join-Path $Root 'third_party\mpv\libmpv-2.dll'
    if (Test-Path $mpvBuilt) {
        Say 'libmpv already built (delete third_party\mpv to force a rebuild)' 'DarkGray'
    } else {
        Write-Host '  Building libmpv. This takes about twenty minutes.' -ForegroundColor Cyan
        & (Join-Path $Root 'scripts\build-mpv.ps1')
        if ($LASTEXITCODE -ne 0) { Bad 'libmpv build failed'; exit 1 }
    }
}

Write-Host ''
Write-Host '  Ready. Run .\build.ps1 to build the app and the installer.' -ForegroundColor Green
Write-Host ''
