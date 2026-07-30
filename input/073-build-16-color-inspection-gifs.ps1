Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host 'CHECK 16-color inspection GIFs: start'
Add-Type -AssemblyName System.Drawing

$outputDirectory = [IO.Path]::GetFullPath($env:WORKER_OUTPUT_DIRECTORY)
$items = @(
    [ordered] @{ source = 'inspect-page-connection.jpg'; target = 'inspect16-page-connection.gif'; width = 300; height = 236 },
    [ordered] @{ source = 'inspect-page-notifications.jpg'; target = 'inspect16-page-notifications.gif'; width = 300; height = 236 },
    [ordered] @{ source = 'inspect-page-publish.jpg'; target = 'inspect16-page-publish.gif'; width = 300; height = 236 },
    [ordered] @{ source = 'inspect-page-history.jpg'; target = 'inspect16-page-history.gif'; width = 300; height = 236 },
    [ordered] @{ source = 'inspect-popup-short.jpg'; target = 'inspect16-popup-short.gif'; width = 300; height = 88 },
    [ordered] @{ source = 'inspect-popup-long.jpg'; target = 'inspect16-popup-long.gif'; width = 300; height = 196 }
)

$report = [System.Collections.Generic.List[object]]::new()
foreach ($item in $items) {
    $sourcePath = Join-Path $outputDirectory $item.source
    $targetPath = Join-Path $outputDirectory $item.target
    $source = [Drawing.Bitmap]::new($sourcePath)
    try {
        $resized = [Drawing.Bitmap]::new([int] $item.width, [int] $item.height)
        try {
            $graphics = [Drawing.Graphics]::FromImage($resized)
            try {
                $graphics.Clear([Drawing.Color]::FromArgb(7, 11, 17))
                $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBilinear
                $graphics.DrawImage($source, 0, 0, [int] $item.width, [int] $item.height)
            }
            finally {
                $graphics.Dispose()
            }

            $rectangle = [Drawing.Rectangle]::new(0, 0, [int] $item.width, [int] $item.height)
            $indexed = $resized.Clone(
                $rectangle,
                [Drawing.Imaging.PixelFormat]::Format4bppIndexed
            )
            try {
                $indexed.Save($targetPath, [Drawing.Imaging.ImageFormat]::Gif)
            }
            finally {
                $indexed.Dispose()
            }
        }
        finally {
            $resized.Dispose()
        }
    }
    finally {
        $source.Dispose()
    }

    $file = Get-Item -LiteralPath $targetPath
    if ($file.Length -gt 14000) {
        throw "$($item.target) is too large for direct retrieval: $($file.Length) bytes."
    }
    $report.Add([ordered] @{
        file = $item.target
        width = [int] $item.width
        height = [int] $item.height
        size_bytes = [int64] $file.Length
        sha256 = (Get-FileHash -LiteralPath $targetPath -Algorithm SHA256).Hash.ToLowerInvariant()
        source = $item.source
        palette = '16-color'
    })
    Write-Host "CHECK 16-color image: $($item.target) ($($file.Length) bytes)"
}

[IO.File]::WriteAllText(
    (Join-Path $outputDirectory 'inspection16-report.json'),
    ($report | ConvertTo-Json -Depth 5),
    [Text.UTF8Encoding]::new($false)
)
Write-Host 'CHECK 16-color inspection GIFs: passed'
