$ErrorActionPreference = 'Stop'
$appRoot = Split-Path -Parent $PSScriptRoot
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=49387 --remote-debugging-address=127.0.0.1'
$env:WEBVIEW2_USER_DATA_FOLDER = Join-Path $appRoot '.tmp\webview-inspect'
$appExe = Join-Path $appRoot 'src-tauri\target\release\jx3-network-diagnostics.exe'
$appProcess = Start-Process -FilePath $appExe -WorkingDirectory $appRoot -WindowStyle Hidden -PassThru
try {
    Start-Sleep -Seconds 8
    $appProcess.Refresh()
    $appProcess | Select-Object Id,HasExited,Responding,MainWindowTitle,MainWindowHandle
    $allProcesses = Get-CimInstance Win32_Process
    $appIds = @($appProcess.Id)
    for ($depth = 0; $depth -lt 4; $depth++) {
        $children = @($allProcesses | Where-Object { $_.ParentProcessId -in $appIds -and $_.ProcessId -notin $appIds })
        $appIds += $children.ProcessId
    }
    $allProcesses | Where-Object ProcessId -in $appIds | Select-Object ProcessId,ParentProcessId,Name,CommandLine | ConvertTo-Json -Depth 3
    Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $_.OwningProcess -in $appIds } | Select-Object LocalAddress,LocalPort,OwningProcess
    try { Invoke-WebRequest -Uri 'http://127.0.0.1:49387/json/version' -NoProxy -TimeoutSec 3 | Select-Object -ExpandProperty Content } catch { Write-Output $_.Exception.Message }
} finally {
    if (-not $appProcess.HasExited) {
        $null = $appProcess.CloseMainWindow()
        if (-not $appProcess.WaitForExit(5000)) { $appProcess.Kill() }
    }
}
