param([switch]$Signed)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$app = Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $projectRoot 'package.json') | ConvertFrom-Json
$appVersion = $app.version
$releaseDir = Join-Path $projectRoot 'release'
$appExe = Join-Path $projectRoot 'src-tauri\target\release\jx3-network-diagnostics.exe'
$installerExe = Join-Path $projectRoot ('src-tauri\target\release\bundle\nsis\剑网三网络诊断_{0}_x64-setup.exe' -f $appVersion)
$installerName = 'jx3-network-diagnostics-{0}-x64-setup.exe' -f $appVersion
if (-not (Test-Path -LiteralPath $appExe)) { throw '请先构建正式版 EXE。' }
if (-not (Test-Path -LiteralPath $installerExe)) { throw '未找到本版本安装包，打包中止。' }
if ($Signed -and -not (Test-Path -LiteralPath "$installerExe.sig")) { throw '未找到更新签名，禁止生成更新清单。' }
New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null
$versionedName = 'jx3-network-diagnostics-{0}.exe' -f $appVersion
Copy-Item -LiteralPath $appExe -Destination (Join-Path $releaseDir $versionedName)
try {
    Copy-Item -LiteralPath $appExe -Destination (Join-Path $releaseDir 'jx3-network-diagnostics.exe')
} catch {
    Write-Warning "release 中的旧 EXE 正在使用，请运行 $versionedName。"
}
try {
    Copy-Item -LiteralPath $appExe -Destination (Join-Path $projectRoot 'jx3-network-diagnostics.exe')
} catch {
    Copy-Item -LiteralPath $appExe -Destination (Join-Path $projectRoot $versionedName)
    Write-Warning "根目录的旧 EXE 正在使用，新版已放到根目录 $versionedName。"
}
Copy-Item -LiteralPath $installerExe -Destination (Join-Path $releaseDir $installerName)
Copy-Item -LiteralPath (Join-Path $projectRoot 'docs\使用说明.txt') -Destination $releaseDir
$utf8 = New-Object System.Text.UTF8Encoding($false)
$hashes = @($versionedName, $installerName) | ForEach-Object {
    $stream = [System.IO.File]::OpenRead((Join-Path $releaseDir $_))
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hash = [System.BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '').ToLowerInvariant()
        '{0}  {1}' -f $hash, $_
    } finally {
        $algorithm.Dispose()
        $stream.Dispose()
    }
}
[System.IO.File]::WriteAllLines((Join-Path $releaseDir 'SHA256SUMS.txt'), [string[]]$hashes, $utf8)
if ($Signed) {
    $signature = [System.IO.File]::ReadAllText("$installerExe.sig").Trim()
    Copy-Item -LiteralPath "$installerExe.sig" -Destination (Join-Path $releaseDir "$installerName.sig")
    $changelog = [System.IO.File]::ReadAllText((Join-Path $projectRoot 'CHANGELOG.md'))
    $pattern = '(?ms)^## ' + [regex]::Escape($appVersion) + ' [^\r\n]*\r?\n(.*?)(?=^## |\z)'
    $match = [regex]::Match($changelog, $pattern)
    if (-not $match.Success) { throw 'CHANGELOG.md 缺少本版本更新说明。' }
    $notes = $match.Groups[1].Value.Trim()
    $manifest = @{
        version = $appVersion
        notes = $notes
        pub_date = $app.releaseDate + 'T00:00:00+08:00'
        platforms = @{
            'windows-x86_64' = @{
                signature = $signature
                url = 'https://github.com/Vintcet/jx3-network-diagnostics/releases/download/v{0}/{1}' -f $appVersion, $installerName
            }
        }
    }
    [System.IO.File]::WriteAllText((Join-Path $releaseDir 'latest.json'), ($manifest | ConvertTo-Json -Depth 5), $utf8)
    [System.IO.File]::WriteAllText((Join-Path $releaseDir 'release-notes.md'), $notes, $utf8)
} else {
    foreach ($name in @('latest.json', 'release-notes.md', "$installerName.sig")) {
        $staleFile = Join-Path $releaseDir $name
        if (Test-Path -LiteralPath $staleFile) { Remove-Item -LiteralPath $staleFile }
    }
}
Write-Output "已复制本次 EXE 到项目根目录，并打包 release/$versionedName。"
