# 验收矩阵

本表以原始两份需求文档为准。只有附有本机证据的项目标记通过；待测与平台边界分别记录。所有写操作夹具均为本轮新建，用户原始文件和回收站不清空。

下表及原成品证据属于 **0.1.0 历史验收**；0.1.1 性能修复的源码、计时边界、复验与交付哈希在本文末尾另列。旧图和旧性能数据保留原构建归属。

| 编号 | 可执行检查 | 状态 | 证据 |
|---|---|---|---|
| F01 | 原生目录选择、输入路径、来源/子目录重扫 | 通过 | native-dialogs/results.json；native-real-ui/results.json；手工路径与原生选择的实际扫描 |
| F02 | 增量进度、错误数、取消与部分结果 | 通过（本地I/O） | cancellation/results.json：4001条部分结果、取消47ms；native-risks权限错误/恢复 |
| F03 | 逻辑/已分配字节、元数据、未知区别零 | 通过 | backend/results.json、report-parse.json：2GiB稀疏/压缩FILE_STANDARD_INFO、硬链接2、未知与零 |
| F04 | 同一扫描三视图、scope/primary/multi/sort守恒 | 通过 | native-ui/results.json；native-real-ui/results.json；core.test.ts |
| F05 | 展开、同级排序、多选/范围、键盘、右侧动作 | 通过 | native-ui/results.json：方向键/Space/Shift/往返；native-real-ui有界分页 |
| F06 | 搜索名称/路径、类型/最小大小、定位和退出恢复 | 通过 | native-real-ui/results.json：sample-0420远页定位、退出恢复；backend名称/路径/类型/阈值 |
| F07 | 关联打开、资源管理器定位、剪贴板、属性 | 通过 | native-shell/results.json实际Explorer选中、属性窗、Notepads关联；backend剪贴板回读 |
| F08 | copy/move/rename/trash、冲突、竞争、部分结果与刷新 | 通过（已建立本机条件） | 最终native-risks 7/7：跨卷、UNC拒回收站、ACL修复局部刷新/类型/祖先计数、AppData祖先copy拒绝；core真实回收站和四动作自保护；尾点与取消实证 |
| F09 | 四格式独立解析、三范围、递归、字段与精度 | 通过 | 最终成品四格式27行独立解析；百万CSV/JSON精确1048576行与XLSX拆表；临时文件被移走并同名替换时EXPORT_TEMP_CHANGED、双sentinel保留、只清自身对象 |
| F10 | 完成/取消/中断记录恢复及过期提示 | 通过 | native-restart/results.json：生产成品真实进程重启恢复3状态；get_scan和历史JSON导出metadata.stale一致 |
| F11 | 五色、系统主题、GB/GiB、默认视图与重启 | 通过 | native-restart真实teal/dark/binary/lens/xlsx重启；native-real-ui系统主题/单位；native-ui五色 |
| U01 | 1024浅色紫色默认树14行、右220 | 通过（用户修改已记） | reference与native fixture对照、native-ui/results.json；真正桌面全高+自绘控件按新增要求 |
| U02 | 736右204，工具栏与文字无溢出 | 通过 | native响应式截图和metrics：736右204，无横滚 |
| U03 | 640常驻 / 639抽屉边界 | 通过 | native响应式metrics：640常驻右204、639抽屉 |
| U04 | 320三视图与抽屉、无文档横滚 | 通过 | native三视图320截图、native-ui 13/13抽屉焦点回归 |
| U05 | Checkpoints+Games=386GB，清除保留scope | 通过 | native-ui：Checkpoints+Games386GB、清除保留scope |
| U06 | 父子选择、汇总与操作/导出同一规范化集合 | 通过 | native-ui父子归一；backend真实Data与Database/操作/导出复核 |
| U07 | 深层选择三视图往返与树多选恢复 | 通过 | native-ui三视图与多选往返 |
| U08 | 地图面积、下钻返回与叶文件 | 通过 | native-ui下钻/返回；native-real-ui聚合余项不能成为操作目标；面积守恒unit |
| U09 | 透镜hover/selection/scope独立与双击 | 通过 | native-ui hover不改变operation target、双击进入 |
| U10 | 五色全视图与类别/危险色保持 | 通过 | native-ui五色与类别/危险色保持 |
| U11 | 深色/高对比、几何不变与可读性 | 通过（渲染层） | native深色截图与forced-colors模拟；未改变用户OS高对比设置 |
| U12 | 四格式三范围，完整33/根直接6条 | 通过 | native-ui四格式三范围33/6/9/2；后端真实四格式和选取归一另证 |
| U13 | 真键盘、菜单/弹层焦点、Escape/Tab | 通过 | native-ui13/13：Escape/Tab、3抽屉关闭后恢复opener；曾发现inert焦点缺陷已修 |
| U14 | 长路径、空选、叶文件、根动作 | 通过 | 成品native-boundary-ui 3/3根copy计划/空选/rename恢复；backend真实长路径、零/空目录、异常UTF16复制 |
| U15 | reduced motion/连续切换无锁 | 通过 | native-ui reduced-motion/快速切换无锁 |
| M5 | 百万元数据、真实盘扫描、Windows程序及安装包 | 0.1.0历史功能/规模/交付通过 | 百万wide/deep首末页2–4ms与三writers重读；旧42轮对照显示完整小文件扫描慢于竞品；旧portable和隔离NSIS版实际运行。0.1.1最终性能与成品复验见末尾章节 |

## 独立风险检查

必须核验父路径 junction 替换、来源身份替换、目标冲突、回收站无永久删除回退、取消导出保留原目标、固定 revision、分页预算、图形汇总守恒及应用自己的数据目录排除。真实云占位、断开网络/卷、跨卷取消与缺失 WebView2 环境如未实际建立，明确为未验证。

## 用户新增的桌面验收

移除系统标题栏，保留自绘最小化/最大化/关闭、空白与品牌拖动、双击最大化、边缘缩放。真实Windows窗口7项通过，证据`artifacts/native-window/results.json`。全部Tauri窗口固定填满可用高度，内部列表与详情各自滚动；真实500文件在600/800/1000 CSS像素高度显示11/16/22行，状态栏贴底，行单元格铺满主区。证据`artifacts/native-height/metrics.json`与相邻窗口截图。这两项是用户对最初附录的明确修改，纯浏览器参考页面仍保留原型自然高度。

## 已实际建立的边界

真实Unicode、超过260字符路径、硬链接、稀疏/压缩、junction循环、ACL拒绝后修复、UNC localhost管理共享与跨G→C卷目录复制/移动已验证。异常UTF16名称的JSON原始编码及Shell复制已验证。回收站测试只处理本轮新建夹具，不清空回收站。百万库为合成元数据，记录构建与查询耗时，不冒充百万真实文件扫描。真实小文件性能夹具另有十万文件与逐工具计数/逻辑字节一致证明。

实际云占位、断开网络/卷、HDD、备用数据流、用户配额受限账户和缺失WebView2新系统未建立实机条件，未标为验证通过；程序提供明确覆盖说明与错误/未知状态。

## 0.1.0成品与复证归属

生产业务源码提交 `d052183`，后端冻结快照SHA-256 `051900e5de27ea8bbb56795ef9c7f82c316182e23194352a3f01aaf140cf2ef5`。前端27项测试、后端23项核心回归与3项单元测试、启用custom-protocol的严格Clippy和格式检查通过。CI配置已本地解析，远程CI未运行。

0.1.0原成品已完整归档至 `artifacts/perf-round2/rollback-ecff9ca/`：portable SHA-256 `15b5e141f98fa9803bcdedf63cd160dc07b8fa1e8f5aed497b6720bb2ab9c85d`；安装包SHA-256 `9606ddab03b936b43c14ec65b0a6a5ff2723bf908bd6e19322752f210b66414a`。原生UI7/7、边界3/3、窗口控制7/7、600/800/1000高度测试均归属该0.1.0 portable，汇总为`artifacts/final/frontend-acceptance.json`，正常截图为`artifacts/final/portable-real-window.png`。同名 `dist-release` 文件由后续版本更新，不应把本段哈希当作当前文件。

隔离安装路径为`artifacts/install-test/Spacefold`，安装器exit0。安装后的exe SHA-256为`b1c6f3c9ff07ef133288fe8341eb3b4bdc27d41b870a219d2c07ddd8b0df48bb`，与portable严格仅Tauri安装类型标记`UNK`→`NSS`三字节不同，其他内容一致。安装版在无1420开发服务器条件下从`http://tauri.localhost/`加载嵌入界面、经真实UI完成500文件扫描并正常关闭，证据`artifacts/install-test/installation.json`、`runtime.json`、`installed-webview.png`。release清单含Common Controls 6、longPathAware与asInvoker。

原型13项和21张响应式对照采用较早生产exe`56ceacf4…`，其前端资产与最终程序完全相同：`index-DUGABKED.js` SHA-256 `12fb935fe0cb1d46aa6a72ce6f2b77823ee923b73935e429b4b84702beb586d9`。保留真实构建归属，不把旧exe图混称新图。性能基准与GUI工作集也独立标明所测构建，详见`performance.md`。

完整大文件、日志、截图及竞品下载在本机`artifacts/`保留并由Git忽略；源码包含重现脚本和本文摘要，克隆源码不会自动带入本机证据。版本化交付、真实哈希与构建/验收提交分别保存在`dist-release/BUILDINFO.json`和`SHA256SUMS.txt`。

## 0.1.1扫描、数据库与性能复验

扫描业务冻结为 `a61e19e6db35a6721d051c89db6e3ba487474561`。正式正常 release probe SHA-256 为 `c0bb7f00f6e2d8f5551ec8e39f7a0cc8e63217fa6b5e92c6e5dfde965301ef84`，保存于 `artifacts/perf-round2/probes/spacefold-probe-final-a61e19e6.exe`。正常 Tauri 打包会重编 probe；不同指纹单独记录，不能替代该基准构建。前端源码与资产未变，0.1.0响应式证据保留其原始版本归属。

独立格式检查、严格 Clippy、全部 feature/target 的39项 Rust 测试通过（11 lib、27 core、1 probe）；另一次默认并行27 core、真实回收站回归5次均通过，没有增加吞错、泛重试或降低测试并发。日志为 `artifacts/perf-round2/final-a61e19e6-independent-tests/`。已修并有确定性回归的边界包括深度上限保留普通兄弟、reader空输出/派发取消、背压退出、双reader panic唤醒、关闭SQLite后再发终态、CLI失败优先、晚取消不得覆盖已完成状态。独立源码审查见 `artifacts/perf-round2/scanner-review-final.md`。

同机普通权限、自然暖缓存、每组固定5轮交替，没有删除慢轮或混合不同时段。用户最终将0.0x秒差接受为基本持平；严格中位数不高于 WinDirStat 的原门槛仍未达到。游戏关闭后的组也保留实际其他程序负载，不能称完全空闲。

| 游戏关闭后的样本 | SF进程退出中位秒 | WDS进程退出中位秒 | 秒差 | SF含完成核验中位秒 | WDS含完成核验中位秒 | 秒差 |
|---|---:|---:|---:|---:|---:|---:|
| 100k小文件 | 0.5795 | 0.5260 | +0.0535 | 0.5864 | 0.5262 | +0.0602 |
| 宽目录 | 0.4563 | 0.3916 | +0.0647 | 0.4573 | 0.3917 | +0.0656 |
| 既有4KiB历史库迁移后稳态 | 0.5296 | 0.5262 | +0.0034 | 0.5365 | 0.5264 | +0.0101 |

上表按用户最新容差基本持平，不声称每组快于竞品。全组主库 immutable 完成审计、退出WAL为0、原始路径/字节/全部目录聚合/索引/历史完整性均通过。`processExitElapsedSeconds` 包括应用启动、完整扫描、SQLite最后连接关闭和退出；`gateElapsedSeconds` 另包括外部最小完成审计，对WDS也包括报告完成确认。外部核验耗时不是应用自身扫描耗时，完整树验证均在计时外。raw 为 `final-a61e19e6-idle-wide-primary/` 与 `final-a61e19e6-idle-legacy-steady/`，均在 `artifacts/perf-round2/`；有游戏负载的最终三组也完整保留，gate秒差分别+0.0297/+0.0579/+0.0731。背景进程全量CPU采样才是依据，窄游戏名称匹配为空不能证明无游戏。详见性能文档和 `scanner-wall-observer-breakdown.json`。

旧库首次升级、真实MSVC头文件目录、深目录各做当前构建一次正确性确认，见 `final-a61e19e6-scan-correctness/results.json`；首次DROP重复索引包含在该次进程退出观察值，旧历史全部字段未变。单次确认不冒充这两个补充形状的正式5轮比较。长1280字符前缀下8000普通文件扫描实际触发writer的4MiB owned-capacity阈值，完整计数/字节/全部祖先聚合通过，见 `final-a61e19e6-long-budget/results.json`。该阈值允许一条有界行越过，不是全程序RAM上限；两reader共享8MiB预算也不代表程序总内存。

百万合成同级库独立API分页/定位通过，见 `final-a61e19e6-million-pages/results.json`；257条混合大小、ASCII大小写、Unicode和NULL时间的五种排序全遍历通过，见 `final-a61e19e6-mixed-pages-v2/results.json`。各类首/中/末页、独立期望ID与原始UTF16路径、旧游标拒绝、索引计划、完整历史字段均核验。大小/名称普通分页约3.2–4.1ms，但 `node-page` 首/中定位仍有约87–968ms的线性前驱成本；`modified_desc` 实际页约1.93–1.97秒且不支持定位（明确 `INVALID_SORT`）。这些是本轮未优化边界，不能套用旧版“所有分页2–4ms”描述。

## 0.1.1最终程序、便携依赖与安装版

成品构建源码为 `2e47cfc28cfae895c61015bf05fe89186927e872`：包含便携启动模块 `6b0bf695` 和最后的报告目标名修复，扫描业务仍为上述 `a61e19e6`。最终完整44项后端测试（12 lib、4 startup、1 probe、27 core）、严格Clippy和格式检查通过，真实日志为 `artifacts/perf-round2/export-target-full-tests.log`、`export-target-clippy.log`。独立复证按影响范围组合：扫描39项、启动4项、新增HANDLE提交1项，最后另复跑4项export保护核心测试，见 `final-0.1.1-portable-startup-tests/` 和 `final-0.1.1-export-fixed-tests/`；不冒称它们是一次新的44项独立调用。

必要真实成品回归发现并修复报告提交的P1：同一个86 UTF16单位的 `report.xlsx` 请求和返回路径都正确，但旧Win32重命名缓冲区恰好对齐时缺少终止NUL，实际文件名附加堆中的尾单位。旧错误文件、两次失败请求/返回和旧C91/A29/D484包均保留，见 `final-0.1.1-export-target-diagnostic.json`、`pre-export-fix-package/`。修复仅显式追加UTF16 NUL、包含其缓冲区容量，`FileNameLength`仍不含NUL，继续提交原已打开HANDLE。独立unit实际覆盖64个长度/中文emoji/扩展名/已有或缺失目标组合；最终release对原精确失败路径实际复证正确落盘，见 `final-0.1.1-export-target-fixed.json`。没有降低身份核验、父链保护或取消语义。

最终便携ZIP完整携带微软官方Fixed WebView2 `155.0.4283.45` x64，CAB SHA-256为 `ec533bf1b7354d288703cf821a2d640d0130108224f93e330bb9c09faf3f8010`。CAB、`msedgewebview2.exe`、`msedge.dll`的微软Authenticode签名均有效，原包256个文件全部保留（705,527,045字节），包含组件许可与credits入口，包根附项目/第三方许可和微软原许可HTML。最终ZIP新解压后的256份文件逐项与官方CAB SHA相等，262份内容校验清单通过；证据为 `webview2-fixed-155.0.4283.45/`、`final-0.1.1-export-fixed-extraction.json`。

Windows 11真实运行中，未设置显式Runtime环境变量，从不同工作目录启动解压后的exe，实际所有WebView2子进程均位于该exe同级 `WebView2Runtime/msedgewebview2.exe`，版本155，与系统154分开；没有卸载系统Runtime或更改用户设置来制造“缺依赖”环境。证据 `final-0.1.1-final-runtime-process.json`。安装目录没有包内Runtime时，安装版正常使用系统154.0.4258.62，已实际复证。Windows 10专用的微软两SID读取执行ACL准备仅作用于包内普通Runtime树、不提升权限、遇reparse拒绝，确定性启动回归通过，但Windows 10实机及全新缺系统Runtime安装引导未建立，不能标为实机通过。默认记录仍保存在用户AppData，便携仅指程序和Runtime可解压运行，不表示数据写入exe旁。Fixed Runtime不自动更新，后续发布需维护安全更新。

真实功能证据均在 `artifacts/perf-round2/`：

| 当前必要项目 | 结果与归属 | 证据 |
|---|---|---|
| 500文件原生UI、搜索/分页/补测分配 | 7/7，页面错误0；C91成品，最终仅报告helper变化，映射保留 | `final-0.1.1-native-ui/results.json` |
| 1600次实际RPC读取、32客户端lane | 错误0、进度/revision不退、观察已提交部分结果，最终100101节点/12800000B；C91扫描源码未变 | `final-0.1.1-concurrent-reads/results.json` |
| 100k文件真实UI扫描 | 首批可见64.44ms、最大frame gap12.5ms、无long task、仅挂载34行；这是UI观察，非竞品基准 | `final-0.1.1-native-scan/results.json` |
| 实际取消与局部结果 | 50k新建文件夹具，取消59.27ms；保留9778文件、根计数和3B/文件聚合精确；C91扫描源码未变 | `final-0.1.1-native-cancellation/results.json` |
| 最终四报告/特殊路径/身份保护 | 11/11，CSV/XLSX/JSON/HTML各27行独立重读；最终A676成品 | `final-0.1.1-native-backend-v2/results.json`、`report-independent-parse.json` |
| 四格式取消保原目标 | 4/4，约17.9–20.5ms，原sentinel保持、无临时文件残留 | `final-0.1.1-export-cancel/results.json` |
| 临时输出被移走并同名替换 | 导出failed，原目标与替换对象两sentinel保持，只清理自身打开对象 | `final-0.1.1-export-temp-identity/<UUID>/results.json` |
| 真实进程中断后重启 | completed/cancelled/interrupted三状态、设置与历史导出stale一致；只终止核验过身份的QA进程 | `final-0.1.1-native-restart/results.json` |
| 最终公开三视图 | 真实新建10文件、6目录含root、162533376B演示夹具；最终A676/包内Fixed155，无fixture模式、图片未加工 | `public-demo/screenshots/provenance.json`、`docs/screenshots/` |
| 隔离安装与正常关闭 | NSIS exit0；嵌入UI从tauri.localhost扫描501节点/128000B/0错误、正常关窗 | `final-0.1.1-install-test/installation.json`、`runtime.json`、`final-0.1.1-installed-smoke/results.json` |

最终文件与哈希如下，不再以修复前构建作为交付：

| 文件 | SHA-256 |
|---|---|
| `dist-release/Spacefold.exe`（单exe需要相邻或系统Runtime） | `a676be39c6552a86b2d2efbce6fe398c00367d38f4bf60c478dd2bc0785a94d4` |
| `dist-release/Spacefold_0.1.1_x64-portable.zip`（完整独立程序依赖） | `becc786e4d18fae10026ef36cb40f691fd7d80c354c93602eee99587cde75e40` |
| `dist-release/Spacefold_0.1.1_x64-setup.exe` | `3ae03d6cdc3350ec6423dd2d8dad9d6f996cb51767b277e1c53f5648951eaa43` |

隔离安装exe SHA-256为 `70571d36e882eef6e4fdef0ae1dbfbb43e5621ba5bb3caeae85b1cd224c4cacc`，与portable严格仅Tauri `UNK`→`NSS`三字节标记不同。构建日志 `final-0.1.1-export-fixed-package.log`，依赖版本、锁文件、SQLite实际选项、前端资产与冻结基准probe映射见 `dist-release/BUILDINFO.json`。ZIP内BUILDINFO/CONTENTS保持构建时内容；构建后的验收与Release提交映射另在外部交付BUILDINFO与验收索引记录，纯文档/测试提交不重复构建。原0.1.0回退与修复前包继续保留，未发布为最终版。代码与公开三图可随仓库获取，本机大体积原始证据仍由Git忽略。
