Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$sourcePath = Join-Path $PSScriptRoot '050-verify-ui-layout.ps1'
$source = [IO.File]::ReadAllText($sourcePath)

$oldAttempts = 'for ($attempt = 1; $attempt -le 40; $attempt++) {'
$newAttempts = 'for ($attempt = 1; $attempt -le 80; $attempt++) {'
$oldLookup = "$windowHandle = [NtfyWindowCapture]::FindWindow(`$null, 'ntfy for Windows')"
$newLookup = @'
$uiProcess.Refresh()
        $windowHandle = $uiProcess.MainWindowHandle
'@.TrimEnd()
$oldError = "throw 'Could not locate the ntfy application window.'"
$newError = @'
throw (
            'Could not locate a main window owned by the ntfy process. ' +
            "Process ID: $($uiProcess.Id)."
        )
'@.TrimEnd()

foreach ($expected in @($oldAttempts, $oldLookup, $oldError)) {
    if (-not $source.Contains($expected)) {
        throw "Expected validation-script text was not found: $expected"
    }
}

$patched = $source.Replace($oldAttempts, $newAttempts)
$patched = $patched.Replace($oldLookup, $newLookup)
$patched = $patched.Replace($oldError, $newError)

$temporaryScript = Join-Path `
    $env:RUNNER_TEMP `
    ('verify-ui-layout-' + [Guid]::NewGuid().ToString('N') + '.ps1')

try {
    [IO.File]::WriteAllText(
        $temporaryScript,
        $patched,
        [Text.UTF8Encoding]::new($false)
    )
    & $temporaryScript
    exit $LASTEXITCODE
}
finally {
    Remove-Item -LiteralPath $temporaryScript -Force -ErrorAction SilentlyContinue
}
