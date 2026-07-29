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
