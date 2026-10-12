param([string]$InstallDirectory = 'artifacts/install-test/Spacefold',[string]$EvidenceDirectory = 'artifacts/install-test')
$ErrorActionPreference = 'Stop'
$taskRoot = [System.IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
Set-Location -LiteralPath $taskRoot
$taskInstall = [System.IO.Path]::GetFullPath((Join-Path $taskRoot $InstallDirectory))
if (-not $taskInstall.StartsWith($taskRoot + [System.IO.Path]::DirectorySeparatorChar,[System.StringComparison]::OrdinalIgnoreCase)) { throw 'Test installation must stay inside this workspace' }
$taskEvidence = [System.IO.Path]::GetFullPath((Join-Path $taskRoot $EvidenceDirectory))
if (-not $taskEvidence.StartsWith($taskRoot + [System.IO.Path]::DirectorySeparatorChar,[System.StringComparison]::OrdinalIgnoreCase)) { throw 'Test evidence must stay inside this workspace' }
New-Item -ItemType Directory -Force $taskEvidence | Out-Null
$taskMarker = Join-Path $taskEvidence 'qa-owned.json'
$taskUninstallRoots = @('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall','HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall','HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall')
foreach ($taskRegistryRoot in $taskUninstallRoots) {
  foreach ($taskRegistered in @(Get-ChildItem -LiteralPath $taskRegistryRoot -ErrorAction SilentlyContinue | Get-ItemProperty -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -eq 'Spacefold' })) {
    if ($taskRegistered.InstallLocation -ne $taskInstall -and $taskRegistered.UninstallString -notlike "*$taskInstall*") { throw 'An existing Spacefold installation outside the isolated QA folder must be preserved' }
  }
}
if ((Test-Path -LiteralPath $taskInstall) -and @(Get-ChildItem -LiteralPath $taskInstall -Force).Count) {
  $taskExistingMarker = $taskMarker
  if (-not (Test-Path -LiteralPath $taskExistingMarker)) { $taskExistingMarker = Join-Path (Split-Path $taskInstall -Parent) 'qa-owned.json' }
  if (-not (Test-Path -LiteralPath $taskExistingMarker)) { throw 'Refusing to overwrite an installation without a QA ownership marker' }
  if ((Get-Content -LiteralPath $taskExistingMarker -Raw -Encoding utf8 | ConvertFrom-Json).installDirectory -ne $taskInstall) { throw 'QA ownership path mismatch' }
}
[ordered]@{installDirectory=$taskInstall;createdBy='Spacefold isolated installer QA'} | ConvertTo-Json | Set-Content -LiteralPath $taskMarker -Encoding utf8
$taskVersion = (Get-Content -LiteralPath 'src-tauri/tauri.conf.json' -Raw -Encoding utf8 | ConvertFrom-Json).version
$taskSetup = Join-Path $taskRoot "dist-release/Spacefold_${taskVersion}_x64-setup.exe"
$taskStarted = Get-Date
$taskProcess = Start-Process -FilePath $taskSetup -ArgumentList @('/S',"/D=$taskInstall") -WindowStyle Hidden -PassThru -Wait
if ($taskProcess.ExitCode -ne 0) { throw "Installer failed with exit code $($taskProcess.ExitCode)" }
$taskInstalledExe = Join-Path $taskInstall 'spacefold.exe'
if (-not (Test-Path -LiteralPath $taskInstalledExe)) { throw 'Installed executable missing' }
$taskPortableHash = (Get-FileHash -LiteralPath 'dist-release/Spacefold.exe' -Algorithm SHA256).Hash.ToLowerInvariant()
$taskInstalledHash = (Get-FileHash -LiteralPath $taskInstalledExe -Algorithm SHA256).Hash.ToLowerInvariant()
$taskMarkerOnly = $false
if ($taskInstalledHash -ne $taskPortableHash) {
  $taskEncoding = [System.Text.Encoding]::GetEncoding(28591)
  $taskPortableBytes = [System.IO.File]::ReadAllBytes((Resolve-Path 'dist-release/Spacefold.exe'))
  $taskInstalledBytes = [System.IO.File]::ReadAllBytes($taskInstalledExe)
  $taskPortableText = $taskEncoding.GetString($taskPortableBytes)
  $taskInstalledText = $taskEncoding.GetString($taskInstalledBytes)
  $taskMarker = '__TAURI_BUNDLE_TYPE_VAR_UNK'
  if ($taskPortableBytes.Length -ne $taskInstalledBytes.Length -or $taskPortableText.IndexOf($taskMarker) -lt 0 -or $taskPortableText.IndexOf($taskMarker) -ne $taskPortableText.LastIndexOf($taskMarker) -or -not [string]::Equals($taskPortableText.Replace($taskMarker,'__TAURI_BUNDLE_TYPE_VAR_NSS'),$taskInstalledText,[System.StringComparison]::Ordinal)) { throw 'Installed executable differs beyond the three-byte Tauri NSIS marker' }
  $taskMarkerOnly = $true
}
[ordered]@{installerExitCode=$taskProcess.ExitCode;elapsedSeconds=((Get-Date)-$taskStarted).TotalSeconds;installDirectory=$taskInstall;installedExecutable=$taskInstalledExe;portableAndInstalledPayloadMatch=$true;differsOnlyByTauriNsisMarker=$taskMarkerOnly;portableExeSha256=$taskPortableHash;installedExeSha256=$taskInstalledHash;installerSha256=(Get-FileHash -LiteralPath $taskSetup -Algorithm SHA256).Hash.ToLowerInvariant();freshMachineWebView2BootstrapNotTested=$true} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskEvidence 'installation.json') -Encoding utf8
Write-Output "Installed and hash-verified: $taskInstalledExe"
