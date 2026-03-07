param(
    [Parameter(Mandatory = $true)]
    [string]$TagName
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$archive = "LTBox-win_amd64-$TagName.zip"
$excludes = @(
    '.git',
    '.git\\*',
    '.github',
    '.github\\*',
    '.gitignore',
    '.gitattributes',
    '.editorconfig',
    '.pre-commit-config.yaml',
    'bin\\requirements-dev.txt',
    'pytest.ini',
    'tests',
    'tests\\*'
)

$excludeArgs = $excludes | ForEach-Object { "-xr!$_" }
& 7z a $archive '.' @excludeArgs
