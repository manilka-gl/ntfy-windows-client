Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host 'CHECK final package: start'

cargo fmt --all -- --check
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
Write-Host 'CHECK final package source format: passed'

$exe = Join-Path $PWD 'target/x86_64-pc-windows-msvc/release/ntfy-windows-client.exe'
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
    cargo build --locked --release --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}
Write-Host 'CHECK final package executable: present'

$outputDirectory = [IO.Path]::GetFullPath($env:WORKER_OUTPUT_DIRECTORY)
New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null

$stage = Join-Path $env:RUNNER_TEMP 'ntfy-final-package'
$expanded = Join-Path $env:RUNNER_TEMP 'ntfy-final-package-expanded'
Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $expanded -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $stage -Force | Out-Null

Copy-Item -LiteralPath $exe -Destination $stage
Copy-Item -LiteralPath README.md -Destination $stage
Copy-Item -LiteralPath LICENSE -Destination $stage

$packagePath = Join-Path $outputDirectory 'ntfy-windows-client-x64.zip'
Remove-Item -LiteralPath $packagePath -Force -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $packagePath

Expand-Archive -LiteralPath $packagePath -DestinationPath $expanded
foreach ($fileName in @('ntfy-windows-client.exe', 'README.md', 'LICENSE')) {
    $sourcePath = Join-Path $stage $fileName
    $archivePath = Join-Path $expanded $fileName
    if (-not (Test-Path -LiteralPath $archivePath -PathType Leaf)) {
        throw "Package is missing $fileName."
    }

    $sourceHash = (Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash
    $archiveHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash
    if ($sourceHash -ne $archiveHash) {
        throw "Packaged $fileName does not match the source file."
    }
    Write-Host "CHECK packaged file: $fileName"
}

$package = Get-Item -LiteralPath $packagePath
$report = [ordered] @{
    package = $package.FullName
    size_bytes = [int64] $package.Length
    sha256 = (Get-FileHash -LiteralPath $packagePath -Algorithm SHA256).Hash.ToLowerInvariant()
    contains = @('ntfy-windows-client.exe', 'README.md', 'LICENSE')
    source_commit = (& git rev-parse HEAD).Trim()
}

$reportPath = Join-Path $outputDirectory 'final-package-report.json'
[IO.File]::WriteAllText(
    $reportPath,
    ($report | ConvertTo-Json -Depth 4),
    [Text.UTF8Encoding]::new($false)
)

Write-Host "CHECK final package report: $reportPath"
Write-Host 'CHECK final package: passed'
