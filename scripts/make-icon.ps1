# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial client for Channels DVR Server
# Copyright (c) 2026 David Brustein

<#
.SYNOPSIS
    Draw the Clicker icon and pack it into assets\clicker.ico.

.DESCRIPTION
    The icon is the thing the app is named for: a 1980s television remote,
    seen straight on. A chunky graphite body, an infrared window across the
    top, one red power key set into a well, and a grid of cream keys — Zenith
    Space Command by way of an early VCR remote, not a flat modern glyph.

    Drawn rather than shipped as a binary asset so it can be adjusted, and so
    the repository carries the recipe instead of an opaque file nobody can
    edit.

    The design is constrained by the smallest size it has to survive. At 16px
    the body is about seven pixels wide, which is room for a silhouette and
    one bright dot — nothing more. So detail is added with size rather than
    drawn once and scaled down: below 32px only the body and the red power
    dot remain; from 32px the keypad appears as a 2x3 grid of chunky keys;
    from 64px it becomes the full 3x4 grid with an amber function row, key
    shadows, the emitter lens, and the IEC power mark on the red key. Each
    tier is what still reads at that size, which is what keeps it legible in
    a taskbar.
#>

[CmdletBinding()]
param(
    [string]$Out = (Join-Path (Split-Path -Parent $PSScriptRoot) 'assets\clicker.ico')
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

# The palette is the object, not the app. src/theme.rs is cool Fluent blue;
# the icon is deliberately warmer — graphite plastic, cream keys, one
# saturated red — because it depicts a thing from 1984, not a surface from
# Windows 11. The red is the only saturated color anywhere, so the eye lands
# on the power key at every size.
$BodyTop    = [System.Drawing.Color]::FromArgb(255,  92,  85,  77)
$BodyBottom = [System.Drawing.Color]::FromArgb(255,  41,  37,  34)
$IrTop      = [System.Drawing.Color]::FromArgb(255,  30,  24,  32)
$IrBottom   = [System.Drawing.Color]::FromArgb(255,  10,   8,  13)
$KeyTop     = [System.Drawing.Color]::FromArgb(255, 244, 233, 208)
$KeyBottom  = [System.Drawing.Color]::FromArgb(255, 209, 193, 162)
$AmberTop   = [System.Drawing.Color]::FromArgb(255, 248, 184,  92)
$AmberBottom= [System.Drawing.Color]::FromArgb(255, 214, 141,  48)
$RedTop     = [System.Drawing.Color]::FromArgb(255, 246,  92,  66)
$RedBottom  = [System.Drawing.Color]::FromArgb(255, 186,  24,  30)
$Well       = [System.Drawing.Color]::FromArgb(255,  22,  20,  18)
# Cream rather than white, and translucent: on a dark taskbar a charcoal
# silhouette simply vanishes, and this hairline is what keeps an edge there.
# Warm and faint so on a light taskbar it reads as rim light, not an outline.
$Rim        = [System.Drawing.Color]::FromArgb(120, 255, 236, 200)

function New-RoundedPath([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $r * 2
    $path.AddArc($x, $y, $d, $d, 180, 90)
    $path.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $path.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $path.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    return $path
}

# One key, drawn as a slab: a dark copy offset downward, then the key with a
# lit-from-above gradient over it. The offset shadow is what makes a rounded
# rectangle read as something a thumb could press instead of a printed label.
# It is optional because at 32px the offset is under a pixel and only fuzzes
# the key's bottom edge.
function Add-Key([System.Drawing.Graphics]$g,
                 [float]$kx, [float]$ky, [float]$kw, [float]$kh,
                 [System.Drawing.Color]$top, [System.Drawing.Color]$bottom,
                 [bool]$withShadow) {
    $r = [Math]::Max(0.8, [Math]::Min($kw, $kh) * 0.32)
    if ($withShadow) {
        $drop = New-RoundedPath $kx ($ky + $kh * 0.12) $kw $kh $r
        $dropBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(90, 0, 0, 0))
        $g.FillPath($dropBrush, $drop)
        $drop.Dispose(); $dropBrush.Dispose()
    }
    $path = New-RoundedPath $kx $ky $kw $kh $r
    # The gradient's endpoints sit a pixel past the key: a LinearGradientBrush
    # wraps at its exact edges, and antialiased edge pixels sample the wrapped
    # color as a hard seam.
    $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.PointF($kx, ($ky - 1))),
        (New-Object System.Drawing.PointF($kx, ($ky + $kh + 1))),
        $top, $bottom)
    $g.FillPath($brush, $path)
    $brush.Dispose(); $path.Dispose()
}

function New-Icon([int]$size) {
    $bmp = New-Object System.Drawing.Bitmap $size, $size,
        ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.Clear([System.Drawing.Color]::Transparent)

    $s = [float]$size
    $detailed = $size -ge 32   # the keypad appears
    $full     = $size -ge 64   # full grid, amber row, lens, shadows, gloss

    # The remote leans a few degrees, which is the difference between a
    # product mark and a diagram. Small sizes stay upright: a tilt at 16px
    # only blurs the silhouette.
    if ($detailed) {
        $g.TranslateTransform($s / 2, $s / 2)
        $g.RotateTransform(-8)
        $g.TranslateTransform(-$s / 2, -$s / 2)
    }

    # Chunky proportions, on purpose. A remote from this era was a brick with
    # corners rounded just enough to leave a pocket intact — slimmer and it
    # turns into a candy bar, wider and it turns into a pager.
    $w = $s * 0.42
    $h = $s * 0.86
    $x = ($s - $w) / 2
    $y = ($s - $h) / 2
    $radius = [Math]::Max(1.5, $w * 0.24)

    # A soft shadow under the body, so it sits above the canvas instead of
    # being a sticker on it.
    if ($detailed) {
        $shadow = New-RoundedPath ($x + $s * 0.015) ($y + $s * 0.03) $w $h $radius
        $shadowBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(55, 0, 0, 0))
        $g.FillPath($shadowBrush, $shadow)
        $shadow.Dispose(); $shadowBrush.Dispose()
    }

    # The body is a vertical gradient, lit from above. Flat charcoal is what
    # makes an icon look like clip art; the gradient is what makes it plastic.
    $bodyPath = New-RoundedPath $x $y $w $h $radius
    $bodyBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.PointF($x, ($y - 1))),
        (New-Object System.Drawing.PointF($x, ($y + $h + 1))),
        $BodyTop, $BodyBottom)
    $g.FillPath($bodyBrush, $bodyPath)
    $bodyBrush.Dispose()

    # The infrared window: a near-black band across the top of the case,
    # clipped to the body so its corners follow the shell. This is the detail
    # that says "remote" rather than "calculator" — a calculator has no reason
    # to have a dark nose.
    if ($detailed) {
        $bandH = $h * 0.085
        $g.SetClip($bodyPath)
        $irBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
            (New-Object System.Drawing.PointF($x, ($y - 1))),
            (New-Object System.Drawing.PointF($x, ($y + $bandH + 1))),
            $IrTop, $IrBottom)
        $g.FillRectangle($irBrush, $x, $y, $w, $bandH)
        $irBrush.Dispose()

        # The emitter itself, a faint maroon slit in the window. Only at 64px
        # and up: below that the whole band is two pixels tall and the slit
        # would just dirty it.
        if ($full) {
            $slitW = $w * 0.30
            $slitH = $bandH * 0.42
            $slit = New-RoundedPath ($x + ($w - $slitW) / 2) ($y + ($bandH - $slitH) / 2) $slitW $slitH ($slitH / 2)
            $slitBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 96, 38, 44))
            $g.FillPath($slitBrush, $slit)
            $slit.Dispose(); $slitBrush.Dispose()
        }
        $g.ResetClip()
    }

    # The rim, at every size — see $Rim for why it exists at all.
    $rimPen = New-Object System.Drawing.Pen $Rim, ([Math]::Max(1.0, $s / 110))
    $g.DrawPath($rimPen, $bodyPath)
    $rimPen.Dispose()
    $bodyPath.Dispose()

    # The power key: the one element that must survive every size, and the
    # only saturated color, so it never has to compete for attention. At the
    # small tier it is simply a red dot near the top of the silhouette — with
    # seven pixels of body width, a bezel would only shrink the dot past
    # legibility.
    $power = if ($full) { $w * 0.32 } elseif ($detailed) { $w * 0.36 } else { $w * 0.50 }
    $px = $x + ($w - $power) / 2
    $py = if ($detailed) { $y + $h * 0.21 - $power / 2 } else { $y + $h * 0.10 }

    if ($detailed) {
        # The key sits in a recessed well, which is what makes it a part
        # molded into the case rather than a circle printed on it.
        $wellD = $power * 1.35
        $wellBrush = New-Object System.Drawing.SolidBrush $Well
        $g.FillEllipse($wellBrush, ($px - ($wellD - $power) / 2), ($py - ($wellD - $power) / 2), $wellD, $wellD)
        $wellBrush.Dispose()
    }

    $powerBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.PointF($px, ($py - 1))),
        (New-Object System.Drawing.PointF($px, ($py + $power + 1))),
        $RedTop, $RedBottom)
    $g.FillEllipse($powerBrush, $px, $py, $power, $power)
    $powerBrush.Dispose()

    # The IEC power mark — the broken ring with a stem through the gap. A
    # bare red dome reads as a recording light; the mark is what makes it an
    # on/off key. Only at 64px and up, because the glyph needs about nine
    # pixels of button before the ring survives as a ring, and below that the
    # dome alone carries the meaning.
    if ($full) {
        $cx = $px + $power / 2
        $cy = $py + $power * 0.53          # a hair low, out from under the sheen
        $r = $power * 0.27
        $mark = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(235, 255, 250, 238)), ([Math]::Max(1.0, $power * 0.11))
        $mark.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
        $mark.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
        # A 290-degree arc whose 70-degree gap is centered on twelve o'clock,
        # which is where the stem passes through.
        $g.DrawArc($mark, ($cx - $r), ($cy - $r), ($r * 2), ($r * 2), 305, 290)
        $g.DrawLine($mark, $cx, ($cy - $r * 1.30), $cx, ($cy - $r * 0.10))
        $mark.Dispose()
    }

    # A sheen across the top of the key, over the mark rather than under it:
    # power keys of the era were domed plastic with the symbol printed on the
    # plastic, so the shine sits on top of everything. The gloss is what says
    # "domed" instead of "flat disc".
    if ($full) {
        $gloss = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(95, 255, 255, 255))
        $g.FillEllipse($gloss, ($px + $power * 0.20), ($py + $power * 0.10), ($power * 0.60), ($power * 0.34))
        $gloss.Dispose()
    }

    # The keypad, in two tiers rather than one drawing scaled down. At 32px a
    # 3x4 grid collapses into gray mush, but 2x3 chunky keys still read as
    # "buttons"; from 64px there is room for the full grid, and for an amber
    # function row across the top of it — which is the sort of thing 1984
    # actually shipped.
    if ($detailed) {
        if ($full) {
            $cols = 3; $rows = 4; $gapFrac = 0.10
            $kx0 = $x + $w * 0.15; $kw = $w * 0.70
            $ky0 = $y + $h * 0.36; $kh = $h * 0.55
        } else {
            $cols = 2; $rows = 3; $gapFrac = 0.16
            $kx0 = $x + $w * 0.17; $kw = $w * 0.66
            $ky0 = $y + $h * 0.38; $kh = $h * 0.48
        }
        $gapX = $kw * $gapFrac
        $gapY = $kh * $gapFrac
        $keyW = ($kw - ($cols - 1) * $gapX) / $cols
        $keyH = ($kh - ($rows - 1) * $gapY) / $rows

        foreach ($row in 0..($rows - 1)) {
            foreach ($col in 0..($cols - 1)) {
                $amber = $full -and $row -eq 0
                Add-Key $g `
                    ($kx0 + $col * ($keyW + $gapX)) ($ky0 + $row * ($keyH + $gapY)) `
                    $keyW $keyH `
                    ($amber ? $AmberTop : $KeyTop) ($amber ? $AmberBottom : $KeyBottom) `
                    $full
            }
        }
    }

    $g.Dispose()
    return $bmp
}

# Windows picks the nearest size, so all the ones it asks for are provided
# rather than letting it scale something and blur it.
$sizes = @(16, 20, 24, 32, 40, 48, 64, 128, 256)
$pngs = @()
foreach ($size in $sizes) {
    $bmp = New-Icon $size
    $stream = New-Object System.IO.MemoryStream
    $bmp.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
    $pngs += , @{ Size = $size; Bytes = $stream.ToArray() }
    $stream.Dispose()
    $bmp.Dispose()
}

# ICO container. Every entry is stored as PNG, which Windows has accepted since
# Vista and which avoids hand-rolling the BMP-with-AND-mask format.
$dir = Split-Path -Parent $Out
if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }

$fs = [System.IO.File]::Create($Out)
$bw = New-Object System.IO.BinaryWriter $fs
try {
    $bw.Write([UInt16]0)                # reserved
    $bw.Write([UInt16]1)                # type: icon
    $bw.Write([UInt16]$pngs.Count)

    # Image data starts after the header and the directory entries.
    $offset = 6 + (16 * $pngs.Count)
    foreach ($png in $pngs) {
        # 256 is written as 0, which is how the format expresses it.
        $dim = if ($png.Size -ge 256) { 0 } else { $png.Size }
        $bw.Write([Byte]$dim)           # width
        $bw.Write([Byte]$dim)           # height
        $bw.Write([Byte]0)              # palette count
        $bw.Write([Byte]0)              # reserved
        $bw.Write([UInt16]1)            # color planes
        $bw.Write([UInt16]32)           # bits per pixel
        $bw.Write([UInt32]$png.Bytes.Length)
        $bw.Write([UInt32]$offset)
        $offset += $png.Bytes.Length
    }
    foreach ($png in $pngs) { $bw.Write($png.Bytes) }
} finally {
    $bw.Dispose()
    $fs.Dispose()
}

Write-Host ("  wrote {0}  ({1:N1} KB, {2} sizes)" -f $Out, ((Get-Item $Out).Length / 1KB), $pngs.Count)

# The window icon. src/main.rs embeds this PNG with include_bytes! and hands
# it to the window, so it is the taskbar and Alt-Tab picture. 512 rather than
# 256 because the compositor scales down far more gracefully than up.
$preview = [System.IO.Path]::ChangeExtension($Out, '.png')
$big = New-Icon 512
$big.Save($preview, [System.Drawing.Imaging.ImageFormat]::Png)
$big.Dispose()
Write-Host ("  wrote {0}  (512x512)" -f $preview)
