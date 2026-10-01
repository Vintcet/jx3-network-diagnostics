$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$appVersion = (Get-Content -Raw -LiteralPath (Join-Path $projectRoot 'package.json') | ConvertFrom-Json).version
$releaseDir = Join-Path $projectRoot 'release'
$appExe = Join-Path $projectRoot 'src-tauri\target\release\jx3-network-diagnostics.exe'
$installerExe = Join-Path $projectRoot ('src-tauri\target\release\bundle\nsis\剑网三网络诊断_{0}_x64-setup.exe' -f $appVersion)
if (-not (Test-Path -LiteralPath $appExe)) { throw '请先构建正式版 EXE。' }
New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null
Copy-Item -LiteralPath $appExe -Destination (Join-Path $releaseDir ('jx3-network-diagnostics-{0}.exe' -f $appVersion))
try { Copy-Item -LiteralPath $appExe -Destination (Join-Path $releaseDir 'jx3-network-diagnostics.exe') } catch { Write-Warning '旧的无版本号 EXE 可能正在运行，请使用新生成的带版本号 EXE。' }
if (Test-Path -LiteralPath $installerExe) {
    Copy-Item -LiteralPath $installerExe -Destination (Join-Path $releaseDir ('jx3-network-diagnostics-{0}-x64-setup.exe' -f $appVersion))
}
Copy-Item -LiteralPath (Join-Path $projectRoot 'docs\使用说明.txt') -Destination $releaseDir
$hashes = Get-ChildItem -LiteralPath $releaseDir -Filter '*.exe' | ForEach-Object {
    $stream = [System.IO.File]::OpenRead($_.FullName)
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try {
        $digest = [System.BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '').ToLowerInvariant()
        '{0}  {1}' -f $digest, $_.Name
    } finally {
        $algorithm.Dispose()
        $stream.Dispose()
    }
}
$hashes | Set-Content -LiteralPath (Join-Path $releaseDir 'SHA256SUMS.txt') -Encoding utf8
Get-ChildItem -LiteralPath $releaseDir | Select-Object Name,Length
