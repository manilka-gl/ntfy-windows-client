Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$sourcePath = Join-Path $PSScriptRoot '060-validate-compact-ui.ps1'
$source = [IO.File]::ReadAllText($sourcePath)
$source = $source.Replace(
    '[hashtable] $Settings',
    '[System.Collections.IDictionary] $Settings'
)
$source = $source.Replace(
    '[hashtable] $Window',
    '[System.Collections.IDictionary] $Window'
)

$patchedPath = Join-Path $env:RUNNER_TEMP '061-validate-compact-ui-patched.ps1'
[IO.File]::WriteAllText(
    $patchedPath,
    $source,
    [Text.UTF8Encoding]::new($false)
)

. $patchedPath
