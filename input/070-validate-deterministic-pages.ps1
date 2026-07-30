Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host 'COMMAND REVISION: 070 deterministic initial-page validation'

# Run the complete release, popup, memory, containment, and packaging suite first.
# Dot-sourcing retains its proven Win32 capture helpers for the second visual pass.
. (Join-Path $PSScriptRoot '061-validate-compact-ui.ps1')

function Save-WindowCrop {
    param(
        [Parameter(Mandatory)]
        [string] $DesktopScreenshot,

        [Parameter(Mandatory)]
        [System.Collections.IDictionary] $Window,

        [Parameter(Mandatory)]
        [Drawing.Rectangle] $DesktopBounds,

        [Parameter(Mandatory)]
        [string] $OutputPath
    )

    $desktop = [Drawing.Bitmap]::new($DesktopScreenshot)
    try {
        $cropRectangle = [Drawing.Rectangle]::new(
            [int] $Window.left - $DesktopBounds.Left,
            [int] $Window.top - $DesktopBounds.Top,
            [int] $Window.width,
            [int] $Window.height
        )
        $crop = $desktop.Clone($cropRectangle, $desktop.PixelFormat)
        try {
            $crop.Save($OutputPath, [Drawing.Imaging.ImageFormat]::Png)
        }
        finally {
            $crop.Dispose()
        }
    }
    finally {
        $desktop.Dispose()
    }
}

Write-Host 'CHECK deterministic visual-page example build: start'
& cargo build --locked --release --target x86_64-pc-windows-msvc --example visual_page
if ($LASTEXITCODE -ne 0) {
    throw "Visual page example build failed with exit code $LASTEXITCODE."
}
$visualExe = Resolve-Path -LiteralPath (
    'target/x86_64-pc-windows-msvc/release/examples/visual_page.exe'
)
Write-Host "CHECK deterministic visual-page example build: passed ($($visualExe.Path))"

Write-Host 'CHECK deterministic initial page captures: start'
$pageNames = @('connection', 'notifications', 'publish', 'history')
$deterministicPages = [System.Collections.Generic.List[object]]::new()
$cropHashes = [System.Collections.Generic.List[string]]::new()

for ($page = 0; $page -lt $pageNames.Count; $page++) {
    $pageName = $pageNames[$page]
    Write-Host "CHECK deterministic page $page ($pageName): start"
    $pageProcess = Start-NtfyProcess `
        -Executable $visualExe.Path `
        -Profile $uiProfile `
        -Arguments @("--page=$page")
    try {
        $pageHandle = Wait-ForLargestVisibleWindow -Process $pageProcess
        Set-TestWindowSize `
            -Handle $pageHandle `
            -Width 660 `
            -Height 520 `
            -WorkArea $workArea
        [NtfyVisualNative]::SetForegroundWindow($pageHandle) | Out-Null
        Start-Sleep -Milliseconds 1500

        $pageRect = Get-WindowRectRecord -Handle $pageHandle
        Assert-WindowInsideWorkArea `
            -Window $pageRect `
            -WorkArea $workArea `
            -Label "Deterministic page $pageName"

        $desktopFile = "desktop-page-$pageName.png"
        $desktopPath = Capture-AnnotatedDesktop `
            -FileName $desktopFile `
            -Window $pageRect `
            -Label "DETERMINISTIC INITIAL PAGE $page // $($pageName.ToUpperInvariant()) // minimum window // contained" `
            -DesktopBounds $desktopBounds `
            -WorkArea $workArea

        $cropFile = "window-page-$pageName.png"
        $cropPath = Join-Path $outputDirectory $cropFile
        Save-WindowCrop `
            -DesktopScreenshot $desktopPath `
            -Window $pageRect `
            -DesktopBounds $desktopBounds `
            -OutputPath $cropPath
        $cropHash = (
            Get-FileHash -LiteralPath $cropPath -Algorithm SHA256
        ).Hash.ToLowerInvariant()
        $cropHashes.Add($cropHash)

        $deterministicPages.Add([ordered] @{
            page_index = $page
            page = $pageName
            process_id = $pageProcess.Id
            launch_argument = "--page=$page"
            screenshot = $desktopFile
            window_crop = $cropFile
            crop_sha256 = $cropHash
            window = $pageRect
            inside_work_area = $true
            selected_before_first_show = $true
        })
        Write-Host (
            "CHECK deterministic page $page ($pageName): passed " +
            "window=$($pageRect.width)x$($pageRect.height) crop_sha256=$cropHash"
        )
    }
    finally {
        if ($null -ne $pageProcess -and -not $pageProcess.HasExited) {
            Stop-Process -Id $pageProcess.Id -Force -ErrorAction SilentlyContinue
            $pageProcess.WaitForExit()
        }
    }
}

$uniqueCropHashes = @($cropHashes | Sort-Object -Unique)
if ($uniqueCropHashes.Count -ne $pageNames.Count) {
    throw (
        'Deterministic page captures were not all visually distinct. ' +
        "Unique crop hashes: $($uniqueCropHashes.Count) of $($pageNames.Count)."
    )
}
Write-Host 'CHECK deterministic initial page captures: passed'

$visualReportPath = Join-Path $outputDirectory 'compact-ui-visual-report.json'
$visualReport = Get-Content -LiteralPath $visualReportPath -Raw | ConvertFrom-Json
$visualReport.pages = @($deterministicPages)
$visualReport | Add-Member `
    -NotePropertyName page_capture_mode `
    -NotePropertyValue 'separate-process-initial-page' `
    -Force
$visualReport | Add-Member `
    -NotePropertyName page_capture_click_timing_used `
    -NotePropertyValue $false `
    -Force
$visualReport | Add-Member `
    -NotePropertyName deterministic_page_crop_hashes_unique `
    -NotePropertyValue $true `
    -Force
$visualReport | Add-Member `
    -NotePropertyName visual_page_executable `
    -NotePropertyValue $visualExe.Path `
    -Force
[IO.File]::WriteAllText(
    $visualReportPath,
    ($visualReport | ConvertTo-Json -Depth 10),
    [Text.UTF8Encoding]::new($false)
)

Write-Host 'CHECK regenerate deterministic visual evidence: start'
& (Join-Path $PSScriptRoot '066-build-visual-contact-sheet.ps1')
if ($LASTEXITCODE -ne 0) {
    throw 'Contact sheet generation failed.'
}
& (Join-Path $PSScriptRoot '067-build-tiny-contact-sheet.ps1')
if ($LASTEXITCODE -ne 0) {
    throw 'Tiny contact sheet generation failed.'
}
& (Join-Path $PSScriptRoot '068-export-tiny-contact-sheet-base64.ps1')
if ($LASTEXITCODE -ne 0) {
    throw 'Visual inspection payload generation failed.'
}
& (Join-Path $PSScriptRoot '069-chunk-visual-contact-payload.ps1')
if ($LASTEXITCODE -ne 0) {
    throw 'Visual inspection payload chunking failed.'
}
Write-Host 'CHECK regenerate deterministic visual evidence: passed'

Write-Host 'CHECK deterministic compact UI validation: passed'
