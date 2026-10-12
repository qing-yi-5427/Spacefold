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
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'environment.json') -Encoding utf8
