param(
    [string]$ExtractRoot = 'tests/data/extracted',
    [string]$ToolsDir = 'bin/tools'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

New-Item -ItemType Directory -Force -Path $ToolsDir | Out-Null

Get-ChildItem -Path $ExtractRoot -Recurse -Filter 'fh_loader.exe' | Copy-Item -Destination $ToolsDir -Force
Get-ChildItem -Path $ExtractRoot -Recurse -Filter 'QSaharaServer.exe' | Copy-Item -Destination $ToolsDir -Force

Get-ChildItem -Path $ToolsDir -Filter '*.exe' | Format-Table -AutoSize
