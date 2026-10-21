param([string]$Destination = (Join-Path $PSScriptRoot 'dist'))
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$binary = Join-Path $PSScriptRoot 'target/release/wayvr-xs-windows.exe'
if (-not (Test-Path -LiteralPath $binary)) { throw 'Build windows/Cargo.toml in release mode first.' }
$package = Join-Path $Destination 'wayvr-xs-windows-x64'
New-Item -ItemType Directory -Force -Path $package | Out-Null
Copy-Item -LiteralPath $binary -Destination $package
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'README.md') -Destination $package
Copy-Item -LiteralPath (Join-Path $repo 'LICENSE') -Destination (Join-Path $package 'LICENSE')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'vendor/openvr/LICENSE') -Destination (Join-Path $package 'LICENSE-OpenVR')
$commit = git -C $repo rev-parse HEAD
if ($LASTEXITCODE -ne 0) { throw 'Cannot determine source commit.' }
Set-Content -LiteralPath (Join-Path $package 'SOURCE.txt') -Value "https://github.com/Chineko/wayvr_xs/tree/$commit"
$archive = Join-Path $Destination 'wayvr-xs-windows-x64.zip'
Compress-Archive -Path (Join-Path $package '*') -DestinationPath $archive -Force
Get-FileHash -LiteralPath $archive -Algorithm SHA256
