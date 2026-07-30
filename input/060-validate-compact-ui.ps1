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

function Write-SettingsProfile {
    param(
        [Parameter(Mandatory)]
        [string] $Profile,

        [Parameter(Mandatory)]
        [hashtable] $Settings
    )

    $settingsDirectory = Join-Path $Profile 'ntfy-windows-client'
    Remove-Item -LiteralPath $Profile -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Path $settingsDirectory -Force | Out-Null
    [IO.File]::WriteAllText(
        (Join-Path $settingsDirectory 'settings.json'),
        ($Settings | ConvertTo-Json),
        [Text.UTF8Encoding]::new($false)
    )
}

function Start-NtfyProcess {
    param(
        [Parameter(Mandatory)]
        [string] $Executable,

        [Parameter(Mandatory)]
        [string] $Profile,

        [string[]] $Arguments = @()
    )

    $originalAppData = $env:APPDATA
    try {
        $env:APPDATA = $Profile
        return Start-Process -FilePath $Executable -ArgumentList $Arguments -PassThru
    }
    finally {
        $env:APPDATA = $originalAppData
    }
}

Write-Host 'CHECK compact UI release validation: start'

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

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public static class NtfyVisualNative {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct Point {
        public int X;
        public int Y;
    }

    private delegate bool EnumWindowsProc(IntPtr window, IntPtr parameter);

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr parameter);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool IsWindowVisible(IntPtr window);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool GetWindowRect(IntPtr window, out Rect rect);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool GetClientRect(IntPtr window, out Rect rect);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool ClientToScreen(IntPtr window, ref Point point);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool SetWindowPos(
        IntPtr window,
        IntPtr insertAfter,
        int x,
        int y,
        int width,
        int height,
        uint flags
    );

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool SetForegroundWindow(IntPtr window);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll")]
    public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extraInfo);

    public static IntPtr[] VisibleWindowsForProcess(uint processId) {
        var result = new List<IntPtr>();
        EnumWindows((window, parameter) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (owner == processId && IsWindowVisible(window)) {
                Rect rect;
                if (GetWindowRect(window, out rect) && rect.Right > rect.Left && rect.Bottom > rect.Top) {
                    result.Add(window);
                }
            }
            return true;
        }, IntPtr.Zero);
        return result.ToArray();
    }
}
'@

function Get-WindowRectRecord {
    param([Parameter(Mandatory)][IntPtr] $Handle)

    $rect = [NtfyVisualNative+Rect]::new()
    if (-not [NtfyVisualNative]::GetWindowRect($Handle, [ref] $rect)) {
        throw "Could not read window bounds for handle $Handle."
    }

    return [ordered] @{
        handle = [int64] $Handle
        left = [int] $rect.Left
        top = [int] $rect.Top
        right = [int] $rect.Right
        bottom = [int] $rect.Bottom
        width = [int] ($rect.Right - $rect.Left)
        height = [int] ($rect.Bottom - $rect.Top)
    }
}

function Get-LargestVisibleWindow {
    param([Parameter(Mandatory)][Diagnostics.Process] $Process)

    $bestHandle = [IntPtr]::Zero
    $bestArea = 0L
    foreach ($handle in [NtfyVisualNative]::VisibleWindowsForProcess([uint32] $Process.Id)) {
        $record = Get-WindowRectRecord -Handle $handle
        $area = [int64] $record.width * [int64] $record.height
        if ($area -gt $bestArea) {
            $bestArea = $area
            $bestHandle = $handle
        }
    }
    return $bestHandle
}

function Wait-ForLargestVisibleWindow {
    param(
        [Parameter(Mandatory)][Diagnostics.Process] $Process,
        [int] $Attempts = 80,
        [int] $DelayMilliseconds = 250
    )

    for ($attempt = 1; $attempt -le $Attempts; $attempt++) {
        Start-Sleep -Milliseconds $DelayMilliseconds
        $Process.Refresh()
        if ($Process.HasExited) {
            throw "Process $($Process.Id) exited before a visible window appeared with code $($Process.ExitCode)."
        }
        $handle = Get-LargestVisibleWindow -Process $Process
        if ($handle -ne [IntPtr]::Zero) {
            return $handle
        }
    }
    throw "No visible window appeared for process $($Process.Id)."
}

function Assert-WindowInsideWorkArea {
    param(
        [Parameter(Mandatory)][hashtable] $Window,
        [Parameter(Mandatory)][Drawing.Rectangle] $WorkArea,
        [Parameter(Mandatory)][string] $Label
    )

    $inside = (
        $Window.left -ge $WorkArea.Left -and
        $Window.top -ge $WorkArea.Top -and
        $Window.right -le $WorkArea.Right -and
        $Window.bottom -le $WorkArea.Bottom
    )
    if (-not $inside) {
        throw (
            "$Label is outside the Windows work area. " +
            "Window=[$($Window.left),$($Window.top),$($Window.right),$($Window.bottom)] " +
            "WorkArea=[$($WorkArea.Left),$($WorkArea.Top),$($WorkArea.Right),$($WorkArea.Bottom)]"
        )
    }
}

function Capture-AnnotatedDesktop {
    param(
        [Parameter(Mandatory)][string] $FileName,
        [Parameter(Mandatory)][hashtable] $Window,
        [Parameter(Mandatory)][string] $Label,
        [Parameter(Mandatory)][Drawing.Rectangle] $DesktopBounds,
        [Parameter(Mandatory)][Drawing.Rectangle] $WorkArea
    )

    $path = Join-Path $outputDirectory $FileName
    $bitmap = [Drawing.Bitmap]::new($DesktopBounds.Width, $DesktopBounds.Height)
    try {
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.CopyFromScreen(
                $DesktopBounds.Left,
                $DesktopBounds.Top,
                0,
                0,
                [Drawing.Size]::new($DesktopBounds.Width, $DesktopBounds.Height)
            )

            $workPen = [Drawing.Pen]::new([Drawing.Color]::LimeGreen, 3)
            $windowPen = [Drawing.Pen]::new([Drawing.Color]::DeepSkyBlue, 4)
            $labelBrush = [Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(220, 0, 0, 0))
            $textBrush = [Drawing.SolidBrush]::new([Drawing.Color]::White)
            $font = [Drawing.Font]::new('Segoe UI', 11, [Drawing.FontStyle]::Bold)
            try {
                $workRect = [Drawing.Rectangle]::new(
                    $WorkArea.Left - $DesktopBounds.Left,
                    $WorkArea.Top - $DesktopBounds.Top,
                    $WorkArea.Width - 1,
                    $WorkArea.Height - 1
                )
                $windowRect = [Drawing.Rectangle]::new(
                    $Window.left - $DesktopBounds.Left,
                    $Window.top - $DesktopBounds.Top,
                    $Window.width - 1,
                    $Window.height - 1
                )
                $graphics.DrawRectangle($workPen, $workRect)
                $graphics.DrawRectangle($windowPen, $windowRect)

                $annotation = (
                    "$Label`n" +
                    "desktop $($DesktopBounds.Width)x$($DesktopBounds.Height)  " +
                    "work $($WorkArea.Width)x$($WorkArea.Height)  " +
                    "window $($Window.width)x$($Window.height)`n" +
                    "green = usable Windows work area   cyan = tested window   result = CONTAINED"
                )
                $labelWidth = [Math]::Min(760, $DesktopBounds.Width - 24)
                $labelRect = [Drawing.RectangleF]::new(12, 12, $labelWidth, 64)
                $graphics.FillRectangle($labelBrush, $labelRect)
                $graphics.DrawString($annotation, $font, $textBrush, $labelRect)
            }
            finally {
                $workPen.Dispose()
                $windowPen.Dispose()
                $labelBrush.Dispose()
                $textBrush.Dispose()
                $font.Dispose()
            }
        }
        finally {
            $graphics.Dispose()
        }
        $bitmap.Save($path, [Drawing.Imaging.ImageFormat]::Png)
    }
    finally {
        $bitmap.Dispose()
    }
    Write-Host "CHECK annotated desktop: $FileName"
    return $path
}

function Click-ApplicationTab {
    param(
        [Parameter(Mandatory)][IntPtr] $Handle,
        [Parameter(Mandatory)][ValidateRange(0, 3)][int] $Index
    )

    [NtfyVisualNative]::SetForegroundWindow($Handle) | Out-Null
    Start-Sleep -Milliseconds 250

    $client = [NtfyVisualNative+Rect]::new()
    if (-not [NtfyVisualNative]::GetClientRect($Handle, [ref] $client)) {
        throw 'Could not read the application client rectangle.'
    }
    $origin = [NtfyVisualNative+Point]::new()
    if (-not [NtfyVisualNative]::ClientToScreen($Handle, [ref] $origin)) {
        throw 'Could not translate the application client origin.'
    }

    $clientWidth = $client.Right - $client.Left
    $available = $clientWidth - 36
    $x = $origin.X + 18 + [int] (($available / 4.0) * ($Index + 0.5))
    $y = $origin.Y + 81

    [NtfyVisualNative]::SetCursorPos($x, $y) | Out-Null
    [NtfyVisualNative]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    [NtfyVisualNative]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 450
}

function Set-TestWindowSize {
    param(
        [Parameter(Mandatory)][IntPtr] $Handle,
        [Parameter(Mandatory)][int] $Width,
        [Parameter(Mandatory)][int] $Height,
        [Parameter(Mandatory)][Drawing.Rectangle] $WorkArea
    )

    $x = $WorkArea.Left + [int] (($WorkArea.Width - $Width) / 2)
    $y = $WorkArea.Top + [int] (($WorkArea.Height - $Height) / 2)
    $flags = 0x0004 -bor 0x0040
    if (-not [NtfyVisualNative]::SetWindowPos(
        $Handle,
        [IntPtr]::Zero,
        $x,
        $y,
        $Width,
        $Height,
        $flags
    )) {
        throw "Could not resize the UI window to ${Width}x${Height}."
    }
    Start-Sleep -Milliseconds 500
}

$primaryScreen = [Windows.Forms.Screen]::PrimaryScreen
$desktopBounds = $primaryScreen.Bounds
$workArea = $primaryScreen.WorkingArea
Write-Host (
    'CHECK desktop geometry: desktop=' +
    "$($desktopBounds.Width)x$($desktopBounds.Height) " +
    'work=' +
    "$($workArea.Width)x$($workArea.Height)"
)

$uiProfile = Join-Path $env:RUNNER_TEMP 'ntfy-compact-ui-profile'
Write-SettingsProfile -Profile $uiProfile -Settings ([ordered] @{
    server_url = 'https://ntfy.sh'
    topic = ''
    notifications_enabled = $true
    sound_enabled = $false
    audio_output = ''
    placement = 2
    auto_connect = $false
})

Write-Host 'CHECK full-desktop application geometry: start'
$uiProcess = Start-NtfyProcess -Executable $exe.Path -Profile $uiProfile
try {
    $uiHandle = Wait-ForLargestVisibleWindow -Process $uiProcess
    [NtfyVisualNative]::SetForegroundWindow($uiHandle) | Out-Null
    Start-Sleep -Milliseconds 500

    $preferredRect = Get-WindowRectRecord -Handle $uiHandle
    Assert-WindowInsideWorkArea -Window $preferredRect -WorkArea $workArea -Label 'Preferred application window'
    $preferredWidthRatio = $preferredRect.width / [double] $workArea.Width
    $preferredHeightRatio = $preferredRect.height / [double] $workArea.Height
    if ($preferredWidthRatio -gt 0.86 -or $preferredHeightRatio -gt 0.86) {
        throw (
            'Preferred application window is not compact enough. ' +
            "Ratios width=$preferredWidthRatio height=$preferredHeightRatio"
        )
    }
    Capture-AnnotatedDesktop `
        -FileName 'desktop-preferred-window.png' `
        -Window $preferredRect `
        -Label 'PREFERRED WINDOW / containment and compactness check' `
        -DesktopBounds $desktopBounds `
        -WorkArea $workArea | Out-Null

    Set-TestWindowSize -Handle $uiHandle -Width 660 -Height 520 -WorkArea $workArea
    $minimumRect = Get-WindowRectRecord -Handle $uiHandle
    Assert-WindowInsideWorkArea -Window $minimumRect -WorkArea $workArea -Label 'Minimum application window'
    if ($minimumRect.width -gt 690 -or $minimumRect.height -gt 550) {
        throw "Minimum window expanded unexpectedly to $($minimumRect.width)x$($minimumRect.height)."
    }

    $pageNames = @('connection', 'notifications', 'publish', 'history')
    $pageReports = [System.Collections.Generic.List[object]]::new()
    for ($page = 0; $page -lt $pageNames.Count; $page++) {
        Click-ApplicationTab -Handle $uiHandle -Index $page
        $pageRect = Get-WindowRectRecord -Handle $uiHandle
        Assert-WindowInsideWorkArea -Window $pageRect -WorkArea $workArea -Label "Page $($pageNames[$page])"
        $fileName = "desktop-page-$($pageNames[$page]).png"
        Capture-AnnotatedDesktop `
            -FileName $fileName `
            -Window $pageRect `
            -Label "MINIMUM WINDOW / page $($pageNames[$page]) / manual visual review required" `
            -DesktopBounds $desktopBounds `
            -WorkArea $workArea | Out-Null
        $pageReports.Add([ordered] @{
            page = $pageNames[$page]
            screenshot = $fileName
            window = $pageRect
            inside_work_area = $true
        })
    }
}
finally {
    if ($null -ne $uiProcess -and -not $uiProcess.HasExited) {
        Stop-Process -Id $uiProcess.Id -Force -ErrorAction SilentlyContinue
        $uiProcess.WaitForExit()
    }
}
Write-Host 'CHECK full-desktop application geometry: passed'

Write-Host 'CHECK adaptive popup geometry: start'
$popupProfile = Join-Path $env:RUNNER_TEMP 'ntfy-compact-popup-profile'
$popupTopic = "worker-popup-$env:GITHUB_RUN_ID-$env:GITHUB_RUN_ATTEMPT"
Write-SettingsProfile -Profile $popupProfile -Settings ([ordered] @{
    server_url = 'https://ntfy.sh'
    topic = $popupTopic
    notifications_enabled = $true
    sound_enabled = $false
    audio_output = ''
    placement = 2
    auto_connect = $true
})

$popupProcess = Start-NtfyProcess -Executable $exe.Path -Profile $popupProfile -Arguments @('--background')
try {
    Start-Sleep -Seconds 6
    if ($popupProcess.HasExited) {
        throw "Popup listener exited before validation with code $($popupProcess.ExitCode)."
    }

    Invoke-WebRequest `
        -UseBasicParsing `
        -Method Post `
        -Uri "https://ntfy.sh/$popupTopic" `
        -Headers @{ Title = 'Compact popup'; Priority = '3' } `
        -Body 'Short message.' | Out-Null

    $shortHandle = Wait-ForLargestVisibleWindow -Process $popupProcess -Attempts 40 -DelayMilliseconds 250
    Start-Sleep -Milliseconds 400
    $shortRect = Get-WindowRectRecord -Handle $shortHandle
    Assert-WindowInsideWorkArea -Window $shortRect -WorkArea $workArea -Label 'Short notification popup'
    Capture-AnnotatedDesktop `
        -FileName 'desktop-popup-short.png' `
        -Window $shortRect `
        -Label 'SHORT POPUP / compact height / fully inside work area' `
        -DesktopBounds $desktopBounds `
        -WorkArea $workArea | Out-Null

    $longBody = @(
        'This is a deliberately long notification used to validate adaptive popup sizing.',
        'It contains several lines and enough text to require a taller card.',
        'The popup must grow vertically without becoming oversized or leaving the work area.',
        'Every line must remain readable inside the bounded notification surface.',
        'The metadata row and close control must remain visible.'
    ) -join "`n"
    Invoke-WebRequest `
        -UseBasicParsing `
        -Method Post `
        -Uri "https://ntfy.sh/$popupTopic" `
        -Headers @{ Title = 'Adaptive popup validation'; Priority = '4' } `
        -Body $longBody | Out-Null

    $longRect = $null
    for ($attempt = 1; $attempt -le 40; $attempt++) {
        Start-Sleep -Milliseconds 250
        $handle = Get-LargestVisibleWindow -Process $popupProcess
        if ($handle -eq [IntPtr]::Zero) {
            continue
        }
        $candidate = Get-WindowRectRecord -Handle $handle
        if ($candidate.height -ge ($shortRect.height + 30)) {
            $longRect = $candidate
            break
        }
    }
    if ($null -eq $longRect) {
        throw "Long popup did not grow at least 30 pixels beyond short height $($shortRect.height)."
    }
    Assert-WindowInsideWorkArea -Window $longRect -WorkArea $workArea -Label 'Long notification popup'
    if ($longRect.height -gt 300) {
        throw "Long popup exceeded compact maximum height: $($longRect.height)."
    }
    Capture-AnnotatedDesktop `
        -FileName 'desktop-popup-long.png' `
        -Window $longRect `
        -Label 'LONG POPUP / adaptive growth / bounded and fully visible' `
        -DesktopBounds $desktopBounds `
        -WorkArea $workArea | Out-Null
}
finally {
    if ($null -ne $popupProcess -and -not $popupProcess.HasExited) {
        Stop-Process -Id $popupProcess.Id -Force -ErrorAction SilentlyContinue
        $popupProcess.WaitForExit()
    }
}
Write-Host "CHECK adaptive popup geometry: passed (short=$($shortRect.height), long=$($longRect.height))"

Write-Host 'CHECK foreground UI smoke: start'
$smokeProcess = Start-NtfyProcess `
    -Executable $exe.Path `
    -Profile $uiProfile `
    -Arguments @('--smoke-test')
$smokeProcess.WaitForExit()
if ($smokeProcess.ExitCode -ne 0) {
    throw "Foreground UI smoke failed with exit code $($smokeProcess.ExitCode)."
}
Write-Host 'CHECK foreground UI smoke: passed'

Write-Host 'CHECK active background listener below 1 MiB: start'
$memoryProfile = Join-Path $env:RUNNER_TEMP 'ntfy-sub-megabyte-profile'
$memoryTopic = "worker-memory-$env:GITHUB_RUN_ID-$env:GITHUB_RUN_ATTEMPT"
Write-SettingsProfile -Profile $memoryProfile -Settings ([ordered] @{
    server_url = 'https://ntfy.sh'
    topic = $memoryTopic
    notifications_enabled = $false
    sound_enabled = $false
    audio_output = ''
    placement = 2
    auto_connect = $true
})

$backgroundProcess = Start-NtfyProcess `
    -Executable $exe.Path `
    -Profile $memoryProfile `
    -Arguments @('--background')
$memorySamples = [System.Collections.Generic.List[object]]::new()
try {
    foreach ($seconds in @(7, 4, 4)) {
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
$limitBytes = 1MB
$memoryReport = [ordered] @{
    executable = $exe.Path
    target = 'x86_64-pc-windows-msvc'
    listener_topic = $memoryTopic
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
    (Join-Path $outputDirectory 'compact-ui-memory-report.json'),
    ($memoryReport | ConvertTo-Json -Depth 6),
    [Text.UTF8Encoding]::new($false)
)
if (-not $memoryReport.within_idle_working_set_limit) {
    throw (
        'Active background listener working set exceeded 1 MiB: ' +
        "$($memoryReport.final_working_set_bytes) bytes."
    )
}
Write-Host 'CHECK active background listener below 1 MiB: passed'

$visualReport = [ordered] @{
    desktop = [ordered] @{
        left = $desktopBounds.Left
        top = $desktopBounds.Top
        width = $desktopBounds.Width
        height = $desktopBounds.Height
    }
    work_area = [ordered] @{
        left = $workArea.Left
        top = $workArea.Top
        width = $workArea.Width
        height = $workArea.Height
    }
    preferred_window = $preferredRect
    preferred_width_ratio = $preferredWidthRatio
    preferred_height_ratio = $preferredHeightRatio
    minimum_window = $minimumRect
    pages = $pageReports
    popup_short = $shortRect
    popup_long = $longRect
    popup_height_growth = [int] ($longRect.height - $shortRect.height)
    all_windows_inside_work_area = $true
    screenshots_are_full_desktop_and_annotated = $true
}
[IO.File]::WriteAllText(
    (Join-Path $outputDirectory 'compact-ui-visual-report.json'),
    ($visualReport | ConvertTo-Json -Depth 8),
    [Text.UTF8Encoding]::new($false)
)

Write-Host 'CHECK release package: start'
$stage = Join-Path $env:RUNNER_TEMP 'ntfy-compact-package'
Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $stage -Force | Out-Null
Copy-Item -LiteralPath $exe.Path -Destination (Join-Path $stage 'ntfy-windows-client.exe')
Copy-Item -LiteralPath 'README.md' -Destination (Join-Path $stage 'README.md')
Copy-Item -LiteralPath 'LICENSE' -Destination (Join-Path $stage 'LICENSE')

$packagePath = Join-Path $outputDirectory 'ntfy-windows-client-x64.zip'
Remove-Item -LiteralPath $packagePath -Force -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $packagePath -CompressionLevel Optimal

$archive = [IO.Compression.ZipFile]::OpenRead($packagePath)
try {
    $entries = @($archive.Entries | ForEach-Object FullName)
}
finally {
    $archive.Dispose()
}
$requiredEntries = @('ntfy-windows-client.exe', 'README.md', 'LICENSE')
foreach ($required in $requiredEntries) {
    if ($entries -notcontains $required) {
        throw "Release package is missing $required."
    }
    Write-Host "CHECK packaged file: $required"
}

$packageFile = Get-Item -LiteralPath $packagePath
$packageHash = Get-FileHash -LiteralPath $packagePath -Algorithm SHA256
$packageReport = [ordered] @{
    package = $packagePath
    size_bytes = [int64] $packageFile.Length
    sha256 = $packageHash.Hash.ToLowerInvariant()
    contains = $requiredEntries
    source_commit = $env:GITHUB_SHA
    visual_report = 'compact-ui-visual-report.json'
    memory_report = 'compact-ui-memory-report.json'
}
[IO.File]::WriteAllText(
    (Join-Path $outputDirectory 'compact-ui-package-report.json'),
    ($packageReport | ConvertTo-Json -Depth 5),
    [Text.UTF8Encoding]::new($false)
)
Write-Host "CHECK release package: passed ($packagePath)"
Write-Host 'CHECK compact UI release validation: passed'
