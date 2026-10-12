# 扫描优化交接

## 当前结论与冻结点

业务源码冻结为 `a61e19e6db35a6721d051c89db6e3ba487474561`，版本 0.1.1，normal release probe SHA-256 `c0bb7f00f6e2d8f5551ec8e39f7a0cc8e63217fa6b5e92c6e5dfde965301ef84`。用户已接受五轮中位差为 0.0x 秒时“基本持平”，本轮不再调优。关闭游戏后的 100k 新库、宽目录、旧库稳态 gate 中位差为 +0.0602、+0.0656、+0.0101 秒；两条计时线、单轮 >=0.1 秒波动和完整原始目录见 [性能实测](performance.md)。相较同组原成品，新库/宽目录 gate 中位数减少约 27.9%/22.2%；这是组合优化效果，没有各单项收益的可靠分解。

当前四组共 80 次（新版 SF 30、原成品 SF 20、WDS 30）已独立解析全部 raw：退出/最小完成审计/完整正确性均通过；全部 50 次 SF 退出 WAL 为零、immutable 主库完整、全树重读正确，10 次新版旧库稳态历史列 digest 未变。源码独审结论为[范围内无阻塞](../artifacts/perf-round2/scanner-review-final.md)。[独立测试日志](../artifacts/perf-round2/final-a61e19e6-independent-tests/all-features.log)为 39 Rust 测试（11 lib / 27 core / 1 probe），另有 fmt、strict clippy all-targets/all-features、默认并行 core 27 及原回收用例串行 5 次通过。本交接只独立复核源码和既有日志，未制造并行测试/基准负载。新版 GUI、百万库及 Windows 生产交付以 [验收记录](acceptance.md) 为准。

## 已采用的策略

下面阶段 profile 是诊断，不能替代冻结构建的完整五轮。旧候选的 profile 与正常 probe 尚无 close-before-terminal 保证，不把内部时间称作完整进程收尾成绩。

| 策略与代码入口 | 实际变化 | 证据与归因边界 |
|---|---|---|
| [repository.rs](../src-tauri/src/repository.rs) `NodeInserter`；[scan.rs](../src-tauri/src/scan.rs) `flush_batch` | 整批持有 `prepare_cached` 的节点/pending insert，避免每条反复借还；全部持久字段、约束和事务不变 | [prepared 阶段](../artifacts/backend-perf-20261012/prepared/)保留；减少重复准备已确认，没有单独正式五轮支持其收益百分比 |
| `scan_sync` / `read_directories` / `ReaderBudget` | 两个目录 reader 枚举/编码与单 SQLite writer 重叠；普通文件复用枚举 hint，非常规项仍由 4 个元数据 worker 处理，目录 read_dir 前 fresh check 保留 | [单 reader 宽目录 profile](../artifacts/backend-perf-20261012/wide-reader-profile/one-reader-profile.json)内部 385.156 ms、readerWait 182.047 ms；[双 reader](../artifacts/backend-perf-20261012/two-readers/wide-profile.json)为 283.999/89.117 ms。目录 fresh check 与枚举累计反而 146.328+75.520 → 168.296+89.246 ms，工作仍完整执行但跨线程重叠；分项不能相加当墙钟 |
| 共享许可与短批次 | 每扫描 reader queued+消费共 8 MiB；请求/output 各 32、inflight 32；每 reader 构建 chunk 为 256 条/256 KiB 阈值；writer 为 8,000 条/4 MiB owned 容量/100 ms 收集触发 | 已实测长路径触发字节提交，见 [预算专项](../scripts/verify-long-path-budget.py)及最终验收。接纳一条后检查阈值，容许单项 overshoot；构建中 chunk、向量槽位、待处理路径、元数据队列和 16 MiB SQLite 页缓存另计。8 MiB 不是应用总 RAM 上限，100 ms 不是取消完成上限 |
| `.cargo/config.toml` `LIBSQLITE3_FLAGS` | 仅新增 `SQLITE_DEFAULT_MEMSTATUS=0` 且 `force=true`，保留 `THREADSAFE=1` / `ENABLE_API_ARMOR` | [memstatus0 阶段](../artifacts/backend-perf-20261012/memstatus0/) SQL 诊断减少约 10–20 ms；各时段负载不同，不作独立墙钟归因。全局 memory_used/highwater/status64 内存统计及 MAX_MEMORY/soft/hard heap limit 不可用；项目不依赖它们，OS 内存测量、每连接 db_status 和独立预算仍有效 |
| `page_cursor_key` / `page` / `node_page` | 大小降序与升序共用 `nodes_parent_size_v3`，旧库仅移除可替代 `asc_v2`。默认降序同值 name/id ASC 保留；升序整体反向，同值 name/id DESC；游标语义 key 2，拒旧键、负 size、非正 ID | [单索引 profile](../artifacts/backend-perf-20261012/single-size-index/profile-1.json) SQL 约 252→210 ms、cacheWrite 12,375→11,348 页为阶段诊断；正式效果只看当前组合。所有排序分页/定位同规则，极值和 Unicode 同值回归已通过；不 VACUUM、不重写历史节点 |
| `ordinary_database_filename` / `verified_sqlite_filename` | 仅 SQLite 私有连接使用身份等价的普通本地短绝对名；canonical 数据目录、保护比较和扫描原始 UTF-16 不变。目录/已有 DB 的卷与 File ID 相同；组件检查、数据库名 <=251 UTF-16，预留 `-journal` 与 NUL | [VFS 并发开关](../artifacts/backend-perf-20261012/sqlite-vfs-churn/results.json)三组 ordinary 各 32,000 读/2,000 写零错，extended 各 32 reader 出现 PROTOCOL。Windows VFS 共享锁竞争机制仍属源码推断。网络/映射网络盘、过长、特殊或无法证明等价位置在 SQLite open/DDL 前拒绝并保留原库，不自动搬库；扫描来源长路径/UNC 不因该修复改表示 |
| `scan_sync` / [probe](../src-tauri/src/bin/spacefold-probe.rs) `finished_scan`；[service.rs](../src-tauri/src/service.rs) `cancel_job` | 全部 worker join、finish commit、扫描连接显式 close 后才 emit 终态；close 错误按失败处理。probe 保留 50 ms DB 轮询，等 runtime 终态、失败优先、重读最终状态。cancel 在同一 event 锁内重核五种终态，防 completed 回退 cancelling | [旧收尾审计](../artifacts/perf-round2/checkpoint-boundary-audit.json)保留非零 WAL/主库 running 实证；`completed_event_observes_closed_scanner_and_complete_main_database` 与确定性晚 cancel 屏障回归通过。这是正确性修复，其完整收尾成本必须计入，不作为“负优化”撤回 |

[双 reader 语义对照](../artifacts/backend-perf-20261012/semantic-difference-two-readers/results.json)在四棵树逐 18 列、原路径/父路径、元数据与聚合核对相等。目录深度漏扫、取消断流、metadata panic 和一 reader panic/另一 Sender 存活时的退出/全部 join 回归均通过，不能为性能删除这些回收分支。

## 已尝试但撤回的策略

| 策略 | 实测结果与放弃原因 | 保留位置 |
|---|---|---|
| 8/16 KiB SQLite 页 | 新库阶段 profile 中位内部时间约 512.6/513.7 ms，但不同候选/时段不构成最终同边界收益；16 KiB 数据库约 55.33 MB，对照 4 KiB 约 48.14 MB。已有 WAL 4 KiB 库不能仅 PRAGMA 换页，静默 VACUUM 不可接受；最终新旧库都保留 4 KiB | [page8k](../artifacts/backend-perf-20261012/page8k/)、[page16k](../artifacts/backend-perf-20261012/page16k/) |
| writer 16k/8 MiB 大批次 | 提交 15→8、cacheWrite 12,375→12,045（仅约 -2.7%）；commit 约 155–175 ms 未明显下降，readerWait 约 13→80 ms、maxFlush 132–151 ms 变差。已回到 8,000/4 MiB，保首批和取消边界 | [batch16k-8MiB](../artifacts/backend-perf-20261012/batch16k-8MiB/) |
| 全字段 raw_bind 缓存 / Box / 仅五个重复 Text typed 缓存 | 全字段 bindings 1,701,700→439,376，但缓存管理抵消收益，SQL 约 250–254 ms（原 252–262）；Box 未证明稳定至少 30 ms 改善。Text 阶段 SQL 244 ms，约 8 ms 小差不足以引入更复杂绑定状态。全部撤回，未用 SQLITE_STATIC 或借用跨 step 的数据 | [safe-bind](../artifacts/backend-perf-20261012/safe-bind/)、[safe-bind-box](../artifacts/backend-perf-20261012/safe-bind-box/)、[typed-text-bind](../artifacts/backend-perf-20261012/typed-text-bind/)，实验源码各 `*-bind-attempt/` |
| 一次 64 行 INSERT | 减少调用同时增加参数拷贝/动态 SQL，没有显著完整流程收益，也没有独立正式五轮归因；撤回 | [multirow64](../artifacts/backend-perf-20261012/multirow64/) |
| 紧凑 UTF-16 路径 codec | 五轮交替 full schema/all rows/all indexes/WAL-NORMAL/close 的串行存储 replay：plain/compact 中位 1039.324/1034.993 ms，仅减 4.33 ms（0.4%）；DB 48,144,384→40,161,280 B，但编码约 8 ms、插入成本抵消提交/关闭节约。它不是流水线真实扫描；双表示查询/刷新兼容未验收，生产保持原始 UTF-16，无 codec 新依赖 | [summary](../artifacts/backend-perf-20261012/path-storage-replay-v1/summary.json)、[全部 raw](../artifacts/backend-perf-20261012/path-storage-replay-v1/results.json)、`path-codec-attempt/`、`storage-replay.py` |

16 MiB 连接 cache 的阶段 `cacheSpillPages=0` 只支持“未观察到 spill”，不表示已禁用 spill，也不支持盲目扩大缓存；未采用 `synchronous=OFF`、MEMORY journal、cache_spill=OFF 或禁用线程安全。所有阶段正常/内部/外部指标不同，50 ms probe 轮询和后台负载也影响短任务，不从这些记录拼出正式提升百分比。

## 后续复现入口

**`artifacts/` 被 Git 忽略；干净 clone 不含原始结果、十万/目录形状夹具、旧 probe、工具成品或 Python QA 依赖。** 本文命令直接用于这份本地交付工作区；迁移时先复制并校验本地证据包，或按 `scripts/prepare-benchmark.mjs` / `prepare-benchmark-shapes.mjs` 重新生成夹具、重新取得锁定工具与依赖，再记录新指纹，不沿用本机时间。

以下为后续 agent 的命令模板，本轮未再执行。先确认文件所有权；保留冻结成品与 raw，普通权限从项目根执行，测试输出只用新建临时夹具，源目录只读。独立正式组必须没有同时编译或其他 agent 的重负载，不关闭用户进程、不清缓存、不关闭安全软件。脚本依赖本地 `artifacts/python-qa/psutil` 和已锁定的三个官方工具。

```powershell
Set-Location -LiteralPath '<你的 Spacefold 项目目录>'
Get-FileHash -LiteralPath 'src-tauri/target/release/spacefold-probe.exe' -Algorithm SHA256
& .\src-tauri\target\release\spacefold-probe.exe version

# 后续改动完成后先验证；不要与正式计时并行
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-features
cargo test --manifest-path src-tauri/Cargo.toml --test core

# 从项目根加载 .cargo/config.toml，保留 default + custom-protocol
cargo build --manifest-path src-tauri/Cargo.toml --release --features custom-protocol --bin spacefold-probe
& .\src-tauri\target\release\spacefold-probe.exe version

# 每次唯一新目录；整组五轮，不以早停替代正式结论
$taskRun = 'artifacts/perf-round2/repeat-' + (Get-Date -Format 'yyyyMMdd-HHmmss')
$env:BENCHMARK_OUTPUT = $taskRun
$env:BENCHMARK_PROBE = (Resolve-Path -LiteralPath 'src-tauri/target/release/spacefold-probe.exe').Path
if ($env:SPACEFOLD_PROFILE) { throw '正式组禁止 profile' }
python scripts/record-benchmark-load.py "$taskRun/background-before.json"
& .\scripts\write-benchmark-environment.ps1 -OutputDirectory $taskRun -ProbeSourceCommit (git rev-parse HEAD)
python scripts/benchmark.py --rounds 5 --early-gate-ratio 999 --sources artifacts/perf-round2/wide-primary-sources.json --tools windirstat-full-tree,spacefold-sqlite,spacefold-old-sqlite --comparison-probe artifacts/perf-round2/probes/spacefold-probe-old-d052183.exe
python scripts/record-benchmark-load.py "$taskRun/background-after.json"
```

旧库稳态使用另一个全新 `$taskRun`，改为 `--sources artifacts/perf-round2/primary-source.json --tools windirstat-full-tree,spacefold-sqlite --seed-database artifacts/perf-round2/final-381f2577-legacy-steady-seed-v2/spacefold.sqlite3 --index-layout current`。这个 seed 已迁移；首次升级另用未迁移历史 seed，二者不可混称。脚本校验 seed 已检查点、4 KiB、原树 digest，正式计时前复制到各轮独立目的位置，拒绝复用已有输出；最后核对成品指纹未变、原输入目录未漂移、全部收尾审计通过。

复核 raw 时分别按 `dataset/tool` 对两个计时字段取五轮中位数，读取 `completionAuditElapsedMs`、`walBytesAfterExit`、`completionAudit`、`persistenceVerification`、`historyVerification`；不要把 `summary.json` 的 gate 中位数误当 process exit。完整树重读在 gate 外，不进行 checkpoint 修补。[observer 拆分](../artifacts/perf-round2/scanner-wall-observer-breakdown.json)包含历史 ca、当前有负载及关闭游戏新库/宽目录；不含最后旧库稳态，后者以其 raw 为准。最小审计通常 100k 约 7 ms、宽目录约 1 ms，但首轮 22.339/36.904 ms 必须保留。

## 必须保持与尚未验证

- 保持 WAL/NORMAL、完整持久字段与必要查询索引、原始 UTF-16、整数精度、未知分配/身份保持 null 和 coverage。普通文件 hint 不替代操作计划前的身份/父链重核；链接/云目录/root/AppData 保护不减弱，回收站无永久删除降级。
- 保持 parent 先提交再派 reader、单 coordinator 分配 ID、单 writer、有界许可与队列、cancel/panic 唤醒和全部 worker join 后终态。partial/failed 已提交 revision、刷新原子性与稳定 ID、晚 cancel 不回退终态均保留。终态保证本扫描连接关闭；GUI 其他读连接存在时，不能把 CLI WAL0 证据推广为每次事件必定主库无 WAL。
- 保持数据库 alias 身份等价且 fail closed，不静默搬用户库、VACUUM 或丢字段；导出 HANDLE/临时对象身份/父链重核、取消保留原目标和原文件操作保护不改。上游研究只取思路，不把 GPL 代码复制进 MIT 项目。

[当前冻结构建的单次正确性复验](../artifacts/perf-round2/final-a61e19e6-scan-correctness/results.json)已覆盖首次旧库升级（历史列保持）、MSVC 和深目录，分别重读 100,101/376/1,345 新节点，全部 WAL0/immutable 主库/完整树通过；它们仍未重做正式五轮性能对照。gdu/dua 当前同收尾边界、冷缓存/HDD/网络盘/整卷或百万真实文件扫描未测，旧结果不能补作新验收。真实云占位、断卷/断网、配额账户和缺 WebView2 新系统仍依赖实机条件，未建立就保留未测。最终百万合成库、GUI 和生产包的新增证据由 QA 写入 [acceptance.md](acceptance.md)，后续只把对应构建/路径/边界补回本节，不将单次正确性变成性能结论。

将来的优化可先拆出实际扫描、natural close、event 和 50 ms 轮询等待，或调查宽目录 metadataWait/ancestor aggregation/queue、单目录 SQL/index 页写放大及真实百万库查询；修改观测脚本须旧/新同方法复测，不能单改轮询凑分数。当前无需继续这些工作。
