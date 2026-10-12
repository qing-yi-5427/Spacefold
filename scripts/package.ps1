param([string]$OutputDirectory = 'dist-release',[switch]$SkipChecks)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path $PSScriptRoot -Parent
Set-Location -LiteralPath $taskRoot
$taskCargo = Join-Path $env:USERPROFILE '.cargo/bin'
if (Test-Path $taskCargo) { $env:PATH = "$taskCargo;$env:PATH" }
if (-not $SkipChecks) {
  npm ci
  if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }
  npm test
  if ($LASTEXITCODE -ne 0) { throw 'Frontend checks failed' }
  cargo fmt --manifest-path src-tauri/Cargo.toml --check
  if ($LASTEXITCODE -ne 0) { throw 'Rust formatting failed' }
  cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets --features custom-protocol -- -D warnings
  if ($LASTEXITCODE -ne 0) { throw 'Rust review checks failed' }
  cargo test --locked --manifest-path src-tauri/Cargo.toml
  if ($LASTEXITCODE -ne 0) { throw 'Rust checks failed' }
}
npm run tauri build
if ($LASTEXITCODE -ne 0) { throw 'Windows package failed' }
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null
Copy-Item -LiteralPath 'src-tauri/target/release/spacefold.exe' -Destination (Join-Path $OutputDirectory 'Spacefold.exe')
$taskVersion = (Get-Content -LiteralPath 'src-tauri/tauri.conf.json' -Raw -Encoding utf8 | ConvertFrom-Json).version
$taskInstaller = "Spacefold_${taskVersion}_x64-setup.exe"
Copy-Item -LiteralPath (Join-Path 'src-tauri/target/release/bundle/nsis' $taskInstaller) -Destination (Join-Path $OutputDirectory $taskInstaller)
$taskCommit = git rev-parse HEAD
if ($LASTEXITCODE -ne 0) { $taskCommit = 'uncommitted source snapshot' }
$taskProbe = Join-Path $taskRoot 'src-tauri/target/release/spacefold-probe.exe'
$taskSqlite = & $taskProbe version | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or -not $taskSqlite.sqliteCompileOptions.threadsafe1 -or -not $taskSqlite.sqliteCompileOptions.enableApiArmor -or $taskSqlite.sqliteThreadsafe -ne 1) { throw 'Packaged probe SQLite threading/API checks were not verified' }
$taskAssets = @(Get-ChildItem -LiteralPath 'dist/assets' -File | ForEach-Object { [ordered]@{name=$_.Name;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()} })
[ordered]@{version=$taskVersion;sourceCommit=$taskCommit;builtAt=(Get-Date).ToString('o');rust=(cargo --version);rustc=(rustc --version);node=(node --version);npm=(npm --version);frontendLockSha256=(Get-FileHash package-lock.json -Algorithm SHA256).Hash.ToLowerInvariant();rustLockSha256=(Get-FileHash src-tauri/Cargo.lock -Algorithm SHA256).Hash.ToLowerInvariant();productionFeatures=@('desktop','custom-protocol');probeSha256=(Get-FileHash -LiteralPath $taskProbe -Algorithm SHA256).Hash.ToLowerInvariant();sqlite=$taskSqlite;frontendAssets=$taskAssets} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'BUILDINFO.json') -Encoding utf8
$taskChecksumLines = @(Get-ChildItem -LiteralPath $OutputDirectory -Filter '*.exe' | Get-FileHash -Algorithm SHA256 | ForEach-Object { "$($_.Hash.ToLowerInvariant())  $(Split-Path $_.Path -Leaf)" })
[System.IO.File]::WriteAllLines([System.IO.Path]::GetFullPath((Join-Path $OutputDirectory 'SHA256SUMS.txt')), $taskChecksumLines, (New-Object System.Text.UTF8Encoding($false)))
@'
间隙 Spacefold

直接运行 Spacefold.exe，或安装 Spacefold_<版本>_x64-setup.exe。
选择扫描位置或输入绝对路径，扫描后在目录树 / 空间地图 / 层级透镜中查看。
右侧显示所选对象与文件操作；位置菜单可重扫来源、当前目录或搜索结果。
复制、移动、重命名与回收站先查看计划，再确认；失败逐项显示。
导出支持 CSV、XLSX、JSON、HTML，范围与是否包含下级可独立选择。
逻辑大小按路径统计；未知分配大小与部分扫描会明确标记。
需要 WebView2 Runtime；安装包缺失时下载官方引导程序。
详细验证、性能比较与平台边界请查看源码 docs/acceptance.md 与 docs/performance.md。
'@ | Set-Content -LiteralPath (Join-Path $OutputDirectory '使用说明.txt') -Encoding utf8
