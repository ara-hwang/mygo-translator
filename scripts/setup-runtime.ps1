#Requires -Version 5.1
<#
.SYNOPSIS
  Downloads a llama.cpp Windows release and copies llama-server.exe + DLLs into HyMT's
  runtime folder.

.DESCRIPTION
  HyMT does not bundle an inference engine. This script fetches the newest official
  llama.cpp build from GitHub and extracts the binaries that `llama-server` needs.

  llama.cpp publishes every commit build as a pre-release (the non-pre-release
  "latest" entry only carries a nightly-tag.txt stub), so this script walks the
  release list and picks the newest release that actually contains an asset
  matching the requested variant.

  If the `gh` CLI is installed and signed in, its token is used for the GitHub API
  to avoid unauthenticated rate limits. Otherwise the API is called anonymously.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\setup-runtime.ps1

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\setup-runtime.ps1 -Variant cuda
#>
param(
    [ValidateSet('cpu', 'cuda', 'vulkan', 'hip')]
    [string]$Variant = 'cpu',

    # Defaults to %LOCALAPPDATA%\offline-translator\runtime, or <repo>\runtime in portable mode.
    [string]$Destination,

    [switch]$Force
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
if (-not $Destination) {
    if ((Test-Path (Join-Path $repoRoot 'portable.txt')) -or (Test-Path (Join-Path $repoRoot 'models'))) {
        $Destination = Join-Path $repoRoot 'runtime'
    } elseif ($env:LOCALAPPDATA) {
        $Destination = Join-Path $env:LOCALAPPDATA 'offline-translator\runtime'
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

# Use the gh CLI token when available; unauthenticated API calls are rate-limited.
$token = $null
try {
    $token = gh auth token
    if ($LASTEXITCODE -ne 0) { $token = $null }
} catch {
    $token = $null
}

$headers = @{ 'User-Agent' = 'hymt-setup'; 'Accept' = 'application/vnd.github+json' }
if ($token) {
    $headers['Authorization'] = "Bearer $token"
}

$releases = Invoke-RestMethod `
    -Uri 'https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=30' `
    -Headers $headers

$pattern = switch ($Variant) {
    'cpu'    { 'bin-win-cpu-x64\.zip$' }
    'cuda'   { '^llama-.+bin-win-cuda-[\d\.]+-x64\.zip$' }
    'vulkan' { 'bin-win-vulkan-x64\.zip$' }
    'hip'    { 'bin-win-hip-.*x64\.zip$' }
}

# Walk releases from newest to oldest; pick the first one that ships the variant.
$release = $null
$asset = $null
foreach ($candidate in $releases) {
    $matched = @($candidate.assets | Where-Object { $_.name -match $pattern } | Sort-Object name)
    if ($matched) {
        $release = $candidate
        $asset = $matched[0]
        break
    }
}
if (-not $asset) {
    Write-Host "No asset matched '$pattern' in the last $($releases.Count) releases." -ForegroundColor Red
    if ($release) {
        Write-Host "Available assets in $($release.tag_name):"
        $release.assets | ForEach-Object { Write-Host "  $($_.name)" }
    }
    exit 1
}

# CUDA builds ship the CUDA runtime in a separate cudart zip. Download it too or
# llama-server.exe fails to start with missing DLLs.
$downloadAssets = @($asset)
if ($Variant -eq 'cuda' -and ($asset.name -match 'bin-win-cuda-([\d\.]+)-x64\.zip$')) {
    $cudartName = "cudart-llama-bin-win-cuda-$($Matches[1])-x64.zip"
    $cudart = $release.assets | Where-Object { $_.name -eq $cudartName } | Select-Object -First 1
    if ($cudart) {
        $downloadAssets += $cudart
    } else {
        Write-Host "Warning: $cudartName not found in $($release.tag_name); startup may fail." -ForegroundColor Yellow
    }
}

$temp = Join-Path $env:TEMP ('hymt-llama-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $temp | Out-Null

try {
    foreach ($item in $downloadAssets) {
        Write-Host "Downloading $($item.name) …"
        $zip = Join-Path $temp $item.name
        Invoke-WebRequest -Uri $item.browser_download_url -OutFile $zip -UseBasicParsing
        Expand-Archive -Path $zip -DestinationPath $temp -Force
    }

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
