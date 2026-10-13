param(
  [string]$OutputDirectory = 'dist-release',
  [Parameter(Mandatory = $true)][string]$WebView2RuntimeCab,
  [switch]$SkipChecks
)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path $PSScriptRoot -Parent
Set-Location -LiteralPath $taskRoot
$taskCargo = Join-Path $env:USERPROFILE '.cargo/bin'
if (Test-Path $taskCargo) { $env:PATH = "$taskCargo;$env:PATH" }
$taskRuntimeVersion = '155.0.4283.45'
$taskRuntimeUrl = 'https://msedge.sf.dl.delivery.mp.microsoft.com/filestreamingservice/files/01b0fbd9-6ea3-4e12-818a-a71f73f3855b/Microsoft.WebView2.FixedVersionRuntime.155.0.4283.45.x64.cab'
$taskExpectedCabHash = 'ec533bf1b7354d288703cf821a2d640d0130108224f93e330bb9c09faf3f8010'
$taskCab = (Resolve-Path -LiteralPath $WebView2RuntimeCab).Path
$taskCabHash = (Get-FileHash -LiteralPath $taskCab -Algorithm SHA256).Hash.ToLowerInvariant()
if ($taskCabHash -ne $taskExpectedCabHash) { throw 'Official Fixed Runtime CAB SHA256 mismatch' }
$taskVersion = (Get-Content -LiteralPath 'src-tauri/tauri.conf.json' -Raw -Encoding utf8 | ConvertFrom-Json).version
$taskOutput = [System.IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force $taskOutput | Out-Null
$taskPortableName = "Spacefold_${taskVersion}_x64-portable"
$taskPortable = Join-Path $taskOutput $taskPortableName
if (Test-Path -LiteralPath $taskPortable) { throw "Portable output already exists; choose a new OutputDirectory: $taskPortable" }
foreach ($taskNotice in @('LICENSE','THIRD-PARTY-LICENSES.txt','docs/licenses/WebView2-Fixed-License.html')) {
  if (-not (Test-Path -LiteralPath $taskNotice -PathType Leaf)) { throw "Missing redistribution notice: $taskNotice" }
}
if (-not $SkipChecks) {
  npm ci
  if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }
  npm test
  if ($LASTEXITCODE -ne 0) { throw 'Frontend checks failed' }
  # generate_context! embeds ../dist for custom-protocol checks on a clean clone.
  npm run build
  if ($LASTEXITCODE -ne 0) { throw 'Frontend production build failed' }
  cargo fmt --manifest-path src-tauri/Cargo.toml --check
  if ($LASTEXITCODE -ne 0) { throw 'Rust formatting failed' }
  cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
  if ($LASTEXITCODE -ne 0) { throw 'Rust review checks failed' }
  cargo test --locked --manifest-path src-tauri/Cargo.toml --all-features
  if ($LASTEXITCODE -ne 0) { throw 'Rust checks failed' }
}
# Keep the regular NSIS/system-Runtime installation as a separate supported output.
npm run tauri build
if ($LASTEXITCODE -ne 0) { throw 'Windows package failed' }
cargo build --locked --manifest-path src-tauri/Cargo.toml --release --features custom-protocol --bin spacefold-probe
if ($LASTEXITCODE -ne 0) { throw 'Production probe build failed' }
Copy-Item -LiteralPath 'src-tauri/target/release/spacefold.exe' -Destination (Join-Path $taskOutput 'Spacefold.exe')
$taskInstaller = "Spacefold_${taskVersion}_x64-setup.exe"
Copy-Item -LiteralPath (Join-Path 'src-tauri/target/release/bundle/nsis' $taskInstaller) -Destination (Join-Path $taskOutput $taskInstaller)
New-Item -ItemType Directory $taskPortable | Out-Null
Copy-Item -LiteralPath (Join-Path $taskOutput 'Spacefold.exe') -Destination (Join-Path $taskPortable 'Spacefold.exe')
# Expand this exact verified Microsoft CAB into a fresh private staging directory.
# Preserve every Runtime file, including Microsoft/Chromium third-party notices.
$taskRuntimeStaging = Join-Path $taskOutput ("webview2-staging-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $taskRuntimeStaging | Out-Null
& (Join-Path $env:SystemRoot 'System32/expand.exe') $taskCab '-F:*' $taskRuntimeStaging | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Official Fixed Runtime extraction failed' }
$taskRuntimeRoots = @(Get-ChildItem -LiteralPath $taskRuntimeStaging -Filter 'msedgewebview2.exe' -File -Recurse)
if ($taskRuntimeRoots.Count -ne 1) { throw 'Expected exactly one Fixed Runtime executable in the official archive' }
$taskRuntimeExecutable = $taskRuntimeRoots[0]
$taskSignature = Get-AuthenticodeSignature -LiteralPath $taskRuntimeExecutable.FullName
if ($taskSignature.Status -ne 'Valid' -or $taskSignature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') { throw 'Fixed Runtime executable does not have a valid Microsoft signature' }
if ($taskRuntimeExecutable.VersionInfo.ProductVersion -ne $taskRuntimeVersion) { throw 'Fixed Runtime version differs from the pinned official release' }
$taskRuntimeSource = $taskRuntimeExecutable.Directory.FullName
$taskRuntimeItems = @(Get-Item -LiteralPath $taskRuntimeSource) + @(Get-ChildItem -LiteralPath $taskRuntimeSource -Recurse -Force)
$taskRuntimeBytes = ($taskRuntimeItems | Where-Object { -not $_.PSIsContainer } | Measure-Object Length -Sum).Sum
if (@($taskRuntimeItems | Where-Object { $_.Attributes -band [System.IO.FileAttributes]::ReparsePoint }).Count -gt 0) { throw 'Fixed Runtime archive contains an unsupported reparse point' }
$taskRuntimeDestination = Join-Path $taskPortable 'WebView2Runtime'
Copy-Item -LiteralPath $taskRuntimeSource -Destination $taskRuntimeDestination -Recurse
foreach ($taskNotice in @('LICENSE','THIRD-PARTY-LICENSES.txt','docs/licenses/WebView2-Fixed-License.html')) {
  Copy-Item -LiteralPath $taskNotice -Destination (Join-Path $taskPortable (Split-Path $taskNotice -Leaf))
}
$taskCommit = git rev-parse HEAD
if ($LASTEXITCODE -ne 0) { $taskCommit = 'uncommitted source snapshot' }
$taskProbe = Join-Path $taskRoot 'src-tauri/target/release/spacefold-probe.exe'
$taskSqlite = & $taskProbe version | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or -not $taskSqlite.sqliteCompileOptions.threadsafe1 -or -not $taskSqlite.sqliteCompileOptions.enableApiArmor -or $taskSqlite.sqliteThreadsafe -ne 1) { throw 'Packaged probe SQLite threading/API checks were not verified' }
$taskAssets = @(Get-ChildItem -LiteralPath 'dist/assets' -File | ForEach-Object { [ordered]@{name=$_.Name;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()} })
[ordered]@{version=$taskVersion;sourceCommit=$taskCommit;scannerSourceCommit='a61e19e6db35a6721d051c89db6e3ba487474561';benchmarkProbeSha256='c0bb7f00f6e2d8f5551ec8e39f7a0cc8e63217fa6b5e92c6e5dfde965301ef84';frozenBenchmarkProbePath='artifacts/perf-round2/probes/spacefold-probe-final-a61e19e6.exe';probeFingerprintBoundary='probeSha256 identifies this packaged build; benchmarkProbeSha256 identifies the immutable performance build before portable startup changes';builtAt=(Get-Date).ToString('o');rust=(cargo --version);rustc=(rustc --version);node=(node --version);npm=(npm --version);frontendLockSha256=(Get-FileHash package-lock.json -Algorithm SHA256).Hash.ToLowerInvariant();rustLockSha256=(Get-FileHash src-tauri/Cargo.lock -Algorithm SHA256).Hash.ToLowerInvariant();productionFeatures=@('desktop','custom-protocol');probeSha256=(Get-FileHash -LiteralPath $taskProbe -Algorithm SHA256).Hash.ToLowerInvariant();sqlite=$taskSqlite;frontendAssets=$taskAssets;fixedRuntime=[ordered]@{version=$taskRuntimeVersion;architecture='x64';officialSource=$taskRuntimeUrl;cabSha256=$taskCabHash;executableSha256=(Get-FileHash -LiteralPath $taskRuntimeExecutable.FullName -Algorithm SHA256).Hash.ToLowerInvariant();microsoftSignature=$taskSignature.Status.ToString();unpackedBytes=$taskRuntimeBytes}} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $taskOutput 'BUILDINFO.json') -Encoding utf8
Copy-Item -LiteralPath (Join-Path $taskOutput 'BUILDINFO.json') -Destination (Join-Path $taskPortable 'BUILDINFO.json')
@'
间隙 Spacefold

便携版：完整解压 Spacefold_<版本>_x64-portable.zip 到本地文件夹，双击其中的 Spacefold.exe。
WebView2Runtime 文件夹必须保持在程序旁；便携版已携带运行时，无需安装或联网下载。
安装版：运行 Spacefold_<版本>_x64-setup.exe；系统缺少 WebView2 时，安装程序使用微软官方引导程序。
设置、扫描记录和 WebView 数据仍保存在当前用户 AppData，不写入程序目录。
选择扫描位置或输入绝对路径，扫描后在目录树 / 空间地图 / 层级透镜中查看。
右侧显示所选对象与文件操作；位置菜单可重扫来源、当前目录或搜索结果。
复制、移动、重命名与回收站先查看计划，再确认；失败逐项显示。
导出支持 CSV、XLSX、JSON、HTML，范围与是否包含下级可独立选择。
逻辑大小按路径统计；未知分配大小与部分扫描会明确标记。
包内 Fixed Runtime 遵循 WebView2-Fixed-License.html 的微软许可，MIT 仅覆盖 Spacefold 自身。
使用和再分发包内 Runtime 应遵守上述微软条款，保留 Runtime 内全部第三方 notices。
WebView2 SmartScreen 可能按微软隐私声明发送相关信息：https://privacy.microsoft.com/privacystatement
Windows 10 启动时会仅为包内 Runtime 准备微软规定的沙箱读取执行权限，不提升权限。
Windows 10 分支尚未在实机验证；Windows 11 以发布验收证据为准。
请解压到本地普通目录；微软 Fixed Runtime 不支持从网络位置或 UNC 路径运行。
Fixed Runtime 不会自动更新；后续安全更新由新版 Spacefold 发布包提供。
详细验证、性能比较与平台边界请查看源码 docs/acceptance.md 与 docs/performance.md。
'@ | Set-Content -LiteralPath (Join-Path $taskOutput '使用说明.txt') -Encoding utf8
Copy-Item -LiteralPath (Join-Path $taskOutput '使用说明.txt') -Destination (Join-Path $taskPortable '使用说明.txt')
$taskUtf8 = New-Object System.Text.UTF8Encoding($false)
$taskContentHashes = @(Get-ChildItem -LiteralPath $taskPortable -File -Recurse | Sort-Object FullName | ForEach-Object { "$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant())  $($_.FullName.Substring($taskPortable.Length + 1).Replace('\','/'))" })
[System.IO.File]::WriteAllLines((Join-Path $taskPortable 'CONTENTS-SHA256.txt'), $taskContentHashes, $taskUtf8)
Add-Type -AssemblyName System.IO.Compression.FileSystem
$taskZip = Join-Path $taskOutput ($taskPortableName + '.zip')
if (Test-Path -LiteralPath $taskZip) { throw 'Portable ZIP already exists; choose a new OutputDirectory' }
[System.IO.Compression.ZipFile]::CreateFromDirectory($taskPortable, $taskZip, [System.IO.Compression.CompressionLevel]::Optimal, $true)
$taskChecksumLines = @(Get-ChildItem -LiteralPath $taskOutput -File | Where-Object { $_.Extension -in @('.exe','.zip') } | Get-FileHash -Algorithm SHA256 | ForEach-Object { "$($_.Hash.ToLowerInvariant())  $(Split-Path $_.Path -Leaf)" })
[System.IO.File]::WriteAllLines((Join-Path $taskOutput 'SHA256SUMS.txt'), $taskChecksumLines, $taskUtf8)
# Delete only this freshly created staging tree, after resolving and checking that
# it is still a direct child of this invocation's output directory. Never clean an
# existing release/user directory as part of packaging.
$taskResolvedOutput = (Resolve-Path -LiteralPath $taskOutput).Path.TrimEnd('\')
$taskResolvedStaging = (Resolve-Path -LiteralPath $taskRuntimeStaging).Path
if ((Split-Path -Parent $taskResolvedStaging) -ne $taskResolvedOutput -or (Split-Path -Leaf $taskResolvedStaging) -notmatch '^webview2-staging-[0-9a-f]{32}$') { throw 'Staging cleanup containment check failed' }
Remove-Item -LiteralPath $taskResolvedStaging -Recurse -Force
Write-Host "Created installation package: $(Join-Path $taskOutput $taskInstaller)"
Write-Host "Created bundled-runtime portable package: $taskZip"
