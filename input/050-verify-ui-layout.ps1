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

Write-Host 'CHECK UI layout release validation: start'

Invoke-Checked 'cargo fmt' {
    cargo fmt --all -- --check
}

Invoke-Checked 'cargo clippy' {
    cargo clippy --locked --all-targets --all-features -- -D warnings
}

Invoke-Checked 'cargo test' {
    cargo test --locked --all-features
}

Invoke-Checked 'release build x86_64-pc-windows-msvc' {
    cargo build --locked --release --target x86_64-pc-windows-msvc
}

$exe = Resolve-Path -LiteralPath (
    'target/x86_64-pc-windows-msvc/release/ntfy-windows-client.exe'
)
Write-Host "CHECK release executable: passed ($($exe.Path))"

$outputDirectory = [IO.Path]::GetFullPath($env:WORKER_OUTPUT_DIRECTORY)
New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null

$uiProfile = Join-Path $env:RUNNER_TEMP 'ntfy-ui-layout-profile'
$uiSettingsDirectory = Join-Path $uiProfile 'ntfy-windows-client'
Remove-Item -LiteralPath $uiProfile -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $uiSettingsDirectory -Force | Out-Null
$uiSettings = [ordered] @{
    server_url = 'https://ntfy.sh'
    topic = ''
    notifications_enabled = $true
    sound_enabled = $true
    audio_output = ''
    placement = 2
    auto_connect = $false
}
[IO.File]::WriteAllText(
    (Join-Path $uiSettingsDirectory 'settings.json'),
    ($uiSettings | ConvertTo-Json),
    [Text.UTF8Encoding]::new($false)
)

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;

public static class NtfyWindowCapture {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    public static extern IntPtr FindWindow(string className, string windowName);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool GetWindowRect(IntPtr window, out Rect rect);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool SetForegroundWindow(IntPtr window);
}
'@

Write-Host 'CHECK UI geometry screenshot: start'
$originalAppData = $env:APPDATA
$uiProcess = $null
try {
    $env:APPDATA = $uiProfile
    $uiProcess = Start-Process -FilePath $exe -PassThru
}
finally {
    $env:APPDATA = $originalAppData
}

try {
    $windowHandle = [IntPtr]::Zero
    for ($attempt = 1; $attempt -le 40; $attempt++) {
        Start-Sleep -Milliseconds 250
        if ($uiProcess.HasExited) {
            throw "UI process exited before capture with code $($uiProcess.ExitCode)."
        }
        $windowHandle = [NtfyWindowCapture]::FindWindow($null, 'ntfy for Windows')
        if ($windowHandle -ne [IntPtr]::Zero) {
            break
        }
    }

    if ($windowHandle -eq [IntPtr]::Zero) {
        throw 'Could not locate the ntfy application window.'
    }

    [NtfyWindowCapture]::SetForegroundWindow($windowHandle) | Out-Null
    Start-Sleep -Milliseconds 500

    $rect = [NtfyWindowCapture+Rect]::new()
    if (-not [NtfyWindowCapture]::GetWindowRect($windowHandle, [ref] $rect)) {
        throw 'Could not read the ntfy application window bounds.'
    }

    $width = $rect.Right - $rect.Left
    $height = $rect.Bottom - $rect.Top
    if ($width -lt 760 -or $height -lt 620) {
        throw "Unexpected UI window size: ${width}x${height}."
    }

    $bitmap = [Drawing.Bitmap]::new($width, $height)
    try {
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.CopyFromScreen(
                $rect.Left,
                $rect.Top,
                0,
                0,
                [Drawing.Size]::new($width, $height)
            )
        }
        finally {
            $graphics.Dispose()
        }

        $screenshotPath = Join-Path $outputDirectory 'ui-layout-smoke.png'
        $bitmap.Save($screenshotPath, [Drawing.Imaging.ImageFormat]::Png)

        $headerBottom = [Math]::Min(145, $height - 1)
        $bodyTop = [Math]::Min(220, $height - 1)
        $bodyBottom = [Math]::Min(520, $height - 1)
        $headerAccentPixels = 0
        $bodyAccentPixels = 0
        $contentLightPixels = 0

        for ($y = 0; $y -le $headerBottom; $y += 2) {
            for ($x = 0; $x -lt $width; $x += 2) {
                $pixel = $bitmap.GetPixel($x, $y)
                if (
                    $pixel.G -ge 145 -and
                    $pixel.G -gt ($pixel.R * 1.45) -and
                    $pixel.G -gt ($pixel.B * 1.15)
                ) {
                    $headerAccentPixels++
                }
            }
        }

        for ($y = $bodyTop; $y -le $bodyBottom; $y += 2) {
            for ($x = 0; $x -lt $width; $x += 2) {
                $pixel = $bitmap.GetPixel($x, $y)
                if (
                    $pixel.G -ge 145 -and
                    $pixel.G -gt ($pixel.R * 1.45) -and
                    $pixel.G -gt ($pixel.B * 1.15)
                ) {
                    $bodyAccentPixels++
                }
                if ($pixel.R -ge 185 -and $pixel.G -ge 185 -and $pixel.B -ge 185) {
                    $contentLightPixels++
                }
            }
        }

        if ($headerAccentPixels -lt 12) {
            throw (
                'Header branding was not detected in the top region; ' +
                "accent pixel count was $headerAccentPixels."
            )
        }

        if ($contentLightPixels -lt 30) {
            throw (
                'Expected page text and form content were not detected; ' +
                "light pixel count was $contentLightPixels."
            )
        }

        $layoutReport = [ordered] @{
            screenshot = $screenshotPath
            window_width = $width
            window_height = $height
            header_scan_bottom = $headerBottom
            body_scan_top = $bodyTop
            body_scan_bottom = $bodyBottom
            header_accent_pixels = $headerAccentPixels
            body_accent_pixels = $bodyAccentPixels
            content_light_pixels = $contentLightPixels
            header_branding_detected = $true
            content_detected = $true
        }
        [IO.File]::WriteAllText(
            (Join-Path $outputDirectory 'ui-layout-report.json'),
            ($layoutReport | ConvertTo-Json -Depth 4),
            [Text.UTF8Encoding]::new($false)
        )
        Write-Host (
            'CHECK UI geometry pixels: header_accent=' +
            $headerAccentPixels +
            ' body_accent=' +
            $bodyAccentPixels +
            ' content_light=' +
            $contentLightPixels
        )
    }
    finally {
        $bitmap.Dispose()
    }
}
finally {
    if ($null -ne $uiProcess -and -not $uiProcess.HasExited) {
        Stop-Process -Id $uiProcess.Id -Force -ErrorAction SilentlyContinue
        $uiProcess.WaitForExit()
    }
}
Write-Host 'CHECK UI geometry screenshot: passed'

Write-Host 'CHECK foreground UI smoke: start'
$originalAppData = $env:APPDATA
try {
    $env:APPDATA = $uiProfile
    $smokeProcess = Start-Process `
        -FilePath $exe `
        -ArgumentList @('--smoke-test') `
        -PassThru `
        -Wait
}
finally {
    $env:APPDATA = $originalAppData
}
if ($smokeProcess.ExitCode -ne 0) {
    throw "Foreground UI smoke failed with exit code $($smokeProcess.ExitCode)."
}
Write-Host 'CHECK foreground UI smoke: passed'

$listenerAppData = Join-Path $env:RUNNER_TEMP 'ntfy-ui-layout-listener-profile'
$settingsDirectory = Join-Path $listenerAppData 'ntfy-windows-client'
Remove-Item -LiteralPath $listenerAppData -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $settingsDirectory -Force | Out-Null
$listenerTopic = "worker-ui-$env:GITHUB_RUN_ID-$env:GITHUB_RUN_ATTEMPT"
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

$lastSample = $memorySamples[$memorySamples.Count - 1]
$limitBytes = 2MB
$memoryReport = [ordered] @{
    executable = $exe.Path
    target = 'x86_64-pc-windows-msvc'
    listener_topic = $listenerTopic
    idle_working_set_limit_bytes = [int64] $limitBytes
    samples = $memorySamples
    final_working_set_bytes = [int64] $lastSample.working_set_bytes
    final_private_bytes = [int64] $lastSample.private_bytes
    final_thread_count = [int] $lastSample.threads
    within_idle_working_set_limit = (
        [int64] $lastSample.working_set_bytes -lt [int64] $limitBytes
    )
}
[IO.File]::WriteAllText(
    (Join-Path $outputDirectory 'ui-layout-memory-report.json'),
    ($memoryReport | ConvertTo-Json -Depth 6),
    [Text.UTF8Encoding]::new($false)
)
if (-not $memoryReport.within_idle_working_set_limit) {
    throw (
        'Active background listener working set exceeded 2 MiB: ' +
        "$($memoryReport.final_working_set_bytes) bytes."
    )
}
Write-Host 'CHECK active background listener working set: passed'

Write-Host 'CHECK release package: start'
$stage = Join-Path $env:RUNNER_TEMP 'ntfy-ui-layout-package'
$expanded = Join-Path $env:RUNNER_TEMP 'ntfy-ui-layout-package-expanded'
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
$packageReport = [ordered] @{
    package = $package.FullName
    size_bytes = [int64] $package.Length
    sha256 = (Get-FileHash -LiteralPath $packagePath -Algorithm SHA256).Hash.ToLowerInvariant()
    contains = @('ntfy-windows-client.exe', 'README.md', 'LICENSE')
    source_commit = (& git rev-parse HEAD).Trim()
}
[IO.File]::WriteAllText(
    (Join-Path $outputDirectory 'ui-layout-package-report.json'),
    ($packageReport | ConvertTo-Json -Depth 4),
    [Text.UTF8Encoding]::new($false)
)
Write-Host "CHECK release package: passed ($packagePath)"
Write-Host 'CHECK UI layout release validation: passed'
