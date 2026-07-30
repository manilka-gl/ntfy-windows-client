Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host 'COMMAND REVISION: 064 after publish string compatibility fix'
& (Join-Path $PSScriptRoot '061-validate-compact-ui.ps1')
