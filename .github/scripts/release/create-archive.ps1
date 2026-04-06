param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$TagName
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host "[release][archive] Creating archive for tag: $TagName"

$archive = "LTBox-win_amd64-$TagName.zip"
$excludes = @(
    '.git',
    '.git\*',
    '.github',
    '.github\*',
    '.gitmodules',
    '.gitignore',
    '.gitattributes',
    '.editorconfig',
    'vendor',
    'vendor\*',
    '.pre-commit-config.yaml',
    'bin\requirements-dev.txt',
    'pytest.ini',
    'tests',
    'tests\*'
)

if (Get-Command 7z -ErrorAction SilentlyContinue) {
    Write-Host '[release][archive] Using 7z executable from PATH.'
} else {
    throw '[release][archive] 7z command not found. Ensure 7-Zip is installed and available in PATH.'
}

if (Test-Path $archive) {
    Write-Host "[release][archive] Removing previous archive: $archive"
    Remove-Item -Force $archive
}

$excludeArgs = $excludes | ForEach-Object { "-xr!$_" }
& 7z a $archive '.' @excludeArgs

if (-not (Test-Path $archive)) {
    throw "[release][archive] Archive was not created: $archive"
}

$archiveInfo = Get-Item $archive
Write-Host "[release][archive] Created archive: $($archiveInfo.FullName) ($([Math]::Round($archiveInfo.Length / 1MB, 2)) MB)"
