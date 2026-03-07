param(
    [string]$ExtractRoot = 'tests/data/extracted',
    [string]$ToolsDir = 'bin/tools'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host "[release][tools] Installing tools from cache. ExtractRoot=$ExtractRoot, ToolsDir=$ToolsDir"

if (-not (Test-Path $ExtractRoot)) {
    throw "[release][tools] Extract root not found: $ExtractRoot"
}

New-Item -ItemType Directory -Force -Path $ToolsDir | Out-Null

$requiredTools = @('fh_loader.exe', 'QSaharaServer.exe')
$copied = @()

foreach ($tool in $requiredTools) {
    $matches = Get-ChildItem -Path $ExtractRoot -Recurse -Filter $tool -File
    if (-not $matches -or $matches.Count -eq 0) {
        throw "[release][tools] Required tool not found in cache: $tool"
    }

    foreach ($match in $matches) {
        Copy-Item -Path $match.FullName -Destination $ToolsDir -Force
        $copied += Join-Path $ToolsDir $match.Name
    }
}

Write-Host '[release][tools] Copied executable files:'
Get-ChildItem -Path $ToolsDir -Filter '*.exe' | Select-Object Name, Length | Format-Table -AutoSize

foreach ($tool in $requiredTools) {
    $target = Join-Path $ToolsDir $tool
    if (-not (Test-Path $target)) {
        throw "[release][tools] Tool missing after copy: $target"
    }
}

Write-Host "[release][tools] Tool installation complete. Copied: $($copied -join ', ')"
