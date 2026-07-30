Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-Checked {
    param(
        [Parameter(Mandatory)]
        [string] $Label,

        [Parameter(Mandatory)]
        [scriptblock] $Command
    )

    Write-Host "CHECK ${Label}: start"
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "CHECK ${Label}: failed with exit code $LASTEXITCODE"
    }
    Write-Host "CHECK ${Label}: passed"
}

Invoke-Checked 'cargo fmt' {
    cargo fmt --all -- --check
}

Invoke-Checked 'cargo clippy' {
    cargo clippy --locked --all-targets --all-features -- -D warnings
}

Invoke-Checked 'cargo test' {
    cargo test --locked --all-features
}

Invoke-Checked 'release build' {
    cargo build --locked --release --target x86_64-pc-windows-msvc
}

$exe = Resolve-Path -LiteralPath (
    'target/x86_64-pc-windows-msvc/release/ntfy-windows-client.exe'
)

Write-Host 'CHECK foreground UI smoke: start'
$uiProcess = Start-Process `
    -FilePath $exe `
    -ArgumentList @('--smoke-test') `
    -PassThru `
    -Wait

if ($uiProcess.ExitCode -ne 0) {
    throw "Foreground UI smoke failed with exit code $($uiProcess.ExitCode)."
}
Write-Host 'CHECK foreground UI smoke: passed'

$listenerAppData = Join-Path $env:RUNNER_TEMP 'ntfy-listener-profile'
$settingsDirectory = Join-Path $listenerAppData 'ntfy-windows-client'
New-Item -ItemType Directory -Path $settingsDirectory -Force | Out-Null
$listenerTopic = "worker-$env:GITHUB_RUN_ID-$env:GITHUB_RUN_ATTEMPT"
$listenerSettings = [ordered] @{
    server_url = 'https://ntfy.sh'
    topic = $listenerTopic
    notifications_enabled = $false
    sound_enabled = $false
    audio_output = ''
    placement = 2
    auto_connect = $true
}
[IO.File]::WriteAllText(
    (Join-Path $settingsDirectory 'settings.json'),
    ($listenerSettings | ConvertTo-Json),
    [Text.UTF8Encoding]::new($false)
)

Write-Host 'CHECK active background listener working set: start'
$originalAppData = $env:APPDATA
try {
    $env:APPDATA = $listenerAppData
    $backgroundProcess = Start-Process `
        -FilePath $exe `
        -ArgumentList @('--background') `
        -PassThru
}
finally {
    $env:APPDATA = $originalAppData
}

$memorySamples = [System.Collections.Generic.List[object]]::new()
try {
    foreach ($seconds in @(6, 4, 4)) {
        Start-Sleep -Seconds $seconds

        if ($backgroundProcess.HasExited) {
            throw (
                'Background process exited before memory sampling with code ' +
                "$($backgroundProcess.ExitCode)."
            )
        }

        $sample = Get-Process -Id $backgroundProcess.Id -ErrorAction Stop
        $sample.Refresh()
        $memorySamples.Add([ordered] @{
            sampled_utc = [DateTimeOffset]::UtcNow.ToString('o')
            working_set_bytes = [int64] $sample.WorkingSet64
            private_bytes = [int64] $sample.PrivateMemorySize64
            paged_bytes = [int64] $sample.PagedMemorySize64
            virtual_bytes = [int64] $sample.VirtualMemorySize64
            handles = [int] $sample.HandleCount
            threads = [int] $sample.Threads.Count
        })
        Write-Host (
            'CHECK memory sample: working_set=' +
            $sample.WorkingSet64 +
            ' private=' +
            $sample.PrivateMemorySize64 +
            ' threads=' +
            $sample.Threads.Count
        )
    }
}
finally {
    if (-not $backgroundProcess.HasExited) {
        Stop-Process -Id $backgroundProcess.Id -Force -ErrorAction SilentlyContinue
        $backgroundProcess.WaitForExit()
    }
}

$outputDirectory = [IO.Path]::GetFullPath($env:WORKER_OUTPUT_DIRECTORY)
New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null

$lastSample = $memorySamples[$memorySamples.Count - 1]
$limitBytes = 2MB
$report = [ordered] @{
    executable = $exe.Path
    target = 'x86_64-pc-windows-msvc'
    listener_topic = $listenerTopic
    listener_configured = $true
    idle_working_set_limit_bytes = [int64] $limitBytes
    samples = $memorySamples
    final_working_set_bytes = [int64] $lastSample.working_set_bytes
    final_private_bytes = [int64] $lastSample.private_bytes
    final_thread_count = [int] $lastSample.threads
    within_idle_working_set_limit = (
        [int64] $lastSample.working_set_bytes -lt [int64] $limitBytes
    )
}

$reportPath = Join-Path $outputDirectory 'memory-report.json'
[IO.File]::WriteAllText(
    $reportPath,
    ($report | ConvertTo-Json -Depth 6),
    [Text.UTF8Encoding]::new($false)
)
Write-Host "CHECK memory report written: $reportPath"

Write-Host 'CHECK release package: start'
$stage = Join-Path $env:RUNNER_TEMP 'ntfy-package'
Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $stage -Force | Out-Null
Copy-Item -LiteralPath $exe -Destination $stage
Copy-Item -LiteralPath README.md -Destination $stage
Copy-Item -LiteralPath LICENSE -Destination $stage

$packagePath = Join-Path $outputDirectory 'ntfy-windows-client-x64.zip'
Remove-Item -LiteralPath $packagePath -Force -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $packagePath
if (-not (Test-Path -LiteralPath $packagePath -PathType Leaf)) {
    throw 'Release package was not created.'
}
Write-Host "CHECK release package: passed ($packagePath)"

if (-not $report.within_idle_working_set_limit) {
    throw (
        'Active background listener working set exceeded 2 MiB: ' +
        "$($report.final_working_set_bytes) bytes."
    )
}

Write-Host 'CHECK active background listener working set: passed'
Write-Host 'CHECK all required verification: passed'
