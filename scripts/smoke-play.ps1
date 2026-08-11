# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial, native client for Channels DVR
# Copyright (c) 2026 David Brustein
#
# Play something for a minute and say, as a number, whether playback worked.
# The Windows half of scripts/smoke-play.sh; see that file for why it exists.
#
#   .\scripts\smoke-play.ps1 C:\test60.mp4
#   .\scripts\smoke-play.ps1 -Seconds 90 http://dvr:8089/devices/ANY/channels/2.1/stream.mpg
#   .\scripts\smoke-play.ps1 -Binary "C:\Program Files\Clicker\clicker.exe" C:\test60.mp4
#
# Exits 0 if playback held up, 1 if it did not, 2 if the test could not run.

[CmdletBinding()]
param(
    [Parameter(Position = 0, Mandatory = $true)][string] $Source,
    [int]    $Seconds = 45,
    [string] $Binary
)

$ErrorActionPreference = 'Stop'

# Same location as platform::data_home on Windows, and it has to stay in step.
$Log = Join-Path $env:LOCALAPPDATA 'Clicker\player.log'

if (-not $Binary) {
    $root = Split-Path -Parent $PSScriptRoot
    foreach ($candidate in @(
        (Join-Path $root 'target\release\clicker.exe'),
        (Join-Path $env:LOCALAPPDATA 'Programs\Clicker\clicker.exe'),
        "$env:ProgramFiles\Clicker\clicker.exe"
    )) {
        if (Test-Path $candidate) { $Binary = $candidate; break }
    }
}

if (-not $Binary -or -not (Test-Path $Binary)) {
    Write-Error 'no clicker.exe found; build one or pass -Binary <path>'
    exit 2
}

Write-Host "==> $Binary"
Write-Host "==> playing $Source for ${Seconds}s"

# Count what is already in the log rather than reading by timestamp: a log is
# appended to by whatever else happens to be running.
$mark = 0
if (Test-Path $Log) { $mark = (Get-Content $Log).Count }

$env:CLICKER_PLAY = $Source
$proc = Start-Process -FilePath $Binary -PassThru
Start-Sleep -Seconds $Seconds

if ($proc.HasExited) {
    Write-Error 'FAIL: it exited on its own before the test was over'
    exit 1
}
Stop-Process -Id $proc.Id -Force
Start-Sleep -Seconds 2

if (-not (Test-Path $Log)) {
    Write-Error "FAIL: no log at $Log — did it start?"
    exit 1
}

# Three numbers out of each five-second line: frames drawn, what the decoder
# produced, and mpv's running count of what it threw away.
$pattern = '\]\s+([\d.]+)fps drawn.*decoder ([\d.]+)fps, mpv dropped (\d+)'
$samples = @()
foreach ($line in (Get-Content $Log | Select-Object -Skip $mark)) {
    $m = [regex]::Match($line, $pattern)
    if ($m.Success) {
        $samples += [pscustomobject]@{
            Drawn   = [double]$m.Groups[1].Value
            Decoder = [double]$m.Groups[2].Value
            Dropped = [int]   $m.Groups[3].Value
        }
    }
}

if ($samples.Count -lt 3) {
    Write-Host "FAIL: only $($samples.Count) measurements in ${Seconds}s — playback never really started"
    Get-Content $Log | Select-Object -Skip $mark |
        Select-String -Pattern 'error|refused|missing|could not' | Select-Object -First 5
    exit 1
}

Write-Host ''
foreach ($s in $samples) {
    Write-Host ('    drawn {0,6:N1}   decoder {1,6:N1}   mpv dropped {2}' -f $s.Drawn, $s.Decoder, $s.Dropped)
}
Write-Host ''

# The first sample covers the seconds where the stream was still opening, so
# it is not counted. See the shell version for why 95% is the bar.
$rest    = $samples | Select-Object -Skip 1
$drawn   = ($rest | Measure-Object -Property Drawn   -Sum).Sum
$decoded = ($rest | Measure-Object -Property Decoder -Sum).Sum
$grew    = $rest[-1].Dropped - $rest[0].Dropped

if ($decoded -le 0) { Write-Host 'FAIL: the decoder produced nothing'; exit 1 }

$ratio = $drawn / $decoded
Write-Host ('    {0:P0} of decoded frames reached the screen ({1} samples)' -f $ratio, $rest.Count)
Write-Host ("    mpv dropped $grew frames during the test")

if ($ratio -lt 0.95) {
    Write-Host "`nFAIL: the renderer is not keeping up with the decoder."
    exit 1
}
if ($grew -gt ($Seconds / 10)) {
    Write-Host "`nFAIL: mpv is dropping frames steadily ($grew in ${Seconds}s)."
    exit 1
}
Write-Host "`nPASS"
exit 0
