param(
    [Parameter(Mandatory = $true)]
    [string]$TagName
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$archive = "LTBox-win_amd64-$TagName.zip"
$hash = (Get-FileHash -Algorithm SHA256 -Path $archive).Hash.ToLower()
"$hash  $archive" | Out-File -FilePath "$archive.sha256" -Encoding ascii
