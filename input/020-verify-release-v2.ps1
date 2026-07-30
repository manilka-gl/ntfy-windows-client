Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host 'CHECK verification revision 2: start'
& (Join-Path $PSScriptRoot '010-verify-release.ps1')
$exitCode = $LASTEXITCODE
Write-Host "CHECK verification revision 2: exit $exitCode"
exit $exitCode
