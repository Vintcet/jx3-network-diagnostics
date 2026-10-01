$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$releaseDir = Join-Path $projectRoot 'release'
$appExe = Join-Path $projectRoot 'src-tauri\target\release\jx3-network-diagnostics.exe'
$installerExe = Join-Path $projectRoot 'src-tauri\target\release\bundle\nsis\剑网三网络诊断_0.1.0_x64-setup.exe'
if (-not (Test-Path -LiteralPath $appExe)) { throw '请先构建正式版 EXE。' }
New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null
Copy-Item -LiteralPath $appExe -Destination (Join-Path $releaseDir 'jx3-network-diagnostics.exe')
if (Test-Path -LiteralPath $installerExe) {
    Copy-Item -LiteralPath $installerExe -Destination (Join-Path $releaseDir 'jx3-network-diagnostics-0.1.0-x64-setup.exe')
}
Copy-Item -LiteralPath (Join-Path $projectRoot 'docs\使用说明.txt') -Destination $releaseDir
$hashes = Get-ChildItem -LiteralPath $releaseDir -Filter '*.exe' | ForEach-Object {
    $hash = Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256
    '{0}  {1}' -f $hash.Hash.ToLowerInvariant(), $_.Name
}
$hashes | Set-Content -LiteralPath (Join-Path $releaseDir 'SHA256SUMS.txt') -Encoding utf8
Get-ChildItem -LiteralPath $releaseDir | Select-Object Name,Length
