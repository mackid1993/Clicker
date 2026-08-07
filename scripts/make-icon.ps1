<#
.SYNOPSIS
    Draw the RustDVR icon and pack it into assets\rustdvr.ico.

.DESCRIPTION
    A TV remote, seen straight on: a rounded body, an accent-colored power
    button at the top, and a directional pad below it.

    Drawn rather than shipped as a binary asset so it can be adjusted, and so
    the repository carries the recipe instead of an opaque file nobody can
    edit.

    The design is constrained by the smallest size it has to survive. At 16px a
    remote is about ten pixels wide, which is room for a silhouette, one bright
    dot and one lighter shape — nothing more. So detail is added with size
    rather than drawn once and scaled down: below 32px the d-pad and the lower
    buttons are dropped entirely and only the body and the power button remain,
    which is what keeps it readable in a taskbar.
#>

[CmdletBinding()]
param(
    [string]$Out = (Join-Path (Split-Path -Parent $PSScriptRoot) 'assets\rustdvr.ico')
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

# Fluent's palette, matching src/theme.rs.
$Body      = [System.Drawing.Color]::FromArgb(255, 38, 41, 49)
$BodyEdge  = [System.Drawing.Color]::FromArgb(255, 74, 79, 92)
$Accent    = [System.Drawing.Color]::FromArgb(255, 96, 165, 250)
$Button    = [System.Drawing.Color]::FromArgb(255, 120, 126, 140)
$Highlight = [System.Drawing.Color]::FromArgb(34, 255, 255, 255)

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

function New-Icon([int]$size) {
    $bmp = New-Object System.Drawing.Bitmap $size, $size,
        ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.Clear([System.Drawing.Color]::Transparent)

    $s = [float]$size
    $detailed = $size -ge 32

    # The remote leans a few degrees, which is the difference between a
    # product mark and a diagram. Small sizes stay upright: a tilt at 16px
    # only blurs the silhouette.
    if ($detailed) {
        $g.TranslateTransform($s / 2, $s / 2)
        $g.RotateTransform(-10)
        $g.TranslateTransform(-$s / 2, -$s / 2)
    }

    # Slimmer proportions than before. The old body was nearly half the canvas
    # wide, which is a candy bar, not a remote.
    $w = $s * 0.38
    $h = $s * 0.84
    $x = ($s - $w) / 2
    $y = ($s - $h) / 2
    $radius = [Math]::Max(1.5, $w * 0.30)

    # A soft shadow under the body, so it sits above the canvas instead of
    # being a sticker on it.
    if ($detailed) {
        $shadow = New-RoundedPath ($x + $s * 0.015) ($y + $s * 0.03) $w $h $radius
        $shadowBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(55, 0, 0, 0))
        $g.FillPath($shadowBrush, $shadow)
        $shadow.Dispose(); $shadowBrush.Dispose()
    }

    # The body is a vertical gradient, lit from above. Flat fill was most of
    # what made the first attempt look like clip art.
    $bodyPath = New-RoundedPath $x $y $w $h $radius
    $gradTop = [System.Drawing.Color]::FromArgb(255, 62, 68, 82)
    $gradBottom = [System.Drawing.Color]::FromArgb(255, 30, 33, 41)
    $bodyBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.PointF($x, $y)),
        (New-Object System.Drawing.PointF($x, ($y + $h))),
        $gradTop, $gradBottom)
    $g.FillPath($bodyBrush, $bodyPath)
    $bodyBrush.Dispose()

    if ($detailed) {
        # A hairline lit edge along the top of the body.
        $edgePen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(70, 255, 255, 255)), ([Math]::Max(1.0, $s / 128))
        $g.DrawPath($edgePen, $bodyPath)
        $edgePen.Dispose()
    }
    $bodyPath.Dispose()

    # The power button: the one element that must survive every size. Smaller
    # than before — a third of the body width, not half — with its own subtle
    # gradient so it reads as a lens of light, not a flat blue spot.
    $power = $w * (0.34, 0.44)[$size -lt 32]
    $px = $x + ($w - $power) / 2
    $py = $y + $h * 0.11
    $powerRect = New-Object System.Drawing.RectangleF($px, $py, $power, $power)
    $powerBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.PointF($px, $py)),
        (New-Object System.Drawing.PointF($px, ($py + $power))),
        [System.Drawing.Color]::FromArgb(255, 140, 190, 255),
        [System.Drawing.Color]::FromArgb(255, 76, 140, 235))
    $g.FillEllipse($powerBrush, $powerRect)
    $powerBrush.Dispose()

    if ($detailed) {
        $buttonBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 108, 114, 128))
        $dimBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 84, 90, 103))

        # A volume pill down the middle, then two pairs of dots. Five muted
        # shapes in a familiar arrangement — enough to say "remote", few
        # enough to stay clean at 48px.
        $pillW = $w * 0.30
        $pillH = $h * 0.24
        $pill = New-RoundedPath ($x + ($w - $pillW) / 2) ($y + $h * 0.34) $pillW $pillH ($pillW / 2)
        $g.FillPath($dimBrush, $pill)
        $pill.Dispose()

        $bw = $w * 0.17
        $gap = $w * 0.24
        foreach ($row in 0, 1) {
            foreach ($col in 0, 1) {
                $bx = $x + ($w - ($bw * 2 + $gap)) / 2 + $col * ($bw + $gap)
                $by = $y + $h * (0.68 + $row * 0.12)
                $g.FillEllipse($buttonBrush, $bx, $by, $bw, $bw)
            }
        }
        $buttonBrush.Dispose(); $dimBrush.Dispose()
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

# A large PNG as well, for a README or a store listing.
$preview = [System.IO.Path]::ChangeExtension($Out, '.png')
$big = New-Icon 256
$big.Save($preview, [System.Drawing.Imaging.ImageFormat]::Png)
$big.Dispose()
Write-Host ("  wrote {0}" -f $preview)
