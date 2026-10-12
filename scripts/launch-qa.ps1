param(
  [string]$Executable = 'src-tauri/target/debug/spacefold.exe',
  [int]$DebugPort = 9223,
  [switch]$Fixture,
  [string]$DataDirectory = 'artifacts/qa-app-data'
)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path $PSScriptRoot -Parent
$taskExe = [System.IO.Path]::GetFullPath((Join-Path $taskRoot $Executable))
$taskData = [System.IO.Path]::GetFullPath((Join-Path $taskRoot $DataDirectory))
New-Item -ItemType Directory -Force $taskData | Out-Null
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$DebugPort"
$env:SPACEFOLD_DATA_DIR = $taskData
$env:WEBVIEW2_USER_DATA_FOLDER = Join-Path $taskData 'webview2-qa'
$taskArgs = @('"--qa-data-dir=' + $taskData + '"')
if ($Fixture) { $taskArgs += '--fixture=1' }
$taskProcess = Start-Process -FilePath $taskExe -ArgumentList $taskArgs -WindowStyle Hidden -PassThru
Write-Output "Native PID=$($taskProcess.Id) CDP=http://127.0.0.1:$DebugPort Data=$taskData"
