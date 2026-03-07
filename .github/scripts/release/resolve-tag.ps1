param(
    [Parameter(Mandatory = $true)]
    [string]$Tag
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

"TAG_NAME=$Tag" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
"tag=$Tag" | Out-File -FilePath $env:GITHUB_OUTPUT -Append -Encoding utf8
