<#
.SYNOPSIS
    Build FFmpeg from source for RustDVR.

.DESCRIPTION
    FFmpeg is built rather than downloaded because the licence has to be
    provable. Every prebuilt libVLC and libmpv binary for Windows embeds an
    FFmpeg configured with --enable-gpl, which would relicense this whole
    application under the GPL. Here --disable-gpl and --disable-nonfree are
    recorded in the build itself, and this script reads them back out of it
    before installing anything.

    Everything LGPL that a player can use is built. Only the encode side is
    dropped: encoders, muxers, filters and capture devices, none of which a
    player has any use for. Every decoder, demuxer, parser, protocol,
    bitstream filter and hardware accelerator is included, because a client
    that cannot open a recording is worse than one that is larger.

    FFmpeg's configure and Makefiles are POSIX shell scripts, so bash and make
    still do the work. This script sets up everything around them, which is the
    part that actually goes wrong on Windows.

.PARAMETER Reconfigure
    Re-run configure even if the tree is already configured.

.PARAMETER Clean
    Delete all build output first.

.EXAMPLE
    scripts\build-ffmpeg.ps1
    scripts\build-ffmpeg.ps1 -Reconfigure
#>

[CmdletBinding()]
param(
    [switch]$Reconfigure,
    [switch]$Clean
)

$ErrorActionPreference = 'Stop'

$Root   = Split-Path -Parent $PSScriptRoot
$Src    = Join-Path $Root 'third_party\ffmpeg-src'
$Prefix = Join-Path $Root 'third_party\ffmpeg'
$Version = 'n7.1.1'

function Fail($message) {
    Write-Host ''
    Write-Host "  $message" -ForegroundColor Red
    Write-Host ''
    exit 1
}

function Step($message) {
    Write-Host "  $message" -ForegroundColor Cyan
}

# ---------------------------------------------------------------- source ----
if (-not (Test-Path (Join-Path $Src 'configure'))) {
    Fail @"
FFmpeg source not found at $Src

  git clone --depth 1 --branch $Version https://github.com/FFmpeg/FFmpeg.git "$Src"
"@
}

# ------------------------------------------------------------------ bash ----
# Git for Windows carries an MSYS2 environment: bash, sh, sed, awk, coreutils.
$GitRoot = @(
    "$env:ProgramFiles\Git",
    "${env:ProgramFiles(x86)}\Git",
    "$env:LOCALAPPDATA\Programs\Git"
) | Where-Object { Test-Path (Join-Path $_ 'bin\bash.exe') } | Select-Object -First 1

if (-not $GitRoot) { Fail "Git for Windows not found. FFmpeg's configure needs a POSIX shell." }
$Bash = Join-Path $GitRoot 'bin\bash.exe'

# make must be handed a real sh.exe. Left to itself, GNU make on Windows falls
# back to cmd.exe, and that breaks the build in two separate ways:
#
#   * cmd.exe caps a command line at 8191 characters. Linking libavcodec passes
#     every object file to compat/windows/makedef on one line, which for a full
#     build is around 26,000 characters, so the list is truncated mid-path and
#     the link fails with "Object does not exist: libavcodec/h".
#   * cmd.exe does not treat single quotes as quotes, so the awk program in
#     FFmpeg's dependency command arrives mangled and every compile dies before
#     the compiler is even reached.
#
# The 8.3 short name is used because the path contains a space, and make cannot
# be given a quoted SHELL.
$ShPath = Join-Path $GitRoot 'usr\bin\sh.exe'
if (-not (Test-Path $ShPath)) { Fail "sh.exe not found at $ShPath" }
$ShShort = (New-Object -ComObject Scripting.FileSystemObject).GetFile($ShPath).ShortPath
$ShShort = $ShShort -replace '\\', '/'

# ------------------------------------------------------------------ MSVC ----
$VcVars = @(
    "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat",
    "$env:ProgramFiles\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat",
    "$env:ProgramFiles\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat",
    "$env:ProgramFiles\Microsoft Visual Studio\2022\Professional\VC\Auxiliary\Build\vcvars64.bat"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $VcVars) { Fail 'Visual Studio 2022 build tools with the C++ workload were not found.' }

Step 'importing the MSVC environment'
# vcvars is a batch file, so it is run once and the environment it produced is
# lifted into this session rather than shelling out to it repeatedly.
cmd /c "`"$VcVars`" >nul 2>&1 && set" | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') {
        Set-Item -Path "Env:\$($matches[1])" -Value $matches[2] -ErrorAction SilentlyContinue
    }
}
if (-not $env:VCToolsInstallDir) { Fail 'vcvars64.bat did not set up the compiler.' }

$MsvcBin = (Join-Path $env:VCToolsInstallDir 'bin\Hostx64\x64')
if (-not (Test-Path (Join-Path $MsvcBin 'cl.exe'))) { Fail "cl.exe not found in $MsvcBin" }

# nasm is needed for the x86 assembly, which is most of the decode performance.
$Nasm = (Get-Command nasm -ErrorAction SilentlyContinue).Source
if (-not $Nasm) { Fail 'nasm not found on PATH. FFmpeg needs it to build its assembly.' }
$NasmDir = Split-Path -Parent $Nasm

# GNU make goes by several names on Windows. Strawberry Perl, which is the most
# likely source of one here, installs it as gmake and mingw32-make but not as
# plain make.
$Make = @('make', 'gmake', 'mingw32-make') |
    ForEach-Object { Get-Command $_ -ErrorAction SilentlyContinue } |
    Select-Object -First 1
if (-not $Make) { Fail 'GNU make not found on PATH (looked for make, gmake, mingw32-make).' }
$MakeName = $Make.Name -replace '\.exe$', ''

# MSVC's switches start with a slash, and MSYS rewrites anything that looks like
# a path. Without this, /nologo reaches cl.exe as C:/nologo.
$env:MSYS2_ARG_CONV_EXCL = '*'

# Strawberry Perl ships a pkg-config that is a Perl script. If it is found, every
# probe fails with "Could not run pkg-config". Nothing here needs it.
Remove-Item Env:\PKG_CONFIG -ErrorAction SilentlyContinue
Remove-Item Env:\PKG_CONFIG_PATH -ErrorAction SilentlyContinue

function ConvertTo-PosixPath([string]$path) {
    $p = $path -replace '\\', '/'
    if ($p -match '^([A-Za-z]):(.*)$') { return "/$($matches[1].ToLower())$($matches[2])" }
    return $p
}

$SrcPosix    = ConvertTo-PosixPath $Src
# Windows form with a trailing separator, for /d1trimfile. Backslashes are
# doubled because the value travels through the shell before reaching cl.exe.
$SrcPosixNative = ($Src.TrimEnd('\') + '\') -replace '\\', '\\'
$PrefixPosix = ConvertTo-PosixPath $Prefix
$MsvcPosix   = ConvertTo-PosixPath $MsvcBin
$NasmPosix   = ConvertTo-PosixPath $NasmDir

# Deliberately returns nothing. A PowerShell function returns everything it
# writes to the pipeline, not just the value after `return`, so `return
# $LASTEXITCODE` here handed back an array of every line the command printed.
# Comparing that array against 0 was always truthy, and a configure run that
# had in fact succeeded was reported as a failure. Callers check $LASTEXITCODE,
# which survives the call, instead.
function Invoke-Bash([string]$script) {
    # Double quotes, so bash expands $PATH. With single quotes it took the text
    # literally, PATH became the string "...:$PATH", and configure lost sed, tr
    # and every other coreutil it depends on.
    #
    # MSVC comes first so its link.exe wins over the coreutils one that Git
    # also ships, and /usr/bin is named explicitly rather than assumed.
    $full = 'export PATH="' + $MsvcPosix + ':' + $NasmPosix + ':/usr/bin:$PATH"; ' +
            'unset PKG_CONFIG PKG_CONFIG_PATH; ' +
            'cd "' + $SrcPosix + '" || exit 1; ' + $script
    & $Bash -c $full
}

# ----------------------------------------------------------------- clean ----
if ($Clean) {
    Step 'cleaning'
    Invoke-Bash "$MakeName distclean >/dev/null 2>&1 || true"
    Remove-Item -Recurse -Force $Prefix -ErrorAction SilentlyContinue
}

# ------------------------------------------------------------- configure ----
$ConfigMak = Join-Path $Src 'ffbuild\config.mak'
if ($Reconfigure -or -not (Test-Path $ConfigMak)) {
    Step "configuring FFmpeg $Version (LGPL, decode only)"

    $options = @(
        # A neutral prefix, deliberately. FFmpeg bakes the entire configure
        # line into the binary as FFMPEG_CONFIGURATION, readable at runtime by
        # anyone with a copy, so a real build path would ship the developer's
        # user directory and name inside every DLL. The real destination is
        # supplied to `make install` instead, where it is not recorded.
        '--prefix=/rustdvr'
        '--toolchain=msvc'
        '--target-os=win64'
        '--arch=x86_64'
        '--enable-shared'
        '--disable-static'
        # The licence position of this entire application rests on these two.
        '--disable-gpl'
        '--disable-nonfree'
        # No external libraries, so nothing can be picked up off this machine
        # and silently become a dependency or a licence problem.
        '--disable-autodetect'
        # Windows' own TLS, for HTTPS sources.
        '--enable-schannel'
        # Hardware decode paths.
        '--enable-d3d11va'
        '--enable-dxva2'
        # A player does not encode, mux, filter or capture.
        # Only the things that are not codecs at all: the command line
        # programs, the documentation, and avdevice, which is capture hardware.
        #
        # Encoders and muxers are NOT disabled, though a player cannot use
        # them. Turning them off leaves libavcodec internally inconsistent:
        # mpegvideo_enc.o is still compiled, because decoders share code with
        # it, but the encoders it calls are gone, and the link fails with sixty
        # unresolved symbols like ff_wmv2_encode_mb. They were originally
        # dropped to shrink the linker command line, and the response-file
        # patches above removed that constraint, so there is no longer a reason
        # to fight FFmpeg's dependency graph.
        '--disable-programs'
        '--disable-doc'
        '--disable-avdevice'
        # NOTE: the __FILE__ trimming flag deliberately does NOT go here.
        # FFmpeg records its entire configure line inside the binary, so
        # passing a path to configure embeds that path — the exact thing the
        # flag exists to remove. It is added to CFLAGS after configure instead.
        # No debug info. Smaller DLLs, and MSVC embeds absolute source paths in
        # debug records, which is another way the build machine's directory
        # layout ends up inside a shipped binary.
        '--disable-debug'
    ) -join ' '

    Invoke-Bash "./configure $options"
    if ($LASTEXITCODE -ne 0) { Fail "configure failed. See $Src\ffbuild\config.log" }
    $NeedsClean = $true
} else {
    Step 'already configured (use -Reconfigure to redo)'
}

# --------------------------------------------------------- __FILE__ fix ----
# MSVC bakes the absolute path of every source file into assertion strings, so
# a stock build ships lines like "C:\Users\<name>\...\libavutil\iamf.h" inside
# avutil — the developer's home directory, in a binary handed to other people.
#
# /d1trimfile strips the prefix at compile time. It is undocumented but has
# worked since VS2015 and is what MSBuild uses for deterministic builds.
#
# It is appended to CFLAGS here rather than passed to configure, because
# configure records its own arguments inside the binary as
# FFMPEG_CONFIGURATION: passing the path there embedded it a second time and
# made the leak worse than doing nothing.
$mak = Get-Content $ConfigMak -Raw
if ($mak -notmatch 'd1trimfile') {
    Step 'stripping source paths from __FILE__'
    # No trailing separator. With one, the quoted argument ends in a backslash,
    # which escapes the closing quote and every compile dies with
    # "unexpected EOF while looking for matching \"". Without it, __FILE__
    # keeps a leading slash — "\libavutil\iamf.h" — which is harmless, and the
    # user name is gone either way.
    $trim = "/d1trimfile:" + $Src.TrimEnd('\')
    $mak = ($mak -split "`n" | ForEach-Object {
        # Quoted, because the path may contain spaces and make hands the line
        # to the compiler verbatim.
        if ($_ -match '^CFLAGS=') { $_ + " `"$trim`"" } else { $_ }
    }) -join "`n"
    Set-Content -Path $ConfigMak -Value $mak -NoNewline
}

# ----------------------------------------------------- command line fix ----
# Linking a shared libavcodec on Windows is impossible as FFmpeg ships it.
#
# ffbuild/config.mak builds the export list by passing every object file to
# compat/windows/makedef on a single command line. For a full decoder set that
# is around 40,000 characters, and Windows refuses anything past 32,767 (8,191
# if the command goes through cmd.exe). The list is truncated mid-path and the
# link dies with "Object does not exist: libavcodec/h".
#
# Two small patches, both idempotent:
#   1. makedef learns to read its object list from a file given as @path.
#   2. config.mak writes that file using GNU make's $(file ...) function, which
#      writes directly and never builds a command line at all.
$MakeDef = Join-Path $Src 'compat\windows\makedef'
$makedefText = Get-Content $MakeDef -Raw
if ($makedefText -notmatch '\$\{1#@\}') {
    Step 'patching makedef to accept an object list file'
    $shim = @'
# Accept "@file" holding one object path per line. Windows cannot pass a full
# FFmpeg object list on a command line; see scripts/build-ffmpeg.ps1.
if [ $# -eq 1 ] && [ "${1#@}" != "$1" ]; then
    set -- $(cat "${1#@}")
fi

if [ ! -f "$vscript" ]; then
'@
    $makedefText = $makedefText -replace '(?m)^if \[ ! -f "\$vscript" \]; then', $shim
    Set-Content -Path $MakeDef -Value $makedefText -NoNewline
}

$mak = Get-Content $ConfigMak -Raw
if ($mak -match '(?m)^SLIB_CREATE_DEF_CMD=.*\$\(OBJS\)') {
    Step 'patching config.mak to write the object list to a file'
    $replacement = 'SLIB_CREATE_DEF_CMD=$(file >$(SUBDIR)lib$(NAME).objs,$(OBJS))EXTERN_PREFIX="$(EXTERN_PREFIX)" $(SRC_PATH)/compat/windows/makedef $(SUBDIR)lib$(NAME).ver @$(SUBDIR)lib$(NAME).objs > $$(@:$(SLIBSUF)=.def)'
    # Line by line rather than -replace: the replacement text is full of $(...)
    # and $$, which -replace would try to interpret as capture group references.
    $mak = ($mak -split "`n" | ForEach-Object {
        if ($_ -match '^SLIB_CREATE_DEF_CMD=') { $replacement } else { $_ }
    }) -join "`n"
    Set-Content -Path $ConfigMak -Value $mak -NoNewline
}

# The link step has the same problem as makedef: ffbuild/library.mak passes
# every object to the linker on one command line. link.exe accepts a response
# file, so the object list goes into one and only "@path" is passed.
$LibraryMak = Join-Path $Src 'ffbuild\library.mak'
$libText = Get-Content $LibraryMak -Raw
if ($libText -notmatch '\.lnk') {
    Step 'patching library.mak to link via a response file'
    $oldLink = '$$(LD) $(SHFLAGS) $(LDFLAGS) $(LDSOFLAGS) $$(LD_O) $$(filter %.o,$$^) $(FFEXTRALIBS)'
    $newLink = '$$(file >$(SUBDIR)lib$(NAME).lnk,$$(filter %.o,$$^))$$(LD) $(SHFLAGS) $(LDFLAGS) $(LDSOFLAGS) $$(LD_O) @$(SUBDIR)lib$(NAME).lnk $(FFEXTRALIBS)'
    $libText = ($libText -split "`n" | ForEach-Object {
        if ($_.Trim() -eq $oldLink) { "`t" + $newLink } else { $_ }
    }) -join "`n"
    Set-Content -Path $LibraryMak -Value $libText -NoNewline
}

# Dependency generation is disabled for the same family of reasons: config.mak
# pipes cl.exe's /showIncludes output into an awk program containing
# gsub(/\\/, "/"), and one backslash is eaten in transit, so awk dies with a
# syntax error before the compiler ever runs. The .d files only matter for
# incremental rebuilds, which a pinned vendored build does not do.
if ($mak -match 'gsub\(/') {
    Step 'disabling MSVC dependency generation (unusable quoting)'
    $mak = ($mak -split "`n" | ForEach-Object {
        if ($_ -match '^(CCDEP|CXXDEP|ASDEP|HOSTCCDEP)=') { ($_ -split '=')[0] + '=true' } else { $_ }
    }) -join "`n"
    Set-Content -Path $ConfigMak -Value $mak -NoNewline
}

# ------------------------------------------------------- licence gateway ----
# Checked before a single object is compiled, so a build that could not legally
# be shipped is never even started.
$ConfigH = Join-Path $Src 'config.h'
if (-not (Test-Path $ConfigH)) { Fail "configure did not produce config.h" }

$gpl     = Select-String -Path $ConfigH -Pattern '^#define CONFIG_GPL 1$'     -Quiet
$nonfree = Select-String -Path $ConfigH -Pattern '^#define CONFIG_NONFREE 1$' -Quiet
if ($gpl)     { Fail 'REFUSING: this configuration enables GPL components.' }
if ($nonfree) { Fail 'REFUSING: this configuration enables nonfree components.' }
Step 'licence check passed: no GPL, no nonfree'

# ----------------------------------------------------------------- build ----
# Reconfiguring means every object is suspect.
#
# Normally make would notice, because each object depends on config.h. That
# dependency lives in the .d files, and generating those is disabled above
# since the awk that writes them cannot survive Windows quoting. So nothing
# rebuilds, objects compiled under two different configurations get linked
# together, and the result is a pile of unresolved symbols like
# ff_put_dirac_pixels32_l4_c that look like an FFmpeg bug and are not.
if ($NeedsClean) {
    Step 'cleaning objects from the previous configuration'
    Invoke-Bash "$MakeName SHELL='$ShShort' clean >/dev/null 2>&1 || true"
}

$jobs = [Environment]::ProcessorCount
Step "building with $MakeName, $jobs jobs (SHELL=$ShShort)"

# The real prefix is given here rather than to configure, so it never becomes
# part of the recorded configuration string inside the binaries.
Invoke-Bash "$MakeName SHELL='$ShShort' -j$jobs && $MakeName SHELL='$ShShort' install prefix='$PrefixPosix'"
if ($LASTEXITCODE -ne 0) { Fail 'build failed.' }

# Confirm rather than assume. If a build path leaked through anyway, it is
# better to know now than after it has been handed to someone.
$leaks = @()
foreach ($dll in Get-ChildItem (Join-Path $Prefix 'bin\*.dll') -ErrorAction SilentlyContinue) {
    $text = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($dll.FullName))
    if ($text -match 'C:\\Users\\|/c/Users/|\\Desktop\\') { $leaks += $dll.Name }
}
if ($leaks) {
    Write-Host ''
    Write-Host "  WARNING: build paths are still present in: $($leaks -join ', ')" -ForegroundColor Yellow
} else {
    Step 'no build paths embedded in the binaries'
}

# ---------------------------------------------------------------- report ----
$dlls = Get-ChildItem (Join-Path $Prefix 'bin\*.dll') -ErrorAction SilentlyContinue
if (-not $dlls) { Fail "the build reported success but produced no DLLs in $Prefix\bin" }

Write-Host ''
Write-Host '  Installed to ' -NoNewline; Write-Host $Prefix -ForegroundColor Green
foreach ($dll in $dlls) {
    Write-Host ('    {0,-22} {1,6:N1} MB' -f $dll.Name, ($dll.Length / 1MB))
}
Write-Host ('    {0,-22} {1,6:N1} MB' -f 'total', (($dlls | Measure-Object Length -Sum).Sum / 1MB))

$counts = [ordered]@{}
foreach ($kind in 'DECODER', 'DEMUXER', 'PARSER', 'PROTOCOL', 'HWACCEL', 'BSF') {
    $counts[$kind] = (Select-String -Path $ConfigH -Pattern "^#define CONFIG_[A-Z0-9_]+_$kind 1$").Count
}
Write-Host ''
Write-Host '  Components:'
foreach ($kind in $counts.Keys) {
    Write-Host ('    {0,-10} {1}' -f $kind, $counts[$kind])
}
Write-Host ''
