$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $projectRoot
$localKey = Join-Path $projectRoot '.secrets/updater.key'
$localPassword = Join-Path $projectRoot '.secrets/updater.password'
$originalKey = $env:TAURI_SIGNING_PRIVATE_KEY
$originalPassword = $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
try {
    if (-not $env:TAURI_SIGNING_PRIVATE_KEY -and (Test-Path -LiteralPath $localKey)) {
        $env:TAURI_SIGNING_PRIVATE_KEY = [System.IO.File]::ReadAllText($localKey).Trim()
        if (Test-Path -LiteralPath $localPassword) {
            $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = [System.IO.File]::ReadAllText($localPassword).Trim()
        }
    }
    if ($env:TAURI_SIGNING_PRIVATE_KEY) {
        & npm.cmd run tauri -- build
        if ($LASTEXITCODE -ne 0) { throw '正式构建失败。' }
        & (Join-Path $PSScriptRoot 'package-release.ps1') -Signed
    } else {
        Write-Warning '未配置更新签名密钥，本次仅构建本地程序，不生成可发布的自动更新清单。'
        & npm.cmd run tauri -- build --config (Join-Path $PSScriptRoot 'tauri.unsigned.conf.json')
        if ($LASTEXITCODE -ne 0) { throw '本地构建失败。' }
        & (Join-Path $PSScriptRoot 'package-release.ps1')
    }
} finally {
    $env:TAURI_SIGNING_PRIVATE_KEY = $originalKey
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $originalPassword
}
