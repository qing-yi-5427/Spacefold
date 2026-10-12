# 验收矩阵

本表以原始两份需求文档为准。只有附有本机证据的项目标记通过；待测与平台边界分别记录。所有写操作夹具均为本轮新建，用户原始文件和回收站不清空。

| 编号 | 可执行检查 | 状态 | 证据 |
|---|---|---|---|
| F01 | 原生目录选择、输入路径、来源/子目录重扫 | 通过 | native-dialogs/results.json；native-real-ui/results.json；手工路径与原生选择的实际扫描 |
| F02 | 增量进度、错误数、取消与部分结果 | 通过（本地I/O） | cancellation/results.json：4001条部分结果、取消47ms；native-risks权限错误/恢复 |
| F03 | 逻辑/已分配字节、元数据、未知区别零 | 通过 | backend/results.json、report-parse.json：2GiB稀疏/压缩FILE_STANDARD_INFO、硬链接2、未知与零 |
| F04 | 同一扫描三视图、scope/primary/multi/sort守恒 | 通过 | native-ui/results.json；native-real-ui/results.json；core.test.ts |
| F05 | 展开、同级排序、多选/范围、键盘、右侧动作 | 通过 | native-ui/results.json：方向键/Space/Shift/往返；native-real-ui有界分页 |
| F06 | 搜索名称/路径、类型/最小大小、定位和退出恢复 | 通过 | native-real-ui/results.json：sample-0420远页定位、退出恢复；backend名称/路径/类型/阈值 |
| F07 | 关联打开、资源管理器定位、剪贴板、属性 | 通过 | native-shell/results.json实际Explorer选中、属性窗、Notepads关联；backend剪贴板回读 |
| F08 | copy/move/rename/trash、冲突、竞争、部分结果与刷新 | 最终安全复证中 | native-risks 7/7、operation-cancellation 200项取消；尾点Shell别名修复待最终exe复证 |
| F09 | 四格式独立解析、三范围、递归、字段与精度 | 通过 | backend/report-parse.json四格式27行；million-reports/large-report-parse.json各1048576；million.xlsx.verification.json拆表 |
| F10 | 完成/取消/中断记录恢复及过期提示 | 最后一致性复证中 | native-restart/results.json真实进程重启恢复3状态；历史导出stale持久化修复待复证 |
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
| U14 | 长路径、空选、叶文件、根动作 | 最终边界复证中 | backend真实长路径、零/空目录、异常UTF16复制；待最终root/空选动作回归 |
| U15 | reduced motion/连续切换无锁 | 通过 | native-ui reduced-motion/快速切换无锁 |
| M5 | 百万元数据、真实盘扫描、Windows程序及安装包 | 发布包验证中 | 百万wide/deep页与三writers重读已过；benchmarks-release-final36次；最终exe/NSIS待本机安装 |

## 独立风险检查

必须核验父路径 junction 替换、来源身份替换、目标冲突、回收站无永久删除回退、取消导出保留原目标、固定 revision、分页预算、图形汇总守恒及应用自己的数据目录排除。真实云占位、断开网络/卷、跨卷取消与缺失 WebView2 环境如未实际建立，明确为未验证。

## 用户新增的桌面验收

移除系统标题栏，保留自绘最小化/最大化/关闭、空白与品牌拖动、双击最大化、边缘缩放。真实Windows窗口7项通过，证据`artifacts/native-window/results.json`。全部Tauri窗口固定填满可用高度，内部列表与详情各自滚动；真实500文件在600/800/1000 CSS像素高度显示11/16/22行，状态栏贴底，行单元格铺满主区。证据`artifacts/native-height/metrics.json`与相邻窗口截图。这两项是用户对最初附录的明确修改，纯浏览器参考页面仍保留原型自然高度。

## 已实际建立的边界

真实Unicode、超过260字符路径、硬链接、稀疏/压缩、junction循环、ACL拒绝后修复、UNC localhost管理共享与跨G→C卷目录复制/移动已验证。异常UTF16名称的JSON原始编码及Shell复制已验证。回收站测试只处理本轮新建夹具，不清空回收站。百万库为合成元数据，记录构建与查询耗时，不冒充百万真实文件扫描。真实小文件性能夹具另有十万文件与逐工具计数/逻辑字节一致证明。

实际云占位、断开网络/卷、HDD、备用数据流、用户配额受限账户和缺失WebView2新系统未建立实机条件，未标为验证通过；程序提供明确覆盖说明与错误/未知状态。
