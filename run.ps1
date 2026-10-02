#requires -Version 7.0
& (Join-Path $PSScriptRoot 'scripts\start.ps1') @args
exit $LASTEXITCODE
