Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host 'CHECK readable inspection images: start'
Add-Type -AssemblyName System.Drawing

$outputDirectory = [IO.Path]::GetFullPath($env:WORKER_OUTPUT_DIRECTORY)
$visualReport = Get-Content `
    -LiteralPath (Join-Path $outputDirectory 'compact-ui-visual-report.json') `
    -Raw | ConvertFrom-Json

function Save-InspectionJpeg {
    param(
        [Parameter(Mandatory)][string] $SourcePath,
        [Parameter(Mandatory)][string] $TargetPath,
        [Parameter(Mandatory)][int] $TargetWidth,
        [Parameter(Mandatory)][int] $TargetHeight,
        [Parameter(Mandatory)][int64] $Quality
    )

    $source = [Drawing.Bitmap]::new($SourcePath)
    try {
        $target = [Drawing.Bitmap]::new($TargetWidth, $TargetHeight)
        try {
            $graphics = [Drawing.Graphics]::FromImage($target)
            try {
                $graphics.Clear([Drawing.Color]::FromArgb(7, 11, 17))
                $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $graphics.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
                $graphics.DrawImage($source, 0, 0, $TargetWidth, $TargetHeight)
            }
            finally {
                $graphics.Dispose()
            }

            $encoder = [Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() |
                Where-Object MimeType -eq 'image/jpeg' |
                Select-Object -First 1
            $parameters = [Drawing.Imaging.EncoderParameters]::new(1)
            try {
                $qualityParameter = [Drawing.Imaging.EncoderParameter]::new(
                    [Drawing.Imaging.Encoder]::Quality,
                    $Quality
                )
                try {
                    $parameters.Param[0] = $qualityParameter
                    $target.Save($TargetPath, $encoder, $parameters)
                }
                finally {
                    $qualityParameter.Dispose()
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
}

function Save-DesktopWindowCrop {
    param(
        [Parameter(Mandatory)][string] $DesktopPath,
        [Parameter(Mandatory)] $Window,
        [Parameter(Mandatory)][string] $TargetPath
    )

    $desktop = [Drawing.Bitmap]::new($DesktopPath)
    try {
        $rectangle = [Drawing.Rectangle]::new(
            [int] $Window.left - [int] $visualReport.desktop.left,
            [int] $Window.top - [int] $visualReport.desktop.top,
            [int] $Window.width,
            [int] $Window.height
        )
        $crop = $desktop.Clone($rectangle, $desktop.PixelFormat)
        try {
            $crop.Save($TargetPath, [Drawing.Imaging.ImageFormat]::Png)
        }
        finally {
            $crop.Dispose()
        }
    }
    finally {
        $desktop.Dispose()
    }
}

$reports = [System.Collections.Generic.List[object]]::new()
foreach ($page in $visualReport.pages) {
    $sourcePath = Join-Path $outputDirectory $page.window_crop
    $targetFile = "inspect-page-$($page.page).jpg"
    $targetPath = Join-Path $outputDirectory $targetFile
    Save-InspectionJpeg `
        -SourcePath $sourcePath `
        -TargetPath $targetPath `
        -TargetWidth 420 `
        -TargetHeight 331 `
        -Quality 24
    $file = Get-Item -LiteralPath $targetPath
    if ($file.Length -gt 16000) {
        throw "$targetFile is too large for direct inspection: $($file.Length) bytes."
    }
    $reports.Add([ordered] @{
        kind = 'page'
        name = $page.page
        file = $targetFile
        width = 420
        height = 331
        size_bytes = [int64] $file.Length
        sha256 = (Get-FileHash -LiteralPath $targetPath -Algorithm SHA256).Hash.ToLowerInvariant()
    })
    Write-Host "CHECK readable page image: $targetFile ($($file.Length) bytes)"
}

foreach ($popupName in @('short', 'long')) {
    $window = if ($popupName -eq 'short') {
        $visualReport.popup_short
    } else {
        $visualReport.popup_long
    }
    $desktopPath = Join-Path $outputDirectory "desktop-popup-$popupName.png"
    $temporaryCrop = Join-Path $env:RUNNER_TEMP "popup-$popupName-crop.png"
    Save-DesktopWindowCrop `
        -DesktopPath $desktopPath `
        -Window $window `
        -TargetPath $temporaryCrop
    $targetFile = "inspect-popup-$popupName.jpg"
    $targetPath = Join-Path $outputDirectory $targetFile
    $targetHeight = if ($popupName -eq 'short') { 112 } else { 248 }
    Save-InspectionJpeg `
        -SourcePath $temporaryCrop `
        -TargetPath $targetPath `
        -TargetWidth 380 `
        -TargetHeight $targetHeight `
        -Quality 30
    $file = Get-Item -LiteralPath $targetPath
    if ($file.Length -gt 16000) {
        throw "$targetFile is too large for direct inspection: $($file.Length) bytes."
    }
    $reports.Add([ordered] @{
        kind = 'popup'
        name = $popupName
        file = $targetFile
        width = 380
        height = $targetHeight
        size_bytes = [int64] $file.Length
        sha256 = (Get-FileHash -LiteralPath $targetPath -Algorithm SHA256).Hash.ToLowerInvariant()
    })
    Write-Host "CHECK readable popup image: $targetFile ($($file.Length) bytes)"
}

[IO.File]::WriteAllText(
    (Join-Path $outputDirectory 'readable-inspection-report.json'),
    ($reports | ConvertTo-Json -Depth 5),
    [Text.UTF8Encoding]::new($false)
)
Write-Host 'CHECK readable inspection images: passed'
