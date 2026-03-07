param(
    [Parameter(Mandatory = $true)]
    [string]$TagName,

    [string]$ConfigPath = 'bin/ltbox/config.json'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$configVersion = (Get-Content $ConfigPath | ConvertFrom-Json).version

Write-Host "Tag: $TagName"
Write-Host "Config: $configVersion"

if ($TagName -ne $configVersion) {
    throw "Error: Git tag ($TagName) does not match config.json version ($configVersion)."
}
