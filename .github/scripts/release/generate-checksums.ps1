param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$TagName
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$archive = "LTBox-win_amd64-$TagName.zip"
$checksumFile = "$archive.sha256"

Write-Host "[release][checksum] Generating checksum for: $archive"

if (-not (Test-Path $archive)) {
    throw "[release][checksum] Archive not found: $archive"
}

$hash = (Get-FileHash -Algorithm SHA256 -Path $archive).Hash.ToLower()
"$hash  $archive" | Out-File -FilePath $checksumFile -Encoding ascii

if (-not (Test-Path $checksumFile)) {
    throw "[release][checksum] Checksum file was not created: $checksumFile"
}

Write-Host "[release][checksum] Created checksum file: $checksumFile"
