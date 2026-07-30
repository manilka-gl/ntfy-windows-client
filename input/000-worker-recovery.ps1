Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host 'CHECK worker recovery revision 3: start'
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

Write-Host 'CHECK malformed-input-detection: start'
$tokens = $null
$errors = $null
[void] [System.Management.Automation.Language.Parser]::ParseInput(
    "Set-StrictMode -Version Latest`nfunction Broken {`n",
    [ref] $tokens,
    [ref] $errors
)

if ($errors.Count -eq 0) {
    throw 'Malformed PowerShell unexpectedly produced no parser errors.'
}
Write-Host "CHECK malformed input detected: $($errors[0].Message)"

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
