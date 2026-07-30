Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host 'CHECK tiny visual inspection contact sheet: start'
Add-Type -AssemblyName System.Drawing

$outputDirectory = [IO.Path]::GetFullPath($env:WORKER_OUTPUT_DIRECTORY)
$sourcePath = Join-Path $outputDirectory 'visual-window-crops-contact-sheet.jpg'
if (-not (Test-Path -LiteralPath $sourcePath)) {
    throw 'visual-window-crops-contact-sheet.jpg is missing.'
}

$source = [Drawing.Bitmap]::new($sourcePath)
try {
    $targetWidth = 500
    $targetHeight = [int] [Math]::Round($source.Height * ($targetWidth / [double] $source.Width))
    $target = [Drawing.Bitmap]::new($targetWidth, $targetHeight)
    try {
        $graphics = [Drawing.Graphics]::FromImage($target)
        try {
            $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $graphics.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
            $graphics.DrawImage($source, 0, 0, $targetWidth, $targetHeight)
        }
        finally {
            $graphics.Dispose()
        }

        $targetPath = Join-Path $outputDirectory 'visual-contact-tiny.jpg'
        $encoder = [Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() |
            Where-Object MimeType -eq 'image/jpeg' |
            Select-Object -First 1
        $parameters = [Drawing.Imaging.EncoderParameters]::new(1)
        try {
            $quality = [Drawing.Imaging.EncoderParameter]::new(
                [Drawing.Imaging.Encoder]::Quality,
                [int64] 28
            )
            try {
                $parameters.Param[0] = $quality
                $target.Save($targetPath, $encoder, $parameters)
            }
            finally {
                $quality.Dispose()
            }
        }
        finally {
            $parameters.Dispose()
        }
    }
    finally {
        $target.Dispose()
    }
}
finally {
    $source.Dispose()
}

$file = Get-Item -LiteralPath (Join-Path $outputDirectory 'visual-contact-tiny.jpg')
if ($file.Length -gt 35000) {
    throw "Tiny contact sheet is still too large: $($file.Length) bytes."
}
$hash = Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256
$report = [ordered] @{
    file = $file.Name
    width = 500
    height = $targetHeight
    size_bytes = [int64] $file.Length
    sha256 = $hash.Hash.ToLowerInvariant()
    source = 'visual-window-crops-contact-sheet.jpg'
}
[IO.File]::WriteAllText(
    (Join-Path $outputDirectory 'visual-contact-tiny-report.json'),
    ($report | ConvertTo-Json -Depth 4),
    [Text.UTF8Encoding]::new($false)
)
Write-Host "CHECK tiny visual inspection contact sheet: passed ($($file.Length) bytes)"
