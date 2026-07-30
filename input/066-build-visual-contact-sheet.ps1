Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host 'CHECK visual inspection contact sheet: start'
Add-Type -AssemblyName System.Drawing

$outputDirectory = [IO.Path]::GetFullPath($env:WORKER_OUTPUT_DIRECTORY)
$reportPath = Join-Path $outputDirectory 'compact-ui-visual-report.json'
if (-not (Test-Path -LiteralPath $reportPath)) {
    throw 'compact-ui-visual-report.json is missing.'
}
$report = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json

$items = @(
    [ordered] @{ label = 'CONNECTION · minimum window'; file = 'desktop-page-connection.png'; rect = $report.pages[0].window },
    [ordered] @{ label = 'NOTIFICATIONS · minimum window'; file = 'desktop-page-notifications.png'; rect = $report.pages[1].window },
    [ordered] @{ label = 'PUBLISH · minimum window'; file = 'desktop-page-publish.png'; rect = $report.pages[2].window },
    [ordered] @{ label = 'HISTORY · minimum window'; file = 'desktop-page-history.png'; rect = $report.pages[3].window },
    [ordered] @{ label = 'POPUP · short body'; file = 'desktop-popup-short.png'; rect = $report.popup_short },
    [ordered] @{ label = 'POPUP · long body'; file = 'desktop-popup-long.png'; rect = $report.popup_long }
)

$cellWidth = 500
$cellHeight = 390
$labelHeight = 30
$sheet = [Drawing.Bitmap]::new($cellWidth * 2, $cellHeight * 3)
try {
    $graphics = [Drawing.Graphics]::FromImage($sheet)
    try {
        $graphics.Clear([Drawing.Color]::FromArgb(8, 12, 18))
        $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::HighQuality
        $labelBrush = [Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(20, 31, 44))
        $textBrush = [Drawing.SolidBrush]::new([Drawing.Color]::White)
        $borderPen = [Drawing.Pen]::new([Drawing.Color]::FromArgb(53, 242, 161), 2)
        $font = [Drawing.Font]::new('Segoe UI', 11, [Drawing.FontStyle]::Bold)
        try {
            for ($index = 0; $index -lt $items.Count; $index++) {
                $column = $index % 2
                $row = [int] [Math]::Floor($index / 2)
                $cellX = $column * $cellWidth
                $cellY = $row * $cellHeight
                $graphics.FillRectangle($labelBrush, $cellX, $cellY, $cellWidth, $labelHeight)
                $graphics.DrawString(
                    $items[$index].label,
                    $font,
                    $textBrush,
                    [Drawing.RectangleF]::new($cellX + 10, $cellY + 5, $cellWidth - 20, 22)
                )

                $sourcePath = Join-Path $outputDirectory $items[$index].file
                if (-not (Test-Path -LiteralPath $sourcePath)) {
                    throw "Missing screenshot $sourcePath"
                }
                $source = [Drawing.Bitmap]::new($sourcePath)
                try {
                    $rect = $items[$index].rect
                    $cropRect = [Drawing.Rectangle]::new(
                        [int] $rect.left,
                        [int] $rect.top,
                        [int] $rect.width,
                        [int] $rect.height
                    )
                    $crop = $source.Clone($cropRect, $source.PixelFormat)
                    try {
                        $availableWidth = $cellWidth - 16
                        $availableHeight = $cellHeight - $labelHeight - 16
                        $scale = [Math]::Min(
                            $availableWidth / [double] $crop.Width,
                            $availableHeight / [double] $crop.Height
                        )
                        if ($index -ge 4) {
                            $scale = [Math]::Min(1.25, $scale)
                        }
                        $drawWidth = [Math]::Max(1, [int] [Math]::Round($crop.Width * $scale))
                        $drawHeight = [Math]::Max(1, [int] [Math]::Round($crop.Height * $scale))
                        $drawX = $cellX + [int] (($cellWidth - $drawWidth) / 2)
                        $drawY = $cellY + $labelHeight + [int] (($cellHeight - $labelHeight - $drawHeight) / 2)
                        $graphics.DrawImage(
                            $crop,
                            [Drawing.Rectangle]::new($drawX, $drawY, $drawWidth, $drawHeight),
                            0,
                            0,
                            $crop.Width,
                            $crop.Height,
                            [Drawing.GraphicsUnit]::Pixel
                        )
                        $graphics.DrawRectangle(
                            $borderPen,
                            $drawX,
                            $drawY,
                            $drawWidth - 1,
                            $drawHeight - 1
                        )
                    }
                    finally {
                        $crop.Dispose()
                    }
                }
                finally {
                    $source.Dispose()
                }
            }
        }
        finally {
            $labelBrush.Dispose()
            $textBrush.Dispose()
            $borderPen.Dispose()
            $font.Dispose()
        }
    }
    finally {
        $graphics.Dispose()
    }

    $contactPath = Join-Path $outputDirectory 'visual-window-crops-contact-sheet.jpg'
    $encoder = [Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() |
        Where-Object MimeType -eq 'image/jpeg' |
        Select-Object -First 1
    $encoderParameters = [Drawing.Imaging.EncoderParameters]::new(1)
    try {
        $quality = [Drawing.Imaging.EncoderParameter]::new(
            [Drawing.Imaging.Encoder]::Quality,
            [int64] 55
        )
        try {
            $encoderParameters.Param[0] = $quality
            $sheet.Save($contactPath, $encoder, $encoderParameters)
        }
        finally {
            $quality.Dispose()
        }
    }
    finally {
        $encoderParameters.Dispose()
    }
}
finally {
    $sheet.Dispose()
}

$contactFile = Get-Item -LiteralPath (Join-Path $outputDirectory 'visual-window-crops-contact-sheet.jpg')
$contactHash = Get-FileHash -LiteralPath $contactFile.FullName -Algorithm SHA256
$summary = [ordered] @{
    file = $contactFile.Name
    width = 1000
    height = 1170
    size_bytes = [int64] $contactFile.Length
    sha256 = $contactHash.Hash.ToLowerInvariant()
    sources = @($items | ForEach-Object file)
}
[IO.File]::WriteAllText(
    (Join-Path $outputDirectory 'visual-contact-sheet-report.json'),
    ($summary | ConvertTo-Json -Depth 5),
    [Text.UTF8Encoding]::new($false)
)
Write-Host "CHECK visual inspection contact sheet: passed ($($contactFile.Length) bytes)"
