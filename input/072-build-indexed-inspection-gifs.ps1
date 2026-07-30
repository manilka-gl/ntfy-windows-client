Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host 'CHECK indexed inspection GIFs: start'
Add-Type -AssemblyName System.Drawing

$outputDirectory = [IO.Path]::GetFullPath($env:WORKER_OUTPUT_DIRECTORY)
$items = @(
    [ordered] @{ source = 'inspect-page-connection.jpg'; target = 'inspect-page-connection.gif'; width = 330; height = 260 },
    [ordered] @{ source = 'inspect-page-notifications.jpg'; target = 'inspect-page-notifications.gif'; width = 330; height = 260 },
    [ordered] @{ source = 'inspect-page-publish.jpg'; target = 'inspect-page-publish.gif'; width = 330; height = 260 },
    [ordered] @{ source = 'inspect-page-history.jpg'; target = 'inspect-page-history.gif'; width = 330; height = 260 },
    [ordered] @{ source = 'inspect-popup-short.jpg'; target = 'inspect-popup-short.gif'; width = 330; height = 97 },
    [ordered] @{ source = 'inspect-popup-long.jpg'; target = 'inspect-popup-long.gif'; width = 330; height = 215 }
)

$report = [System.Collections.Generic.List[object]]::new()
foreach ($item in $items) {
    $sourcePath = Join-Path $outputDirectory $item.source
    $targetPath = Join-Path $outputDirectory $item.target
    if (-not (Test-Path -LiteralPath $sourcePath)) {
        throw "Missing inspection source $sourcePath"
    }

    $source = [Drawing.Bitmap]::new($sourcePath)
    try {
        $target = [Drawing.Bitmap]::new([int] $item.width, [int] $item.height)
        try {
            $graphics = [Drawing.Graphics]::FromImage($target)
            try {
                $graphics.Clear([Drawing.Color]::FromArgb(7, 11, 17))
                $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBilinear
                $graphics.DrawImage($source, 0, 0, [int] $item.width, [int] $item.height)
            }
            finally {
                $graphics.Dispose()
            }
            $target.Save($targetPath, [Drawing.Imaging.ImageFormat]::Gif)
        }
        finally {
            $target.Dispose()
        }
    }
    finally {
        $source.Dispose()
    }

    $file = Get-Item -LiteralPath $targetPath
    if ($file.Length -gt 12000) {
        throw "$($item.target) is too large for direct retrieval: $($file.Length) bytes."
    }
    $report.Add([ordered] @{
        file = $item.target
        width = [int] $item.width
        height = [int] $item.height
        size_bytes = [int64] $file.Length
        sha256 = (Get-FileHash -LiteralPath $targetPath -Algorithm SHA256).Hash.ToLowerInvariant()
        source = $item.source
    })
    Write-Host "CHECK indexed image: $($item.target) ($($file.Length) bytes)"
}

[IO.File]::WriteAllText(
    (Join-Path $outputDirectory 'indexed-inspection-report.json'),
    ($report | ConvertTo-Json -Depth 5),
    [Text.UTF8Encoding]::new($false)
)
Write-Host 'CHECK indexed inspection GIFs: passed'
