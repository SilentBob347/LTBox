param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$TagName,

    [string]$ConfigPath = 'bin/ltbox/config.json'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Test-Path $ConfigPath)) {
    throw "[release][verify] Config file not found: $ConfigPath"
}

$config = Get-Content $ConfigPath -Raw | ConvertFrom-Json
if (-not $config.version) {
    throw "[release][verify] 'version' field missing in $ConfigPath"
}

$configVersion = $config.version
Write-Host "[release][verify] Tag version: $TagName"
Write-Host "[release][verify] Config version: $configVersion"

if ($TagName -ne $configVersion) {
    throw "[release][verify] Git tag ($TagName) does not match config.json version ($configVersion)."
}

Write-Host '[release][verify] Version check passed.'
