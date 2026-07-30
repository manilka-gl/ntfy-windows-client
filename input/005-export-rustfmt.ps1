Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host 'CHECK rustfmt export: start'
cargo fmt --all
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$destinationRoot = Join-Path $env:WORKER_OUTPUT_DIRECTORY 'formatted-src'
New-Item -ItemType Directory -Path $destinationRoot -Force | Out-Null

foreach ($sourcePath in @(
    'src/main.rs',
    'src/notification.rs',
    'src/memory.rs'
)) {
    $destination = Join-Path $destinationRoot $sourcePath
    $parent = Split-Path -Parent $destination
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
    Copy-Item -LiteralPath $sourcePath -Destination $destination -Force
    Write-Host "CHECK exported: $sourcePath"
}

Write-Host 'CHECK rustfmt export: passed'
