# stoolie installer for Windows 10/11.
#
#   irm https://raw.githubusercontent.com/0DukePan/stoolie/main/install.ps1 | iex
#
# Downloads the latest release asset, verifies it against the published
# SHA-256, and installs stoolie.exe under %LOCALAPPDATA%\stoolie\bin.
# No administrator rights required.

$ErrorActionPreference = 'Stop'
$repo = '0DukePan/stoolie'
$assetPattern = 'windows-x86_64.zip'

Write-Host "stoolie installer" -ForegroundColor Cyan
Write-Host "  source: https://github.com/$repo"

$release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/latest" `
    -Headers @{ 'User-Agent' = 'stoolie-installer'; 'Accept' = 'application/vnd.github+json' }
$asset = $release.assets | Where-Object { $_.name -like "*$assetPattern" } | Select-Object -First 1
$checksumAsset = $release.assets | Where-Object { $_.name -like "*.sha256" } | Select-Object -First 1

if (-not $asset) { throw "Release $($release.tag_name) has no $assetPattern asset." }
if (-not $checksumAsset) { throw "Release $($release.tag_name) publishes no SHA-256 file." }

$temp = Join-Path $env:TEMP ("stoolie-install-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temp | Out-Null

try {
    $zip = Join-Path $temp $asset.name
    Write-Host "  downloading $($asset.name)"
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip -UseBasicParsing

    Write-Host "  verifying SHA-256"
    # `Content` is a string on some PowerShell builds and a byte array on others.
    $response = Invoke-WebRequest -Uri $checksumAsset.browser_download_url -UseBasicParsing
    $published = if ($response.Content -is [byte[]]) {
        [System.Text.Encoding]::UTF8.GetString($response.Content)
    } else {
        [string]$response.Content
    }
    $expected = ($published -split '\s+')[0].Trim().ToLower()
    if ($expected -notmatch '^[0-9a-f]{64}$') { throw "published checksum is malformed: $expected" }
    $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    if ($expected -ne $actual) {
        throw "checksum mismatch: expected $expected, got $actual"
    }
    Write-Host "  checksum ok ($($actual.Substring(0,12))...)" -ForegroundColor Green

    Expand-Archive -Path $zip -DestinationPath $temp -Force
    $exe = Get-ChildItem -Path $temp -Filter 'stoolie.exe' -Recurse | Select-Object -First 1
    if (-not $exe) { throw "stoolie.exe is missing from the archive." }

    $target = Join-Path $env:LOCALAPPDATA 'stoolie\bin'
    New-Item -ItemType Directory -Path $target -Force | Out-Null
    Copy-Item $exe.FullName (Join-Path $target 'stoolie.exe') -Force
    Write-Host "  installed to $target\stoolie.exe" -ForegroundColor Green

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($userPath -notlike "*$target*") {
        [Environment]::SetEnvironmentVariable('Path', "$userPath;$target", 'User')
        Write-Host "  added $target to your user PATH (open a new terminal)" -ForegroundColor Yellow
    }

    Write-Host ""
    Write-Host "Try it:" -ForegroundColor Cyan
    Write-Host "  stoolie doctor"
    Write-Host "  stoolie"
    Write-Host "  stoolie receipt --open"
}
finally {
    Remove-Item -Path $temp -Recurse -Force -ErrorAction SilentlyContinue
}
