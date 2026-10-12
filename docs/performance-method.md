# 同机性能对照方法

本方法用于 Spacefold 与开源 Windows 磁盘工具的可重复对照。结果以本机实测日志为准；官方宣传数字不进入结果表。测试日期为 2026-10-12 至 2026-10-13，版本、CPU、内存、卷类型和 Windows/WebView2 版本由执行日志记录。

## 下载与版本锁定

只使用项目官方 Release 的 Windows x64 成品，保存下载地址、版本输出和 SHA-256：

- [WinDirStat 官方 Release](https://github.com/windirstat/windirstat/releases)。普通权限扫描；不执行 Run Elevated，不使用额外 MFT 快速扫描。
- [gdu 官方 Release](https://github.com/dundee/gdu/releases)。记录 `--version` 与 `--help`。
- [dua 官方 Release](https://github.com/Byron/dua-cli/releases)。记录 `--version`、根命令和 aggregate 子命令的帮助。

下方参数经官方文档核查，但执行前还必须与下载版本的帮助核对。上游 main/master 的文档可能包含尚未进入该成品的功能；不支持的参数不得偷偷省略。

## 两类工作量与统计口径

| 测试 | Spacefold | 对照工具 | 可作出的结论 |
|---|---|---|---|
| 默认扫描流程 | 枚举、元数据、完整树写 SQLite、目录聚合、可恢复记录 | gdu 非交互合计；dua aggregate；WinDirStat 常规扫描/CLI 保存 | 用户等待多久、真实结果是否正确；各工具实际承担的工作不同 |
| 保留完整树 | 同上，SQLite 持久化包含在扫描时间内 | gdu 完整 JSON 树；WinDirStat `/SaveTo`；dua 若成品支持完整快照则另测 | 包含完整结果保留成本的对照；格式和文件系统元数据字段仍有差异 |
| SQLite 保留完整树 | WAL、synchronous=NORMAL、16 MiB 页缓存、短批次提交 | gdu 5.38.0 的 `--db`，独立重读全部节点、根字节及完整性 | 同为数据库保留，但默认耐久配置与索引结构不同，另列模式 |

Spacefold 的大小口径是可读取主数据流的逻辑长度，硬链接按路径条目累计；不跟随目录 symlink/junction/mount point。gdu 的 `-a` 使用 apparent size；其平台实现有差异，5.38.0 的 Windows `getSyscallStats` 返回 `usage=info.Size()`、多链接标识 `0`，不能把 Unix 上的 inode 去重或分配空间统计直接套用到这个 Windows 成品。单纯合计模式没有保留和 Spacefold 同样完整的浏览索引。[gdu 5.38.0 Windows 实现](https://github.com/dundee/gdu/blob/v5.38.0/pkg/analyze/dir_other.go)

本机 gdu 5.38.0 成品已确认支持 `--db`。对应版本源码设置 `synchronous=OFF`、`journal_mode=MEMORY`、`cache_size=-64000`、`temp_store=MEMORY`；Spacefold 保持 WAL/NORMAL、16 MiB 页缓存和 4 KiB 数据库页。普通查询连接使用 `temp_store=FILE`；扫描专用连接将 `temp_store` 设为 MEMORY，其显式 TEMP 表仅保存最多 256 个已取目录 ID，不将持久节点或目录队列改为内存库。两者的索引和恢复边界不同，默认配置分别报告；同等耐久配置需另测，不为成绩降低正式程序的设置。[gdu 5.38.0 官方 SQLite 实现](https://github.com/dundee/gdu/blob/v5.38.0/pkg/analyze/sqlite.go)，[SQLite 临时存储说明](https://www.sqlite.org/pragma.html#pragma_temp_store)

上游研究锁定 gdu `v5.38.0`，对应提交 `387cf50377e53bad628603658cfb76fd4a54d718`，许可证为 [MIT](https://github.com/dundee/gdu/blob/v5.38.0/LICENSE.md)。其 SQLite 分析保留 prepared statements、并行处理目录后串行写库，可作为减少重复准备和重叠枚举/写入的参考。Spacefold 仍需短事务、扫描中可查询的修订以及取消后的已提交快照，不能直接采用上游的整次扫描事务。已有 gdu 数据库可能直接加载旧结果，所以每轮必须创建新的 `--db` 路径，并独立重读实际新树。

本轮 SQLite 编译只新增 `SQLITE_DEFAULT_MEMSTATUS=0`，关闭可选的全局分配统计，保留 `THREADSAFE=1` 与 `ENABLE_API_ARMOR`。`sqlite3_memory_used()`、`memory_highwater()`、全局 `status64` 内存统计以及 `SQLITE_MAX_MEMORY`、soft/hard heap limit 因此不可用；当前项目没有依赖这些统计或全局堆限制。Reader 字节许可、批次数量限制、每连接页缓存和 Windows 进程内存测量独立保留，页缓存预算也不等于整个程序的硬内存上限。[SQLite 编译选项](https://www.sqlite.org/compile.html#default_memstatus)，[全局堆限制条件](https://www.sqlite.org/c3ref/hard_heap_limit64.html)

该编译选项由仓库根 `.cargo/config.toml` 的 `LIBSQLITE3_FLAGS` 固定，`force=true` 防止外部环境悄悄覆盖。构建从仓库根或 `src-tauri` 执行；从外部目录仅传绝对 `--manifest-path` 不会自动加载项目配置，须显式指定 `--config`。每次记录实际成品 `probe version` 的 SQLite 版本、编译选项和 `sqliteThreadsafe`，不能仅凭配置文件宣称开关已生效。[Cargo 配置查找规则](https://doc.rust-lang.org/cargo/reference/config.html#hierarchical-structure)

大小升降序共用 `(scan_id,parent_id,sort_size,name COLLATE NOCASE,id)` 索引，其中 `sort_size=-logical_total`。默认降序保持同大小时名称、ID 升序；大小升序完整反向遍历该索引，同大小时名称、ID 降序。两种方向都保持确定顺序，普通分页、定位页和游标比较采用同一规则。游标查询键版本为 2，旧排序语义的游标直接拒绝。名称、未完成节点与非文件类型索引仍保留；已有库仅移除可被反向遍历替代的独立大小升序索引，不重写节点、不自动 VACUUM。已有库首次打开的迁移时间计入其外部成绩。

枚举与编码由两个目录 reader 和单 SQLite writer 重叠执行，普通文件使用枚举返回的元数据，非常规条目仍经过受限元数据工作队列。两个 reader 共用 32 个目录请求上限、一个 32 chunk 输出通道及每次扫描内的 8 MiB 字节许可；不为每个 reader 再分一套 8 MiB。数据 chunk 最多 256 条，数据容量达到 256 KiB 时发送，许可覆盖已入队及消费中的 chunk。两个正在构建的 chunk 另有开销。writer 批次仍为最多 8,000 条、4 MiB 已拥有数据阈值，收集时长达到 100 ms 也触发提交。该字节计数包含路径容量、元数据所拥有的字符串及编码后 raw path、payload、名称、扩展名、显示路径容量；不是仅路径有效长度。接纳一条后检查阈值，所以 4 MiB 是提交触发值而非提前拒绝的硬上限。向量槽位、待处理目录路径、特殊元数据队列与 SQLite 页缓存也有开销；8 MiB 不是扫描线程或整个应用的总内存上限。目录父节点先提交，实际枚举时重新核查目录安全性；取消与错误统一在 writer 锁外关闭通道、停止预算等待并 join 全部线程。枚举线程 panic 显式传播失败，不依赖另一线程仍持有的 Sender 断流。

Windows 数据库连接文件名与扫描来源路径分开处理。SQLite 3.53.2 的 Windows VFS 将起始双反斜杠的本地扩展路径也归入 UNC 共享锁句柄分支；本轮并发开关连接测试在该表示上稳定出现 `SQLITE_PROTOCOL`。应用仅为 SQLite 选择经身份验证的普通本地绝对文件名，canonical 数据目录、保护比较与扫描节点原始路径保持不变。普通表示要求组件没有末尾点、空格或设备名，目录及已有数据库的卷/文件 ID 必须一致；数据库完整文件名最多 251 个 UTF-16 单元，另预留 `-journal` 与 NUL。网络或映射网络盘、过长、特殊名称及无法等价验证的数据库位置，在任何 SQLite 打开/迁移之前报 `DATABASE_PATH_UNSUPPORTED`，保留原数据库，不偷偷换位置；这不是限制扫描来源的长路径或 UNC 读取。[SQLite 3.53.2 官方 Windows VFS](https://raw.githubusercontent.com/sqlite/sqlite/version-3.53.2/src/os_win.c)，[Windows 名称与命名空间规则](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file)

dua 的 `--apparent-size` 与 `--count-hard-links` 分别控制逻辑长度与逐链接计数；线程数必须记录，`0` 在 Windows 上使用逻辑处理器数量。其 aggregate 默认只做合计，和可恢复完整目录索引分开报告。[dua 官方参数定义](https://github.com/Byron/dua-cli/blob/main/src/options.rs)

WinDirStat 的 `/SaveTo` 写扫描树并退出，受保存的筛选、隐藏/保护文件排除、reparse 排除和线程设置影响；JSON/CSV 的 Logical Size 字段是字节数。启动成功或退出码 0 不能证明所有项目均可读取。固定普通用户令牌，以免提权启用另一条扫描路线。[WinDirStat 官方命令与导出说明](https://github.com/windirstat/windirstat/wiki/Command-Line-and-CSV)，[官方扫描模式说明](https://github.com/windirstat/windirstat)

主对照目录应没有硬链接、目录重定向、云占位和权限差异；这些边界另设正确性测试。实际目录如含此类项目，应保留差异条目及原因，不强求两个不同口径的总量相等。所有工具读取相同绝对目录；测试数据库、JSON、日志与临时文件位于扫描根之外。

## 参数模板

以下路径仅为模板，执行日志应保存实参。

```powershell
# gdu 默认非交互逻辑合计
gdu.exe -a -n -p --no-prefix --no-delete --no-view-file -s "G:\Benchmark\input"

# gdu 完整树导出，输出位于输入根之外
gdu.exe -a -p --no-delete --no-view-file -o "G:\Benchmark\reports\gdu-tree.json" "G:\Benchmark\input"

# gdu 5.38.0 完整 SQLite 保留，输出位于输入根之外
gdu.exe -a -n -p --no-prefix --no-delete --no-view-file --db="G:\Benchmark\reports\gdu.sqlite" -s "G:\Benchmark\input"

# dua 默认线程配置，逻辑大小并按路径计硬链接
dua.exe aggregate --apparent-size --count-hard-links --format bytes --threads 0 --stats "G:\Benchmark\input"

# WinDirStat 普通权限，保存完整树并退出；先清除非默认过滤设置
WinDirStat.exe /SaveTo "G:\Benchmark\reports\windirstat.json" "G:\Benchmark\input"
```

保留默认并发结果；如果另测相同并发上限，列为独立附表。不要混入默认结果。不要开启重复文件哈希、目录压缩或文件内容预览。保持相同杀毒、索引和后台服务状态，不为某一个工具修改系统缓存或优先级。

## 数据集、轮次与计时

本轮正式对照至少 5 轮，顺序轮换，所有轮次保留原始结果。两轮门槛实验仅用于判断是否继续优化，不作为达标结论：

1. 一个稳定的实际目录，记录文件/目录数、字节数、介质、路径和覆盖错误。
2. 一个专用的 100,000 个普通小文件目录（例如 100 个子目录各 1,000 个文件）。记录生成规则、每文件长度、总字节数；生成时间不计入扫描。
3. 可选百万条 SQLite 合成元数据只测查询/图形/滚动，不混入真实磁盘扫描成绩。

新库与已有库分别报告。已有库使用已保存真实扫描树的 4 KiB 数据库，先在计时外准备各轮独立副本；逐轮核对旧节点原始路径、payload、目录合计和扫描记录未变，同时核验新增完整树。不得通过自动 VACUUM 大库或只优化全新夹具库掩盖已有用户的实际成本。

不清系统缓存的连续轮次标为“自然缓存/重复扫描”，第一轮也不得自动称作冷缓存。只有明确重启或使用受控缓存流程且记录条件后才单列冷缓存。

开始计时前记录后台负载；若用户关闭后台程序或测试条件改变，建立新的结果目录并整组重跑。此前有负载的完整轮次和早停诊断仍保留，不混入低负载正式组，也不凭某一个慢值就删除该轮或断言具体程序造成了差异。

当前 `scripts/benchmark.py` 同时记录两条外部计时线，所有五轮原值与各自中位数都保留：

| 字段 | 起止边界 | 包含的工作 |
|---|---|---|
| `processExitElapsedSeconds` | 创建进程前 → 观测到进程退出且 stdout 读取线程结束 | 启动、真实扫描、完整持久化与聚合、线程回收、程序自身实际收尾、轮询与退出；不是精确 OS 退出时间戳 |
| `gateElapsedSeconds`（`elapsedSeconds` 同值） | 同一起点 → 独立最小完成核验结束 | 上一条 + 输出解析/文件检查；Spacefold 为退出 WAL 与 immutable 主库 status/COUNT/root 核验，WinDirStat 为非空报告文件检查 |
| `completionAuditElapsedMs` | 两条线之间的本轮实际差 | 独立观测器额外成本；不是扫描器耗时，也不是两个中位数之差 |

完整收尾要求全部扫描 worker join、finish 提交及扫描连接显式关闭后才发布运行时终态；probe 保留 50 ms 持久状态轮询，并等待运行时终态、按权威失败返回非零、最后重读状态。只看到 DB completed 不能保证后台连接已关闭。终态保证本扫描自身资源回收，不意味着 GUI 的其他并发连接已关闭；本轮主库/WAL 为零结论属于逐轮实测的独立 CLI。

gate 中的 Spacefold 审计以 `mode=ro&immutable=1` 忽略 WAL，只读主库，验证退出 WAL 为零、completed、finished、discovered/count、根逻辑字节及文件/目录数；不会 checkpoint/repair。正常带 WAL 读取只能证明已提交树，不能替代主库完整性。任一轮收尾核验失败即整组失败，原始数据保留，不选别轮替换。旧/新成品接受相同外部检查，但旧版未具备运行时终态等待，必须保留其设计限制。

全部 UTF-16 路径、单项字节、父子关系、每个目录合计、索引、历史列 digest 与完整性重读在 gate 之后执行，仍是该轮有效结果的必要条件；它不负责补完扫描，也不计作程序用时。Spacefold 内部任务耗时和 scanner profile 单独列。外部结束检查为 10 ms，probe 终态轮询为 50 ms，不为成绩更改轮询；若修改程序收尾或计时脚本，旧/新版均在新目录同方法整组复测，不拿旧边界成绩拼接。

首批结果时间指已提交、可以查询的节点；它可能只有目录或已知零字节，和第一份非零文件数据、stdout 第一份摘要或非空报告不同。100 ms 是批次收集时长触发条件，不是包含元数据调用、写库及线程收尾的取消完成上限。流水线的 reader 与 writer profile 可以重叠，其分项不能相加当作外部墙钟；`SQLITE_DBSTATUS_CACHE_SPILL` 的计数为零也不表示设置了 `PRAGMA cache_spill=OFF`。

## 内存、CPU 与 I/O

以固定采样周期（建议 50–100 ms）记录进程树工作集和 Private Bytes，报告峰值及采样周期。Spacefold 包含 exe 与其 WebView2 子进程；也可单列 Rust 核心，但不能把“核心内存”与其他工具“总内存”放入同一列。CLI 应包含对应所有子进程，排除驱动测试的 PowerShell/Node 本身。

记录 CPU 总时间、完成墙钟、文件数和逻辑字节数；可取得时记录读写字节与 SQLite/报告最终大小。GUI 常驻开销与数据扫描增长分别给出。持久化写入是 Spacefold 默认任务的一部分，不能在统计时删去这段成本。

## 正确性与发布结论

每轮核对原始字节总量、项目数、错误数和实际输出。Spacefold 完整扫描须独立只读重读 SQLite，核对全部原始 UTF-16 路径、单项逻辑长度、父子关系、每个目录合计、完整查询索引、终态、空 pending 队列及完整性检查；stdout 根合计正确不能替代此验证。报告根总量取根或规范化集合，不能相加所有目录与其文件的递归大小。另测取消、深度限制、不可读目录、链接与云目录的保守跳过，以及进程中断后的已提交快照。

结果表列全部 5 次耗时、范围、版本、模式、内存口径和中位数。主对照仍是这个 100,000 小文件目录的完整 Spacefold 扫描与普通权限 WinDirStat 完整树保存的五轮自然暖缓存；kernel、少字段、无索引或不持久化结果不能用于宣称达标。2026-10-13 用户接受五轮中位差为 0.0x 秒时“基本持平”，并要求停止抠严格 `<=`；报告保留原严格条件是否满足，逐轮 >=0.1 秒波动及完整范围也明确列出，不把“中位数持平”改写成每轮持平、统计等价或普遍领先。新库与历史库单列，保留实际目录结果，结论不得扩展成所有硬盘都更快。

最终冻结构建、当前四组完整结果目录、全部两字段五轮值由 [performance.md](performance.md) 标明；[performance-handoff.md](performance-handoff.md) 给出优化记录、代码入口及复现命令。首轮有效 36 轮、后续 42 轮、历史候选及失败组均保留各自构建/边界，不能替代当前证据。下载和官方帮助在 `artifacts/bench-tools/`。此前漂移及优化前轮次不与有效结果混合。
