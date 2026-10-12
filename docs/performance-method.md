# 同机性能对照方法

本方法用于 Spacefold 与开源 Windows 磁盘工具的可重复对照。结果以本机实测日志为准；官方宣传数字不进入结果表。测试日期为 2026-10-12，版本、CPU、内存、卷类型和 Windows/WebView2 版本由执行日志记录。

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

Spacefold 的大小口径是可读取主数据流的逻辑长度，硬链接按路径条目累计；不跟随目录 symlink/junction/mount point。gdu 的 `-a` 使用 apparent size，默认会对硬链接去重；它的非交互模式在不设置 top/depth 时只保留顶级合计，不能把其内存优势解释为保存了和 Spacefold 同样完整的树。[gdu 官方说明](https://github.com/dundee/gdu/blob/master/README.md)

本机 gdu 5.38.0 成品已确认支持 `--db`。对应版本源码设置 `synchronous=OFF`、`journal_mode=MEMORY`、`cache_size=-64000`、`temp_store=MEMORY`；Spacefold 保持 WAL/NORMAL、16 MiB 页缓存和 FILE 临时存储。两者保留完整节点，但 SQLite 配置、索引和恢复边界不同。各自默认配置的结果分别列出，同等耐久配置的评估需另列对照，不为追逐成绩降低正式程序的设置。[gdu 5.38.0 官方 SQLite 实现](https://github.com/dundee/gdu/blob/v5.38.0/pkg/analyze/sqlite.go)

dua 的 `--apparent-size` 与 `--count-hard-links` 分别控制逻辑长度与逐链接计数；线程数必须记录，`0` 在 Windows 上使用逻辑处理器数量。其 aggregate 默认只做合计，和可恢复完整目录索引分开报告。[dua 官方参数定义](https://github.com/Byron/dua-cli/blob/main/src/options.rs)

WinDirStat 的 `/SaveTo` 写扫描树并退出，受保存的筛选、隐藏/保护文件排除、reparse 排除和线程设置影响；JSON/CSV 的 Logical Size 字段是字节数。启动成功或退出码 0 不能证明所有项目均可读取。固定普通用户令牌，以免提权启用另一条扫描路线。[WinDirStat 官方命令与导出说明](https://github.com/windirstat/windirstat/wiki/Command-Line-and-CSV)，[官方扫描模式说明](https://github.com/windirstat/windirstat)

主对照目录应没有硬链接、目录重定向、云占位和权限差异；这些边界另设正确性测试。实际目录如含此类项目，应保留差异条目及原因，不强求两个不同口径的总量相等。所有工具读取相同绝对目录；测试数据库、JSON、日志与临时文件位于扫描根之外。

## 参数模板

以下路径仅为模板，执行日志应保存实参。

```powershell
# gdu 默认非交互逻辑合计
gdu.exe -a -n -p --no-prefix --no-delete --no-view-file "G:\Benchmark\input"

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

每款工具至少 3 轮，顺序轮换，所有轮次保留原始结果：

1. 一个稳定的实际目录，记录文件/目录数、字节数、介质、路径和覆盖错误。
2. 一个专用的 100,000 个普通小文件目录（例如 100 个子目录各 1,000 个文件）。记录生成规则、每文件长度、总字节数；生成时间不计入扫描。
3. 可选百万条 SQLite 合成元数据只测查询/图形/滚动，不混入真实磁盘扫描成绩。

不清系统缓存的连续轮次标为“自然缓存/重复扫描”，第一轮也不得自动称作冷缓存。只有明确重启或使用受控缓存流程且记录条件后才单列冷缓存。

记录两条计时线：应用/命令启动至完成的外部墙钟；Spacefold 从 `start_scan` 接受至最终状态的任务耗时。命令退出、报告存在、状态 completed 和完整性检查共同确定终点。轮询检测完成引入的采样上界应公布。WinDirStat GUI扫描若不能自动读任务结束，应另列手工判定误差，不与毫秒级内部计时混列。

## 内存、CPU 与 I/O

以固定采样周期（建议 50–100 ms）记录进程树工作集和 Private Bytes，报告峰值及采样周期。Spacefold 包含 exe 与其 WebView2 子进程；也可单列 Rust 核心，但不能把“核心内存”与其他工具“总内存”放入同一列。CLI 应包含对应所有子进程，排除驱动测试的 PowerShell/Node 本身。

记录 CPU 总时间、完成墙钟、文件数和逻辑字节数；可取得时记录读写字节与 SQLite/报告最终大小。GUI 常驻开销与数据扫描增长分别给出。持久化写入是 Spacefold 默认任务的一部分，不能在统计时删去这段成本。

## 正确性与发布结论

每轮核对原始字节总量、项目数、错误数和实际输出；报告根总量取根或规范化集合，不能相加所有目录与其文件的递归大小。记录取消后的真实最终状态与保留项目，另测 UI 是否仍能响应。

结果表分别列 3 次耗时、范围、版本、模式、内存口径和中位数。若差异来自硬链接、未读取目录、过滤或完整索引成本，在同一行解释。只陈述实测：例如“这个目录的 3 次自然缓存扫描中位数”，不扩展成“所有硬盘快 N 倍”。

首轮有效 36 轮及新增 SQLite 对照的末轮 42 轮原始结果分别保留，最终路径与构建指纹由 [performance.md](performance.md) 标明；下载和官方帮助在 `artifacts/bench-tools/`。此前漂移及优化前轮次另存为历史，不与有效结果混合。此方法文件不包含虚构成绩。
