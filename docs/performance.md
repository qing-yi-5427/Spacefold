# 扫描性能实测

2026-10-12 在同一台 Windows 机器以普通权限测试三个官方 Windows x64 工具和 Spacefold release 核心。下表来自无并发编译负载的 [最终原始结果](../artifacts/benchmarks-final-42/results.json)，共 42 次执行，全部完成原始字节数与条目数核对。

环境：Windows 11 专业版 10.0.26300 x64，Ryzen 7 9800X3D（8 核 / 16 逻辑处理器），51,125,407,744 字节物理内存，C:/G: 均为 NTFS、本机 SSD。完整机器与卷元数据在[环境记录](../artifacts/benchmarks-final-42/environment.json)；未把卷字母推断为某一个物理 SSD 型号。内置 SQLite 3.53.2。该轮 release probe SHA-256 为 `8efbfb00836e80c360c541299cca6cd0ec14c89f9f2515dad6edfcf156ad255a`。

## 已锁定的官方成品

| 工具 | 版本 | 官方来源 | 下载归档 SHA-256 |
|---|---|---|---|
| gdu | 5.38.0 | [官方 Release](https://github.com/dundee/gdu/releases/tag/v5.38.0) | `22e6616e0bdc25d8d803934b4d3189996ebf74de8ac9b3635a4e4e079c3d7f8a` |
| dua | 2.45.1 | [官方 Release](https://github.com/Byron/dua-cli/releases/tag/v2.45.1) | `48c245f987c5359df0302202e6338e3235e4a4724db58d7d66138b096e8b2015` |
| WinDirStat | 2.9.3 | [官方 Release](https://github.com/windirstat/windirstat/releases/tag/release/v2.9.3) | `c980fee14389eb3a160415b0a07361900f3630f232501884863fec40f369a4a4` |
| Spacefold | 本地 release probe | 本项目 Rust 核心；该轮 exe SHA-256 随基准原始日志记录 | 桌面交付成品另见 SHA256SUMS |

具体下载 URL、成品帮助与版本输出保存在 `artifacts/bench-tools/`。归档哈希用于核对下载，不能当作包内每个 exe 的哈希。

## 数据与正确性

- 小文件夹具：100 个子目录，每个 1,000 个 128 字节普通文件，100,000 文件、100 目录；根外条目 100,100，总逻辑大小 **12,800,000 字节**。
- 实际目录：本机 MSVC `14.44.35207/include`，361 文件、14 目录；根外条目 375，总逻辑大小 **16,200,957 字节**。该目录在测试期间保持只读。
- 两组数据均已用独立枚举和各工具实际报告/合计核对。每一条有效结果含 `correctnessVerified:true`、实参、轮次、退出码与期望值；漂移或错误轮次不混入有效成绩。
- 初次使用 node_modules 发现 Vite 临时配置文件造成 512 字节漂移，因此最终实际目录改为稳定的 MSVC headers；旧漂移轮次不作为正式结果。

三轮顺序轮换，不清缓存、不提权、不调整系统安全设置。夹具生成和参考枚举已经预热缓存，准确标签是**自然暖缓存**。它不代表冷盘、网络盘、HDD 或整卷扫描。

## 各模式的工作量

| 模式 | 工作量 | 不能混淆的边界 |
|---|---|---|
| gdu summary | apparent 逻辑合计，不保完整浏览树 | 默认硬链接去重；本组普通文件没有该口径差异 |
| dua aggregate | apparent 逻辑合计，开启逐硬链接计数 | 没有 Spacefold 的完整 SQLite 浏览索引 |
| Spacefold kernel | 同一 Windows 元数据枚举与逻辑合计 | 这是剖析模式，不是完整应用扫描成绩 |
| gdu full tree | 完整树与 JSON 写出 | 其输出字段、格式与 SQLite 查询索引不同 |
| gdu sqlite | `--db` 保留完整 SQLite 树 | 独立重读全部节点、根字节和完整性；默认耐久配置与 Spacefold 不同 |
| WinDirStat full tree | 普通权限 `/SaveTo` 完整树报告 | 不启用提权 MFT 加速；保存过滤设置必须固定 |
| Spacefold sqlite | 元数据、完整节点与索引持久化、目录合计、首批可浏览结果 | 默认延后单文件分配/身份细查，选中时测量；逻辑口径一致 |

gdu 5.38.0 默认 SQLite 使用 `synchronous=OFF`、`journal_mode=MEMORY`、`cache_size=-64000`、`temp_store=MEMORY`；Spacefold 保持 WAL/NORMAL、16 MiB 页缓存和 FILE 临时存储。这是各自默认配置的完整树对照，同等耐久配置需要另行测试；没有降低正式程序的耐久设置。配置依据见 [gdu 官方实现](https://github.com/dundee/gdu/blob/v5.38.0/pkg/analyze/sqlite.go)。

参数、口径与官方依据见 [performance-method.md](performance-method.md)。本机 gdu 的默认 `max-cores` 为 16；dua 使用 `--threads 0`。保留各工具默认并发，不把另行固定并发的结果混入本表。

## 实际目录：MSVC headers

外部墙钟包含进程启动至退出，单位为秒。三轮按原始轮次顺序列出；峰值为 Windows 记录的该工具父进程峰值工作集，取三轮最大，单位 MiB。

| 模式 | 第 1 / 2 / 3 轮（秒） | 中位数（秒） | 最大峰值工作集（MiB） |
|---|---|---:|---:|
| dua aggregate | 0.0324 / 0.0331 / 0.0339 | 0.0331 | 11.99 |
| gdu summary | 0.0586 / 0.0454 / 0.0457 | 0.0457 | 14.84 |
| Spacefold kernel | 0.0306 / 0.0317 / 0.0344 | 0.0317 | 6.13 |
| gdu full tree | 0.0603 / 0.0617 / 0.0631 | 0.0617 | 17.33 |
| gdu sqlite | 0.0614 / 0.0627 / 0.0616 | 0.0616 | 19.52 |
| WinDirStat full tree | 0.3102 / 0.3059 / 0.3071 | 0.3071 | 53.43 |
| Spacefold sqlite | 0.1173 / 0.1016 / 0.1010 | 0.1016 | 8.55 |

Spacefold 完整 SQLite 流程中位数 0.1016 秒，低于本组 WinDirStat 的 0.3071 秒，高于 gdu SQLite 的 0.0616 秒及 gdu 完整 JSON 树的 0.0617 秒。内部扫描耗时 70 / 71 / 70 ms；首批已提交可查询节点时间 29 / 30 / 30 ms。kernel 内部 1 / 1 / 2 ms 是枚举剖析指标，不能替代完整流程。

## 100,000 个小文件

| 模式 | 第 1 / 2 / 3 轮（秒） | 中位数（秒） | 最大峰值工作集（MiB） |
|---|---|---:|---:|
| dua aggregate | 0.0664 / 0.0846 / 0.0714 | 0.0714 | 12.14 |
| gdu summary | 0.0614 / 0.0607 / 0.0627 | 0.0614 | 39.84 |
| Spacefold kernel | 0.0973 / 0.0966 / 0.0973 | 0.0973 | 6.14 |
| gdu full tree | 0.1014 / 0.1034 / 0.0990 | 0.1014 | 64.72 |
| gdu sqlite | 0.4066 / 0.3972 / 0.4008 | 0.4008 | 48.08 |
| WinDirStat full tree | 0.6389 / 0.5790 / 0.5982 | 0.5982 | 65.45 |
| Spacefold sqlite | 0.8236 / 0.8314 / 0.8316 | 0.8314 | 27.64 |

本组 Spacefold 完整 SQLite 扫描中位数为 0.8314 秒，是 WinDirStat 的 1.39 倍、gdu SQLite 的 2.07 倍、gdu 完整 JSON 树的 8.20 倍。kernel 中位数 0.0973 秒，也高于 dua/gdu 单纯合计模式。因此**尚未达到所有开源工具的小文件扫描速度**，完整持久化流程的差距仍在。

完整 SQLite 内部耗时 792 / 786 / 792 ms，首批已提交可查询节点为 127 / 123 / 119 ms。默认流程保留完整节点、目录聚合与查询索引，扫描中即可分页浏览。终端 `firstOutputSeconds` 是 probe 最终摘要第一次写 stdout 的时间，不是 UI 首批结果时间；WinDirStat 的非空报告文件也不等同于完整报告写完。

此前有效 [36 次结果](../artifacts/benchmarks-release-final/results.json)及其[构建环境](../artifacts/benchmarks-release-final/environment.json)保留作历史。旧轮 Spacefold 小文件中位数为 1.1317 秒，本轮为 0.8314 秒，降低约 26.5%；后者才是最终构建成绩，历史工具结果不混入本轮对照。

## 测量边界与应用体验

工作集采样周期为 10 ms；原始日志同时保留父进程峰值和采样进程树峰值，后者是采样下界。短进程可能来不及取得进程树样本，采样值 0 表示**未捕获**，不能解释为零内存。上表使用系统记录的父进程峰值，避免把漏采解释为内存优势。

Rust probe 的内存不含 Tauri/WebView2。完整桌面应用另测 exe 与 WebView2 进程树的峰值和交互反馈；不能把核心内存列为整个 GUI 的占用。空间图、透镜和虚拟树的查询/渲染数据也单独记录，不用百万条合成数据库速度替代真实磁盘扫描速度。

实际 Tauri/WebView2 窗口对 500 文件扫描进行了 600 / 800 / 1000 CSS px 高度测试，树表完整可见行 11 / 16 / 22，实际挂载 21 / 26 / 32 行，状态栏贴底、详情独立滚动；搜索可直接定位第 420 个文件。证据为 `artifacts/native-height/metrics.json`、`artifacts/native-real-ui/results.json` 和对应原生截图。这证明已接通窗口自适应与按页查询，并不是百万文件真实磁盘扫描性能证明。

百万条合成数据库另测查询与导出，不能代替真实磁盘扫描：

- [最终分页原始结果](../artifacts/performance/million-pages-final.json)：1,048,576 个同级条目，首页 200 行 4 ms、尾页 200 行 3 ms、文件过滤页 3 ms、无目录匹配页 2 ms，计数精确。深层 32 级祖先及 32 行稳定查询为 3 ms。
- 首次打开旧深层库包含新版索引迁移，外部启动至退出 **9.792 秒**、父进程峰值约 41.27 MiB；迁移后重跑外部墙钟为 0.122 秒。冷迁移成本保留，不能只引用稳定查询毫秒数。
- [CSV](../artifacts/performance/million-csv-release-final.json) / [JSON](../artifacts/performance/million-json-release-final.json) / [XLSX](../artifacts/performance/million-xlsx-release-final.json) 各导出 1,048,576 条的外部墙钟分别为 11.707 / 15.177 / 15.254 秒，100 ms 采样 Rust 进程工作集峰值约 27.50 MiB；采样峰值是下界，不含 WebView2。[独立 CSV/JSON 流式重读](../artifacts/million-reports-final/large-report-parse.json)核对记录数及精确逻辑总量 `10485760000000000`；[独立 XLSX OOXML 重读](../artifacts/million-reports-final/million.xlsx.verification.json)核对 1,048,575 + 1 条分表。

CPU/I/O 等未在这份外部结果结构记录的指标不作数值结论。以后改变枚举或落库路径，需按相同脚本复测并保留原始日志。
