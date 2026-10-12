param([string]$OutputDirectory = 'artifacts/benchmarks-final-42')
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path $PSScriptRoot -Parent
Set-Location -LiteralPath $taskRoot
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null
$taskOs = Get-CimInstance Win32_OperatingSystem
$taskCpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$taskComputer = Get-CimInstance Win32_ComputerSystem
$taskProbe = Get-Item 'src-tauri/target/release/spacefold-probe.exe'
$taskCommit = cmd /c 'git rev-parse --verify HEAD 2>NUL'
if ($LASTEXITCODE -ne 0) { $taskCommit = 'uncommitted source snapshot' }
$taskIdentity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
$taskPrincipal = [System.Security.Principal.WindowsPrincipal]::new($taskIdentity)
$taskWdsOptions = Get-ItemProperty 'HKCU:\Software\WinDirStat\WinDirStat\Options' -ErrorAction SilentlyContinue
$taskSavedOptions = [ordered]@{}
if ($taskWdsOptions) { $taskWdsOptions.PSObject.Properties | Where-Object { $_.Name -notlike 'PS*' } | ForEach-Object { $taskSavedOptions[$_.Name] = $_.Value } }
$taskToolPaths = @('artifacts/bench-tools/gdu/gdu_windows_amd64.exe','artifacts/bench-tools/dua/dua-v2.45.1-x86_64-pc-windows-msvc/dua.exe','artifacts/bench-tools/windirstat/x64/WinDirStat.exe')
[ordered]@{
  recordedAt = (Get-Date).ToString('o')
  probeSha256 = (Get-FileHash $taskProbe.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
  probeLastWrite = $taskProbe.LastWriteTime.ToString('o')
  probeVersion = (& $taskProbe.FullName version | ConvertFrom-Json)
  os = $taskOs.Caption
  osVersion = $taskOs.Version
  architecture = $taskOs.OSArchitecture
  cpu = $taskCpu.Name.Trim()
  logicalProcessors = $taskComputer.NumberOfLogicalProcessors
  totalPhysicalMemoryBytes = $taskComputer.TotalPhysicalMemory
  disks = @(Get-CimInstance Win32_DiskDrive | Select-Object Model,MediaType,Size)
  drives = @(Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' | Select-Object DeviceID,FileSystem,Size)
  sourceCommit = $taskCommit
  sourceStatus = @(git status --short)
  isElevated = $taskPrincipal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)
  profileEnvironmentEnabled = [bool]$env:SPACEFOLD_PROFILE
  compilerProcesses = @(Get-Process cargo,rustc,cl,link -ErrorAction SilentlyContinue | Select-Object Id,ProcessName)
  spacefoldProcesses = @(Get-Process spacefold -ErrorAction SilentlyContinue | Select-Object Id,ProcessName,Path)
  windirstatOptions = $taskSavedOptions
  toolExecutables = @($taskToolPaths | ForEach-Object { $taskTool = Get-Item -LiteralPath $_; [ordered]@{path=$taskTool.FullName;sha256=(Get-FileHash -LiteralPath $taskTool.FullName -Algorithm SHA256).Hash.ToLowerInvariant();productVersion=$taskTool.VersionInfo.ProductVersion} })
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'environment.json') -Encoding utf8
