#Requires -Version 5.1
<#
.SYNOPSIS
  Downloads a llama.cpp Windows release and copies llama-server.exe + DLLs into HyMT's
  runtime folder.

.DESCRIPTION
  HyMT does not bundle an inference engine. This script fetches the newest official
  llama.cpp build from GitHub and extracts the binaries that `llama-server` needs.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\setup-runtime.ps1

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\setup-runtime.ps1 -Variant cuda
#>
param(
    [ValidateSet('cpu', 'cuda', 'vulkan', 'hip')]
    [string]$Variant = 'cpu',

    # Defaults to %LOCALAPPDATA%\HyMT\runtime, or <repo>\runtime in portable mode.
    [string]$Destination,

    [switch]$Force
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
if (-not $Destination) {
    if ((Test-Path (Join-Path $repoRoot 'portable.txt')) -or (Test-Path (Join-Path $repoRoot 'models'))) {
        $Destination = Join-Path $repoRoot 'runtime'
    } elseif ($env:LOCALAPPDATA) {
        $Destination = Join-Path $env:LOCALAPPDATA 'HyMT\runtime'
    } else {
        $Destination = Join-Path $repoRoot 'runtime'
    }
}

Write-Host "llama.cpp variant : $Variant"
Write-Host "destination       : $Destination"

if ((Test-Path (Join-Path $Destination 'llama-server.exe')) -and -not $Force) {
    Write-Host "llama-server.exe already exists. Use -Force to overwrite." -ForegroundColor Yellow
    exit 0
}

New-Item -ItemType Directory -Force -Path $Destination | Out-Null

$release = Invoke-RestMethod `
    -Uri 'https://api.github.com/repos/ggml-org/llama.cpp/releases/latest' `
    -Headers @{ 'User-Agent' = 'hymt-setup'; 'Accept' = 'application/vnd.github+json' }

$pattern = switch ($Variant) {
    'cpu'    { 'bin-win-cpu-x64\.zip$' }
    'cuda'   { 'bin-win-cuda-[\d\.]+-x64\.zip$' }
    'vulkan' { 'bin-win-vulkan-x64\.zip$' }
    'hip'    { 'bin-win-hip-.*x64\.zip$' }
}

$asset = $release.assets | Where-Object { $_.name -match $pattern } | Select-Object -First 1
if (-not $asset) {
    Write-Host "No asset matched '$pattern' in release $($release.tag_name)." -ForegroundColor Red
    Write-Host "Available assets:"
    $release.assets | ForEach-Object { Write-Host "  $($_.name)" }
    exit 1
}

$temp = Join-Path $env:TEMP ('hymt-llama-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $temp | Out-Null
$zip = Join-Path $temp $asset.name

try {
    Write-Host "Downloading $($asset.name) …"
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip -UseBasicParsing
    Expand-Archive -Path $zip -DestinationPath $temp -Force

    $binaries = Get-ChildItem -Path $temp -Recurse -File -Include '*.exe', '*.dll'
    if (-not ($binaries | Where-Object { $_.Name -eq 'llama-server.exe' })) {
        throw "llama-server.exe was not found in $($asset.name)."
    }

    foreach ($file in $binaries) {
        Copy-Item -Path $file.FullName -Destination $Destination -Force
    }

    Write-Host "Installed $($binaries.Count) files into $Destination" -ForegroundColor Green
    Write-Host "Restart HyMT, or press '엔진 시작' in the Models tab." -ForegroundColor Green
}
finally {
    Remove-Item -Recurse -Force $temp -ErrorAction SilentlyContinue
}
