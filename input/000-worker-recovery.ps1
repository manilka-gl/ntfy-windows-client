Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host 'CHECK worker-control-parse: start'

$workerScripts = @(
    'scripts/watch-input.ps1',
    'scripts/watch-input-fixed.ps1'
)

foreach ($scriptPath in $workerScripts) {
    $tokens = $null
    $errors = $null
    [void] [System.Management.Automation.Language.Parser]::ParseFile(
        (Resolve-Path -LiteralPath $scriptPath),
        [ref] $tokens,
        [ref] $errors
    )

    if ($errors.Count -ne 0) {
        $details = $errors | ForEach-Object { $_.Message }
        throw "PowerShell parse errors in ${scriptPath}: $($details -join '; ')"
    }

    Write-Host "CHECK parsed: $scriptPath"
}

Write-Host 'CHECK malformed-child-isolation: start'
$malformedPath = Join-Path $env:RUNNER_TEMP 'ntfy-malformed-input.ps1'
[IO.File]::WriteAllText(
    $malformedPath,
    "Set-StrictMode -Version Latest`nfunction Broken {`n",
    [Text.UTF8Encoding]::new($false)
)

$process = Start-Process `
    -FilePath (Get-Command pwsh -ErrorAction Stop).Source `
    -ArgumentList @(
        '-NoLogo',
        '-NoProfile',
        '-NonInteractive',
        '-ExecutionPolicy',
        'Bypass',
        '-File',
        $malformedPath
    ) `
    -NoNewWindow `
    -Wait `
    -PassThru

Remove-Item -LiteralPath $malformedPath -Force -ErrorAction SilentlyContinue

if ($process.ExitCode -eq 0) {
    throw 'Malformed PowerShell unexpectedly returned exit code 0.'
}

Write-Host "CHECK malformed child rejected with exit code $($process.ExitCode)"

& git ls-remote --exit-code --heads origin "refs/heads/$env:WORKER_BRANCH" *> $null
if ($LASTEXITCODE -ne 0) {
    throw "Monitored remote branch is missing: $env:WORKER_BRANCH"
}
Write-Host "CHECK monitored branch exists: $env:WORKER_BRANCH"

cargo fmt --all -- --check
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
Write-Host 'CHECK cargo fmt: passed'

Write-Host 'CHECK worker recovery diagnostic: passed'
