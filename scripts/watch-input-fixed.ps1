[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string] $Branch,

    [string] $InputDirectory = 'input',

    [string] $OutputDirectory = 'output',

    [ValidateRange(2, 3600)]
    [int] $PollSeconds = 10,

    [ValidateRange(1, 340)]
    [int] $MaxRuntimeMinutes = 320,

    [ValidateRange(1, 300)]
    [int] $MaximumCommandMinutes = 60
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$SourcePath = Join-Path $PSScriptRoot 'watch-input.ps1'
$SourceText = [IO.File]::ReadAllText($SourcePath)

$OriginalFetch = @'
function Fetch-WatchedBranch {
    Invoke-Git -Arguments @(
        'fetch',
        '--no-tags',
        'origin',
        $FetchRefSpec
    )
}
'@

$PatchedFetch = @'
function Fetch-WatchedBranch {
    & git fetch --no-tags origin $FetchRefSpec
    $FetchExitCode = $LASTEXITCODE

    if ($FetchExitCode -eq 0) {
        return
    }

    # A command is allowed to use Git and can accidentally delete the watched
    # remote branch. Distinguish that case from a transient fetch failure and
    # recreate the ref from the checked-out source commit before continuing.
    & git ls-remote --exit-code --heads origin $RemoteHeadRef *> $null
    $LookupExitCode = $LASTEXITCODE

    if ($LookupExitCode -ne 2) {
        throw (
            "Could not fetch monitored branch $Branch; " +
            "git fetch exited with $FetchExitCode and remote lookup " +
            "exited with $LookupExitCode."
        )
    }

    $LocalCommit = Get-GitText -Arguments @(
        'rev-parse',
        '--verify',
        'HEAD'
    )

    Write-Warning (
        "Remote branch $Branch disappeared. Restoring it from " +
        "$LocalCommit."
    )

    Invoke-Git -Arguments @(
        'push',
        'origin',
        "${LocalCommit}:$RemoteHeadRef"
    )

    Invoke-Git -Arguments @(
        'fetch',
        '--no-tags',
        'origin',
        $FetchRefSpec
    )
}
'@

if (-not $SourceText.Contains($OriginalFetch)) {
    throw 'Expected Fetch-WatchedBranch implementation was not found.'
}

$OriginalInvocation = @'
            $CommandResult = Invoke-CommandFile `
                -CommandFile $CommandFile `
                -TimeoutSeconds $CommandTimeoutSeconds
'@

$PatchedInvocation = @'
            $CommandResultItems = @(
                Invoke-CommandFile `
                    -CommandFile $CommandFile `
                    -TimeoutSeconds $CommandTimeoutSeconds
            )

            if ($CommandResultItems.Count -eq 0) {
                throw 'Invoke-CommandFile returned no command metadata.'
            }

            $CommandResult = $CommandResultItems[-1]
'@

if (-not $SourceText.Contains($OriginalInvocation)) {
    throw 'Expected Invoke-CommandFile invocation was not found.'
}

$PatchedText = $SourceText.Replace(
    $OriginalFetch,
    $PatchedFetch
).Replace(
    $OriginalInvocation,
    $PatchedInvocation
)

$TemporaryPath = Join-Path `
    $env:RUNNER_TEMP `
    ('watch-input-patched-' + [Guid]::NewGuid().ToString('N') + '.ps1')

try {
    [IO.File]::WriteAllText(
        $TemporaryPath,
        $PatchedText,
        [Text.UTF8Encoding]::new($false)
    )

    & $TemporaryPath @PSBoundParameters
    exit $LASTEXITCODE
}
finally {
    Remove-Item `
        -LiteralPath $TemporaryPath `
        -Force `
        -ErrorAction SilentlyContinue
}
