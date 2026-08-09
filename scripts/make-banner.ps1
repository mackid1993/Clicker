<#
.SYNOPSIS
    Draw assets\banner.png, the repository's social preview.

.DESCRIPTION
    1280x640, which is what GitHub asks for and renders at. The remote is the
    icon's own artwork rather than a second drawing of the same object, so the
    two can never drift apart: make-icon.ps1 writes a 512px master and this
    composes it against a graphite field with the wordmark beside it.

    GitHub crops the preview for some cards, so nothing that has to be read
    goes near an edge. The remote overlaps the safe margin on purpose; the
    words do not.
#>

[CmdletBinding()]
param(
    [string]$Out = (Join-Path (Split-Path -Parent $PSScriptRoot) 'assets\banner.png')
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$Root = Split-Path -Parent $PSScriptRoot
$Icon = Join-Path $Root 'assets\clicker.png'

# The remote is not redrawn here. If the master is missing, the script that
# owns it runs, and this one stays the thing that arranges rather than the
# second place the artwork lives.
if (-not (Test-Path $Icon)) {
    & (Join-Path $PSScriptRoot 'make-icon.ps1') | Out-Null
}
if (-not (Test-Path $Icon)) { throw "assets\clicker.png is missing and make-icon.ps1 did not produce it." }

$W = 1280
$H = 640

# The icon's palette, not the interface's. See make-icon.ps1: this depicts an
# object from 1984, so the field is warm graphite rather than Fluent blue, and
# the one saturated color in the whole image is still the power key.
$FieldTop    = [System.Drawing.Color]::FromArgb(255, 38, 35, 33)
$FieldBottom = [System.Drawing.Color]::FromArgb(255, 20, 18, 17)
$Cream       = [System.Drawing.Color]::FromArgb(255, 246, 238, 222)
$Muted       = [System.Drawing.Color]::FromArgb(255, 158, 148, 134)
$Amber       = [System.Drawing.Color]::FromArgb(255, 232, 158,  64)

$bmp = New-Object System.Drawing.Bitmap $W, $H, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode     = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$g.PixelOffsetMode   = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
$g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::ClearTypeGridFit

# The field, lit from the top left so the remote has somewhere to cast into.
#
# Built from a rectangle and an angle rather than two points. A two-point
# gradient repeats past its second point, and with the axis ending partway
# across the image that wrap lands as a hard diagonal seam through the middle
# of the artwork. The rectangle form sizes the ramp to exactly what is being
# filled, so there is nothing left over to repeat.
$fieldRect = New-Object System.Drawing.RectangleF(0, 0, $W, $H)
$field = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    $fieldRect, $FieldTop, $FieldBottom, 55.0)
$g.FillRectangle($field, 0, 0, $W, $H)
$field.Dispose()

# A warm pool behind where the remote sits, drawn as concentric rings rather
# than a radial brush: GDI+'s PathGradientBrush blows out its center on a dark
# field, and forty rings of two percent alpha is a falloff that behaves.
$glowX = 360.0
$glowY = $H / 2
for ($i = 40; $i -ge 1; $i--) {
    $r = 40 + $i * 9.0
    $a = [int](2 + (40 - $i) * 0.12)
    $brush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb($a, 92, 82, 70))
    $g.FillEllipse($brush, ($glowX - $r), ($glowY - $r), ($r * 2), ($r * 2))
    $brush.Dispose()
}

# The remote, from the icon's own master.
$art = [System.Drawing.Image]::FromFile($Icon)
try {
    $artH = 470.0
    $artW = $artH * ($art.Width / $art.Height)
    $artX = $glowX - $artW / 2
    $artY = ($H - $artH) / 2
    $g.DrawImage($art, $artX, $artY, $artW, $artH)
} finally {
    $art.Dispose()
}

# The words. Segoe UI is on every machine this targets, and the fallback is
# only there so a run on a stripped image produces a banner rather than an
# exception.
function New-Font([string]$family, [single]$size, [System.Drawing.FontStyle]$style) {
    try { return New-Object System.Drawing.Font $family, $size, $style, ([System.Drawing.GraphicsUnit]::Pixel) }
    catch { return New-Object System.Drawing.Font 'Arial', $size, $style, ([System.Drawing.GraphicsUnit]::Pixel) }
}

$textX = 620.0
$wordmark = New-Font 'Segoe UI' 118 ([System.Drawing.FontStyle]::Bold)
$tagline  = New-Font 'Segoe UI' 30  ([System.Drawing.FontStyle]::Regular)
$footnote = New-Font 'Segoe UI' 23  ([System.Drawing.FontStyle]::Regular)

$creamBrush = New-Object System.Drawing.SolidBrush $Cream
$mutedBrush = New-Object System.Drawing.SolidBrush $Muted
$amberBrush = New-Object System.Drawing.SolidBrush $Amber

# Laid out from the middle outward so the block stays optically centered
# whatever the strings are.
$y = 196.0
$g.DrawString('Clicker', $wordmark, $creamBrush, $textX, $y)
$y += 150

# A short amber rule under the wordmark, the same amber as the function row on
# the keypad. It is the only place the eye is told to move left to right.
$g.FillRectangle($amberBrush, ($textX + 4), $y, 96, 5)
$y += 34

$g.DrawString('A native Windows client for', $tagline, $mutedBrush, $textX, $y)
$y += 40
$g.DrawString('Channels DVR Server', $tagline, $creamBrush, $textX, $y)
$y += 62
$g.DrawString('Unofficial. Live TV, recordings and a guide,', $footnote, $mutedBrush, $textX, $y)
$y += 32
$g.DrawString('in a single Rust binary.', $footnote, $mutedBrush, $textX, $y)

$creamBrush.Dispose(); $mutedBrush.Dispose(); $amberBrush.Dispose()
$wordmark.Dispose(); $tagline.Dispose(); $footnote.Dispose()
$g.Dispose()

$dir = Split-Path -Parent $Out
if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()

Write-Host ("  wrote {0}  ({1}x{2}, {3:N1} KB)" -f $Out, $W, $H, ((Get-Item $Out).Length / 1KB))
