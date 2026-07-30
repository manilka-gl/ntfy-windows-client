Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host 'COMMAND REVISION: 065 after div_ceil compatibility fix'
& (Join-Path $PSScriptRoot '061-validate-compact-ui.ps1')
