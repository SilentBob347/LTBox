param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$Tag
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($Tag -notmatch '^v') {
    throw "[release][tag] Invalid tag format: '$Tag'. Expected to start with 'v'."
}

Write-Host "[release][tag] Resolved tag: $Tag"
"TAG_NAME=$Tag" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
"tag=$Tag" | Out-File -FilePath $env:GITHUB_OUTPUT -Append -Encoding utf8
