# 间隙 Spacefold：UI 设计与复刻规范

版本：1.0 · 右侧详情定稿版  
日期：2026-10-11  
配套文档：`spacefold-features-tech.md`（功能范围与技术实现）

## 1. 开发 agent 先读这一节

这是一个桌面文件空间分析工具的完整 UI 交接。目标是让用户看清哪个目录、哪个文件占了多少空间，沿目录继续查找，在同一个工作区完成文件操作，并导出扫描数据。产品暂名“间隙”，英文工程名 Spacefold；名称不影响实现。

**视觉与交互基准是本文附录 A 内嵌的完整 HTML / CSS / JavaScript 原型。** 不需要寻找之前的聊天、截图或临时文件。先把附录原样提取并运行，固定基准，再拆成 React 组件。不要凭文字描述重新设计一套“类似”的界面。附录包含全部三视图、模拟数据、弹层、强调色设置及交互脚本；它是视觉参考实现，真实扫描和文件操作按配套技术文档接入。

本次已经确定的产品约束：

- 同一应用保留“空间地图 / 目录树 / 层级透镜”三种视图，默认打开目录树。
- 目录工作台采用真正可展开、可折叠的树表，保留父子连接线和大小列。
- 项目详情、单项操作、多项操作都在右侧。桌面窗口不再有底部详情板或底部批量操作栏。
- 借鉴 Mac 工具的排版、留白、层次和反馈方式；主导航是顶部工具栏与分段切换，不设置常驻 Finder 式左栏，也不绘制红黄绿窗口按钮。
- 默认强调色是紫色，设置中提供紫、蓝、青、橙、玫红五种颜色。切换全局生效；目录类别色、危险操作色保持各自语义。
- 这是高密度工具界面：让文件内容成为主角，工具栏和详情区保持克制，不增加大标题、宣传语、统计卡片墙或装饰背景。

“1:1”以**同一测试环境、同一容器宽度、同一字体、同一数据和状态**下的参考源码为准。Windows 与 macOS 的系统字体、原生复选框和 WebView 字形栅格化可能不同；首次验收须固定操作系统、WebView 版本、缩放为 100%、设备像素比与字体。不能通过要求不同系统截图的每个像素完全相等来验收。

## 2. 页面构成与视觉层级

整个应用是一个连续的工作区。最上方只有一条应用工具栏；工具栏下方是当前视图。主内容在左侧占据剩余宽度，详情在右侧形成稳定的阅读终点。表格、地图和透镜共用边框、字体、强调色与文件操作弹层。

| 区域 | 承担的任务 | 视觉处理 |
|---|---|---|
| 顶部工具栏 | 应用名、扫描位置、三视图切换、设置、扫描、文件菜单、导出 | 浅灰白底，底部 1 px 分隔线，工具以小图标为主 |
| 当前路径条 | 返回上级、面包屑、当前范围；小窗口打开详情 | 最小 44 px；面包屑允许换行，单个长项目按视图省略 |
| 主分析区 | 树表 / 按面积绘制的地图 / 两层环形层级图 | 白色或深色实心画布，容量与文件名优先 |
| 右侧详情区 | 当前选中项、合计、下级项目、文件操作 | 桌面固定 220 px，左侧 1 px 分隔线；没有独立悬浮大卡片 |
| 状态行 | 扫描状态、已选数量等短信息 | 只做薄状态行，不承担文件操作 |
| 浮层 | 位置菜单、外观、文件菜单、操作对话框 | 从触发器附近出现，有边框与适量阴影 |

三个视图不是三款不同风格的产品。切换时传递位置与选择，让用户在同一批文件上切换理解方式。若图形需要把深层所选文件置于其直接父目录下展示，可以调整显示范围；返回树的恢复规则见第 12 节。

## 3. 几何基准

下表均为 CSS 像素；最终级联值以附录为准。本文断点与测试宽度指 `#spacefold-mac` 的容器查询内容宽度，也就是没有水平内边距时的 `clientWidth`，不含左右两条外框边线。原型宽度跟随宿主，主要检查内容宽度为 1024、736、640、639 和 320 px。桌面应用正式实现应填满内容窗口，保持这些组件尺寸，并把窗口多出的高度分配给工作区。

| 项目 | 定稿值 / 规则 |
|---|---|
| 应用外框 | 1 px 边框，14 px 圆角，`overflow:hidden`，独立 stacking context |
| 外框阴影 | `0 14px 40px`，浅色 `#2723350c`，深色 `#00000024` |
| 顶部工具栏 | 常规最小高度 62 px，内边距 `10px 17px`，常规间距 16 px |
| 应用标志 | 27 × 27 px，8 px 圆角；内部 layers 图标 17 px |
| 位置按钮 | 常规最小宽度 157 px、最小高度 38 px，7 px 圆角 |
| 分段切换器 | 外内边距 3 px、8 px 圆角；选中底片 6 px 圆角 |
| 三个标签按钮 | 正文字号 13 px；常规最小高度 31 px，内边距 `6px 12px` |
| 顶部图标按钮 | 常规 32 px 以上宽、33 px 以上高，7 px 圆角；图标 16 px |
| 路径条 | 最小 44 px；主要文本 12–13 px；面包屑 `flex-wrap:wrap` |
| 右侧栏 | 容器宽度 >800 px 时 220 px；640–800 px 时 204 px |
| 目录树行 | 36 px，正文 14 px；表头 35 px、12 px 字号 |
| 目录树列宽 | 选择 28 px；名称自适应；大小 86 px；占比 122 px；进入 30 px |
| 中等宽度树列 | ≤800 px：大小 82 px，占比 89 px |
| 窄屏树列 | ≤480 px：大小 78 px，选择 27 px；占比和进入列隐藏 |
| 目录树缩进 | 每级 17 px；≤800 px 变为 15 px；≤480 px 为 12 px |
| 常规详情栏横向内边距 | 树 / 地图 16 px；透镜 12 px；中等宽度略减 |
| 详情容量数字 | 树 / 地图 30 px，正常字重、紧凑字距；不是粗重的仪表盘数字 |
| 图内大数字 | 透镜中心常规 39 px；≤790 px 为 34 px；≤360 px 为 30 px，按断点切换 |
| 树的薄状态行 | 最小 31 px，12 px 文本 |
| 地图工作区 | 常规 `.dm-content` 最小 608 px；地图矩形区域最小 520 px |
| 透镜工作区 | `.dl-shell` 最小 608 px，路径条下 `.dl-workspace` 最小 564 px |
| <640 px 的工作区 | 树根、地图内容和透镜 shell 最小 650 px；透镜 workspace 最小 606 px |
| 环图最大宽度 | 常规 440 px；窄屏 430 px；保持正方形比例 |

**高度规则：**默认树仅展开根、AI Models 与 Games，共 14 行。详情不额外叠加到树的下方；透镜目录项默认保持紧凑，不能自动展开所有子目录。原型总高度由工具栏、行高和工作区最小高度自然形成，不使用固定外层高度裁掉内容。按 CSS 推导，桌面默认树整体约 696 px，地图 / 透镜约 672 px；这是几何预期，尚未经浏览器像素测量。正式桌面应用使用窗口内滚动：树表滚动、右栏内部滚动，顶部工具栏保持可用。不得为了容纳百万行而把整个窗口无限增高。

## 4. 颜色、字体与图标

### 4.1 中性色

| Token | 浅色模式 | 深色模式 | 用途 |
|---|---|---|---|
| `--sf-ink` | `#272830` | `#efedf4` | 主文字 |
| `--sf-muted` | `#71737e` | `#a7a5b2` | 次要文字、辅助图标 |
| `--sf-panel` | `#ffffff` | `#25252b` | 主面板、浮层不透明底 |
| `--sf-line` | `#e9e9ef` | `#3b3944` | 分隔线 |
| `--sf-soft` | `#f3f3f7` | `#302e38` | 轻背景、hover |
| `--sf-chrome` | `#fafafb` | `#29282f` | 工具栏 |
| `--sf-danger` | `#ba4c54` | `#eea1a5` | 危险动作文字 |
| 外框边线 | `#dedee7` | `#45424e` | 应用外框 |

不要给所有区域加玻璃模糊。正文所在区域保持实心背景；层级主要由间距、分隔线、字体权重与轻微明度差表达。

局部危险动作沿用模块 token，不要在初次复刻时全部替换成 `--sf-danger`：顶栏危险文字是上表的值；树右栏为浅色 `#c33d39` / 深色 `#ff8984`；地图与透镜为 `#c35453` / `#ed8b88`；对话框危险填色按钮为 `#bc4d55` / `#b94a51`，其文字为白色。这些危险色均不随强调色改变。

### 4.2 五种强调色

`--sf-accent` 用于文字、选中边界与图标；填色主按钮单独使用 `--sf-accent-button`，按钮文字均为白色。

| 名称 / key | 浅色 accent | 深色 accent | 两模式按钮底 |
|---|---|---|---|
| 紫色 / `purple`（默认） | `#7654bb` | `#bf9ff5` | `#7652bb` |
| 蓝色 / `blue` | `#226bce` | `#89b8fa` | `#236ac6` |
| 青色 / `teal` | `#067f87` | `#7bcdd2` | `#087d82` |
| 橙色 / `amber` | `#a45d18` | `#e4b477` | `#a0601c` |
| 玫红 / `rose` | `#ad4f80` | `#e6a2c6` | `#ad4f80` |

所有混色使用 sRGB，另一个颜色均为当前 `--sf-panel`：`accent-soft = 10% accent`，`accent-selected = 15%`，`accent-covered = 4%`，文件夹轮廓 `85%`，文件夹填色 `26%`。树行最终选中背景还有与本地背景的混合；直接保留附录选择器，不以肉眼估色替换。

外观设置面板宽 `min(296px, 100% - 20px)`，五个色彩按钮等宽一行。按钮包含 27 px 色圆、选中 check 和中文名称。不能只有色圆而没有名称；切换立即反馈，无需额外“应用”按钮。正式版保存到本地设置，原型只在当前页面会话内改变状态。

### 4.3 目录类别色

这些颜色区分数据分组，不跟随用户的强调色变化。同一顶级目录在三种视图里使用同一类别色；深层目录继承所属分组。真实数据中由稳定映射生成分类色，不依据当前排序重新分配。

| 演示类别 | 浅色 | 深色 |
|---|---|---|
| AI Models | `#8b7fdb` | `#a499ed` |
| Games | `#59aec2` | `#76c1d2` |
| Media | `#df9689` | `#e7ac9f` |
| Projects | `#ddb077` | `#e5c08f` |
| Downloads | `#a4b879` | `#b5c991` |
| Others | `#a9aab3` | `#a5a6b1` |

### 4.4 字体与图标

字体栈为 `-apple-system, BlinkMacSystemFont, "Segoe UI Variable Text", "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif`。全局 13 px / 1.45；树名 14 px；详情名称在树 / 地图 / 透镜中分别为 15 / 14 / 13 px、500 字重；弹窗标题 18 px、600 字重。辅助标签 11–12 px，不把关键操作文字降到 11 px。容量与占比使用等宽数字特性 `font-variant-numeric:tabular-nums`，不把整段路径改成代码字体。

图标使用 Lucide 的同一套线性图标，常规 14–17 px、描边约 1.6–1.7；文件夹详情图标在树 / 地图 / 透镜中分别为 30 / 28 / 24 px。原型用 `i[data-lucide]`，宿主提供 `window.lucide.createIcons()`。正式 React 实现使用同一 Lucide 图标组件并本地打包，不依赖外部图标 CDN。图标名与各按钮的准确配对见附录；不要用 emoji、系统彩色文件图标或另一套混合图标替代。

## 5. 顶部工具栏与导航

从左到右为：品牌 → 当前扫描位置 → 三视图分段控制 → 设置 → 扫描 → 文件操作菜单 → 导出。容器宽度减小时先收紧间距和省略辅助文字，再把分段控制换到第二行，主数据不因此改成单列长页。

位置按钮显示 `Data · D:`，第二行是磁盘容量短信息。点击打开“本次扫描”浮层：磁盘选项、容量条、根下六个演示目录、选择新位置扫描入口。点击一个目录将它设为当前 scope：目录树清空选择，地图和透镜会选择该范围的首个可展示子项，并同步右侧详情；若没有子项，则选择范围本身。正式空目录状态见第 13 节；这里的模拟项目由真实扫描位置及最近记录替换。

分段控制顺序固定：**空间地图 → 目录树 → 层级透镜**。默认中间项选中。使用 `--sf-panel` 的底片在三项间移动，选中文字使用强调色。切换不重复扫描、不无故跳回根目录；深层选择的图形范围转换按第 12 节执行。

文件菜单包含打开所在位置、复制路径、移动、重命名、属性、移至回收站。菜单根据当前选择数量与项目类型禁用不适用动作。顶层磁盘不可移动、重命名或回收；无选择时文件动作禁用；多选不提供单文件重命名和单项打开。

位置菜单、外观菜单、文件菜单互斥；打开其中一个关闭另外两个。浮层位置根据触发按钮实测定位，距按钮约 9 px，左右边距至少 9 px。点击外部或 Escape 关闭；从键盘关闭时焦点回到触发按钮。浮层被边界裁切时优先调整位置或内部滚动。

## 6. 目录树视图

### 6.1 表格内容

使用语义化 treegrid：选择框、名称、大小、占父级比例、进入目录按钮。类型不单占一列；文件夹与文件用图标及详情说明区别。名称单行省略，完整路径在右栏和可访问名称中可获取。大小右对齐，比例是 4 px 细条加数值。

层级使用 17 px 的逐级缩进和细连接线。展开按钮、名称选择按钮、进入目录按钮是三个不同交互目标：

| 点击目标 | 行为 |
|---|---|
| 左侧展开箭头 | 只展开或折叠此目录，不改变当前 scope |
| 名称按钮单击 | 改变 selection，在右侧展示该项目；不进入目录 |
| 目录名称双击 | 展开 / 折叠该分支，scope 不变；与地图 / 透镜的双击下钻不同 |
| 文件名称双击 | 打开所在位置的操作预演；正式版资源管理器定位 |
| 复选框 | 加入或移出多选集合；支持父子覆盖状态 |
| 行尾进入 / 右侧“进入” | 将该目录变成 scope，更新面包屑 |
| 返回按钮 / 面包屑 | 改变 scope；保持可解释的选择位置 |
| 名称 / 大小表头 | 同级项目排序；不会把不同层级混在一起排序 |
| 清除 | 清空 selection；保留当前 scope 和展开状态 |

默认按大小降序。数字对应技术文档里的逻辑大小；“占父级”的分母是父目录已知大小。扫描中、不完整目录、未知字节的补充状态按第 13 节处理。

默认根范围 `D:\`，初始选择 `D:\AI Models`；展开根、AI Models 与 Games。按当前模拟数据默认可见 14 行。不要把整棵演示树全部展开作为初始状态。

### 6.2 右侧单选详情

顺序为：

1. “项目详情”细标题与“清除”；窄屏多一个关闭按钮。
2. 文件夹 / 文件图标、名称、类型。
3. 大小数字、占当前范围的比例和细进度条。
4. 操作：第一行“打开位置 / 移动”；第二组两列为复制路径、重命名、导出所选、回收站。
5. “下级占用”列表，最多展示前四个真实子项，超过时显示剩余数量与合计；右侧“进入”可进入当前目录。
6. 位置标签与完整路径。

点击下级项目应选中真实节点并展开到可见，不能只替换一段文本。列表条与树表使用相同容量，右侧的小进度条以当前被检查目录为分母。叶文件不虚构下级列表。

### 6.3 多选与空选

多选后右侧变为集合摘要：已选项目数量、覆盖归一后的合计、可执行批量动作和所选项目摘要。移动成为主要动作；复制路径、导出所选、回收站保持可用。单项打开和重命名隐藏且不可聚焦。**没有底部批量工具栏。**

父目录与子文件同时被选中时，选择的逻辑覆盖集合只计父目录一次。子项可以显示“已被上级包含”的轻背景，而不把祖先和子项重复相加。视觉上的勾选状态、右侧合计、文件操作目标、导出范围必须使用同一归一结果。

无选择时右侧提示选择项目，显示无选择状态并禁用依赖选择的操作；不得沿用上一项数据，让用户误以为还在操作它。

## 7. 空间地图视图

空间地图是矩形 treemap，面积对应当前范围里各项的同一大小指标。当前实现为确定性的按权重二分布局；附录算法与顺序是复刻基准。每个大矩形是当前层级的目录或文件；目录内部的小块用于预示下一层结构。

画布左、右边距约 16 / 14 px，右侧详情宽 220 px。目录色使用低饱和类别色；标签与容量有稳定层级。小矩形放不下文字时省略文字，真实文件名仍可通过详情或列表读取，不强行挤成难读的小字。

单击选中矩形，描绘 2 px 的强调色内部边界并更新右栏；双击目录或右侧“进入目录”改变 scope。叶文件可选但不能进入。图形里的预览子块不是没有说明的独立文件操作目标；下钻后才作为当前层节点处理。

右侧自上而下展示标题、图标与名称、完整路径、30 px 容量、占比等短信息、“进入目录”、真实下级项目、底部两列操作。操作在右栏内部靠下排列，不能移到整个地图下方。点右侧子项会进入其父范围并选择该真实子项。

地图必须在视图可见后测量宽高，并在容器尺寸改变时重算。不能把隐藏视图的 0 宽度作为永久布局缓存。正式大数据版的布局与渲染预算见技术文档；不要为了精确面积而给每一个扫描文件创建 DOM 节点。

## 8. 层级透镜视图

透镜用左侧双环 sunburst 表达当前目录与下一级的关系，右侧是紧凑目录列表及选中项操作。没有横跨整个页面的底部选中卡或操作栏。

中心始终表示**当前 scope**：目录名、当前范围合计、返回上级提示。点击中心返回父级；根范围时返回不可用。中心不要随着 hover 变成另一个目录，也不要把“当前范围”和“所选项”混成一个状态。

内环为 scope 的直接子项，外环为它们的下级；角度由同一大小比例计算。hover 环形或右侧行时，两边同时强调相同项目；hover 只影响临时视觉状态，不改变 selection，不改变导出对象。点击选中；双击目录或显式进入按钮下钻。切换 selection 不销毁当前被点击的 SVG 节点，以保证双击可靠。

右侧列表显示当前目录的直接子项。默认只列这些项目，**不展开每个项目的下一层列表**；下钻后列表更新到新的直接子项。列表包含类别色条、名称、短说明、右对齐容量 / 占比与进入箭头。选中背景只覆盖该行主区域，不让整块子目录面板染紫。

右侧列表下面以细线分隔选中项摘要，显示图标、`名称 · 大小`、完整路径，以及两列的打开位置、移动、重命名、复制路径、导出所选、回收站。即使中心和选中项不同，用户也能明确分辨“当前在哪一层”和“操作哪个项目”。

## 9. 共用文件操作与导出 UI

### 9.1 操作对话框

文件操作从右栏或顶部文件菜单进入同一套对话框，不能每个视图写一套参数校验和表单。对话框挂在应用根节点上方；背景轻遮罩，14 px 圆角；标题 18 px / 600，左右内边距 22 px。按钮常规至少 33 px，高度由原生控件与文本自然决定；小窗口对话框内部可滚动。

| 动作 | 核心内容 | 确认后的原型行为 / 正式版要求 |
|---|---|---|
| 选择扫描位置 | 磁盘 / 文件夹选择、路径、扫描入口 | 原型切换模拟范围；正式版系统目录选择器和后台任务 |
| 打开位置 | 选中对象路径 | 原型给反馈；正式版在资源管理器定位 |
| 复制路径 | 单项或多项路径 | 原型演示反馈；正式版写系统剪贴板 |
| 重命名 | 原名称、新名称、父目录 | 原型校验；正式版重新校验身份、冲突与平台规则 |
| 移动 | 所选集合、目标目录、范围合计 | 禁止目标是自身 / 后代；正式版展示逐项结果 |
| 回收站 | 所选项目和合计、明确动作名称 | 正式版只走可验证的回收站路线；失败不改成永久删除 |
| 属性 | 路径、类型、大小等 | 正式版补充逻辑大小、已分配大小、时间、扫描状态 |

错误就地放在相关输入下方，保留用户输入；取消返回原状态。成功提示短小，不弹长说明。真实文件操作运行时允许取消尚未开始的项，显示部分成功及失败原因，不能一律显示“成功”。

### 9.2 导出

顶部“导出”默认导出当前范围；右侧“导出所选”默认所选集合。冻结原型的对话框顺序固定：标题 → 四格式选择 → 范围 → 是否包含下级 → 字段 → 预览 → 提示 → 取消 / 生成导出预览。预览头显示只读的建议文件名、记录数与合计；原型没有独立对象摘要、可编辑文件名或保存路径输入框。正式版主按钮改为“导出”，点击后调用系统保存对话框，确定文件名和位置，再启动后台导出，不把新增字段塞进原型表单。

初始默认 CSV，递归开启，勾选名称 / 路径 / 大小 / 类型；父路径与深度默认不勾选。顶部默认 `current`，三个右栏入口默认 `selected`。

| 控件 | 选项 / 规则 |
|---|---|
| 格式 | CSV、Excel（`.xlsx`）、JSON、HTML；等宽四项分段按钮 |
| 范围 | 全部扫描结果、当前目录、所选项目；无选择时所选不可用 |
| 递归 | 独立复选框“包含子目录”；改变记录数，不改变集合口径 |
| 字段 | 名称、路径、大小、类型、父路径、深度；正式版扩展见技术文档 |
| 预览 | 实际导出 query 的样本、记录数、范围大小；不以可见树行作为全量结果 |
| 确认 | 主按钮使用强调色；导出任务成功后显示真实文件位置 |

原型预览不等于已生成真实 Excel 文件，完整文件写入由正式后端实现。正式版区分“可阅读报告”和“完整扫描记录”：任意裁剪字段的报告不能声称能恢复整棵扫描树。字节精度、CSV 表格兼容文本化、XLSX 行数上限都遵循技术文档，错误要进入本对话框而不是静默丢数据。

## 10. 响应式与滚动契约

响应式依据应用容器宽度，而不是整台显示器宽度。所有布局计算都允许主列 `minmax(0,1fr)`，长路径不能撑破列宽。

| 容器宽度 | 顶部 | 分析区与详情 |
|---|---|---|
| >850 px | 常规单行；≤960 px 可隐藏扫描文字并收紧间距 | 左侧自适应，右 220 px |
| 801–850 px | 紧凑单行；隐藏品牌文字、位置总容量等辅助文字 | 右 220 px |
| 720–800 px | 紧凑单行，三个视图名称保留 | 右 204 px；736 px 不得变为上下布局 |
| 640–719 px | 分段切换器独占第二行 | 主分析区与右 204 px 仍并列 |
| <640 px | 分段切换器第二行 | 主分析区全宽；“详情”按钮从右侧打开抽屉 |
| ≤480 px | 进一步收紧间距 | 树表隐藏占比与进入独立列，保留名称 / 大小；右侧操作仍可访问 |
| ≤420 px | 品牌只保留图标；扫描图标可收进位置菜单 | 不隐藏三视图入口，不隐藏导出入口 |

窄屏详情抽屉宽 `min(304px, 100% - 16px)`，原型固定 `top:44px`、`height:calc(100% - 44px)`，从常规路径条下方开始、右贴边；遮罩覆盖主分析区。面包屑按附录允许换行：地图和透镜的单个长 crumb 尾部省略，树的多个 crumb 可换行，当前没有自动中段折叠算法。正式版若支持更多层级，需要让抽屉顶部跟随路径条实际高度，避免长面包屑被遮盖；这是长路径状态补全，不是原型已有测量逻辑。抽屉有关闭按钮、`role=dialog`、`aria-modal`；后方工具栏和分析区 inert。Escape、关闭按钮、遮罩可以关闭，焦点回到“详情”。抽屉使用自身滚动条，正文超出时不增高整页，也不被应用外框截断。

窗口放宽到 640 px 及以上时退出抽屉状态，恢复常驻右栏。切换视图关闭上一视图抽屉。打开文件操作对话框时先收起抽屉，避免叠加两个可交互弹层。

正式桌面窗口的竖向约束建议：工具栏与路径条不滚动，树表 / 图形工作区填满剩余高度，详情独立 `overflow:auto`。右栏的操作与当前对象始终在同一滚动上下文中，不转移到窗口底部。狭窄只是兼容场景，桌面主验收仍以 1024 px 和 736 px 为准。

## 11. 动效、键盘与可访问性

交互要即时：按下按钮马上改变亮度；不能等待动画结束才接收下一次点击。切换器的底片使用可中断弹簧，原型参数为 stiffness 480、damping 44，步长上限 32 ms，位置 / 速度终止阈值 0.08。这个参数是参考实现细节，不应再额外叠加拉伸、弹跳或大范围缩放。系统启用 reduced motion 时直接定位，不播放过渡。

使用原生 button、input、select，保留可见的浏览器 / 系统焦点样式。图标按钮具备中文可访问名称；不要删除 outline。主表有 `role=treegrid`、行层级和展开状态；复选框具备已选 / 部分包含状态。选中不只靠颜色，还通过复选框、边框、文字或语义属性表达。

| 场景 | 键盘要求 |
|---|---|
| 三视图控制 | 左右箭头切换；Home / End 到两端 |
| 强调色 | 左右箭头、Home / End 切换并反馈名称 |
| 原生动作按钮 | Tab 可达，Enter / Space 执行 |
| 菜单 | 上下箭头移动；Escape 关闭并恢复触发焦点 |
| 对话框 / 右侧抽屉 | Tab 在当前弹层可用控件内循环；Escape 关闭 |
| 树展开 / 进入 | 原型可通过原生按钮完成；生产 treegrid 补齐方向键导航，见技术文档验收 |
| 选择、扫描、导出反馈 | 简短 aria-live，不能每扫描一个文件播报一次 |

没有可用鼠标 hover 的设备也必须能完成选择和下钻；触控 / 粗指针操作目标至少约 44 px。高对比模式增强 muted 与分隔线；深色模式改变完整调色板，不用半透明白面板叠在随机背景上。

## 12. 数据与视图状态契约

### 12.1 演示数据

为了跨版本复刻，使用同一固定 fixture。根 `D:\` 的模拟逻辑合计 742 GB；模拟磁盘总容量 1 TB、可用 258 GB。这两个 742 GB 相等只是演示选择，真实扫描不得强迫它们相等。

| 一级目录 | 大小 | 直接下级 |
|---|---|---|
| AI Models | 286 GB | Checkpoints 168；LLM 78；LoRA 24；VAE 16 |
| Games | 218 GB | SteamLibrary 166；Epic Games 42；Saves 10 |
| Media | 132 GB | Videos 88；Photos 30；Audio 14 |
| Projects | 64 GB | Workspace 39；Build cache 18；Design 7 |
| Downloads | 28 GB | Installers 17；Archives 8；Temporary 3 |
| Others | 14 GB | Backups 10；Misc 4 |

数据共 34 个节点（含根）、33 个后代。Checkpoints 下有 64 / 48 / 32 / 24 GB 的四项；LLM 下有 36 / 28 / 14 GB 三项；LoRA 下有 15 / 9 GB 两项。完整名称、类型、父子关系和路径都在附录。这里有部分“代表聚合目录”的叶节点，不能据此宣称真实扫描包含几百万文件。

### 12.2 状态分离

`scope` 是当前浏览的目录，`selection` 是将被查看或操作的项目，`hover` 是短暂强调。三者必须独立。切换视图传递 scope 与 selection；图形视图使用一个主要选择，树视图可保留多选集合。树里选中深层项目再切图形时，图形可把 scope 调整到该主选择的直接父目录。返回树时，如果主要选择未变，同时恢复原树 scope 和多选；用户在图形中改选其他项目时跟随新的选择与相应范围。

参考原型对外接口如下。正式版可以换内部组件结构，但要提供等价状态行为：

```ts
type ViewKey = 'map' | 'tree' | 'lens';
type ViewState = { scopePath: string; selectedPaths: string[] };

interface ViewAdapter {
  getState(): ViewState;
  setState(state: ViewState): void;
  getActionContext(): {
    name: string; path: string; size: number; type: string;
    scope: string; scopeSize: number;
    rows: unknown[]; selectedRows: unknown[]; count?: number;
  };
}
// window.diskViewAdapters.map / tree / lens
// window.diskConceptData: root, nodes, get, normalizeRows, flattenRows
// window.diskConceptActions: open, close, toast
// window.spacefoldPreview: switchView, getActiveView, getState,
//   setAccent, getAccent, showInspector, closeInspector, getOpenInspector
```

这是**原型接口**，其中 `size:number` 仅表示固定演示 GB。生产接口使用 `scanId + nodeId + revision` 与字节十进制字符串，按技术文档实现；不能把这里的 path 小写归一方法照搬为真实文件身份判断。

已提交的选择或范围变化触发 `disk-view-change`；getter 与 hover 不发事件。禁止通过反复读取 getState 触发重渲染或造成循环同步。生产实现的三视图必须读同一个数据源，不各自硬编码合计；冻结原型为保持模块独立，包含了三份一致的 fixture 定义，不要把这种演示结构照搬成三个生产数据库。

## 13. 从原型补全到真实产品的状态设计

本节是正式产品必须补齐的状态；附录只实现其中的交互演示。新增状态复用上面的区域与样式，不新增导航架构。

| 状态 | 放在哪里 / 怎么显示 | 允许动作 |
|---|---|---|
| 尚未扫描 | 主分析区居中简短说明、“选择位置扫描”按钮；右侧为空提示 | 扫描、外观设置 |
| 扫描中 | 位置处小进度标记；状态行“已扫描 N 项 · 已知大小”；无已知总量时不伪造百分比 | 浏览已提交结果、停止扫描；导出明确的部分版本 |
| 正在停止 | 状态行说明正在结束当前读取；停止按钮禁用重复请求 | 浏览已提交结果 |
| 有错误 / 跳过 | 状态行“完成 · N 项未读取”，可打开问题列表 | 浏览、重新扫描问题范围、导出带完整性标记的数据 |
| 空目录 | 保留路径与右栏，主区一句“此目录为空”；显示 0 项 | 返回、打开位置、允许的文件操作 |
| 未知大小 | 大小显示“— / 未知”，详情解释原因；不填 0 B | 定位、重试或用户允许的操作 |
| 过期记录 | 项目显示“扫描后已变化”，操作前重新核实 | 刷新；通过核验后再执行 |
| 文件操作中 | 共用对话框显示进度与已完成数量；不可重发相同任务 | 取消待执行项 |
| 部分操作失败 | 同一对话框逐项列结果，保留失败项目选择 | 查看原因、重试允许项、关闭 |
| 导出中 / 失败 | 导出对话框显示生成进度；失败保留参数 | 取消、重试、另选位置 |

计量口径在表头 tooltip / 属性中解释：默认大小是逻辑大小，硬链接按路径计数；已分配字节是独立字段，未知显示“未取得”。多选文案使用“选中合计”，不能直接写“预计释放”。磁盘可用空间与本次扫描合计是两个独立信息。

### 13.1 原型尚未绘制的 P0 入口

配套技术文档还要求搜索 / 筛选、复制文件、系统关联打开、完整设置和最近扫描恢复。这些是正式产品补全项，不能把附录中的模拟控件当作已实现。按下面的约定扩展既有入口，不增设常驻左导航，也不改变三个主视图的定稿结构。

| 补全功能 | 入口与布局约定 |
|---|---|
| 搜索 / 筛选 | 位置菜单底部增加“搜索扫描结果…”；Ctrl+F 也可打开。搜索状态在主分析区显示结果，右栏仍检查所选项；顶部是关键词输入、类型和最小大小筛选，提供明确“返回目录”动作。退出恢复原 scope / 展开 / 选择 |
| 复制文件 | 顶部文件操作菜单在“复制路径”后增加“复制到…”；复用移动表单的目标选择与冲突处理，右栏默认六项操作不扩张 |
| 系统关联打开 | 文件菜单增加“打开”；原有“打开所在位置”继续保留。通过用户明确点击打开文件，不在扫描或 hover 时打开 |
| 完整设置 | 外观浮层保留五强调色；增加“更多设置…”进入统一设置对话框，容纳主题、单位、默认视图，不把所有设置常驻在主界面 |
| 最近扫描 | 位置浮层增加“最近扫描”区，显示来源、时间及完整性；点击打开保存记录，读取旧记录不自动重新扫描 |

这张表描述正式产品新增状态，默认原型视觉验收仍用附录中的已有入口。M1 先完成原样复刻，后续里程碑按上述位置补齐功能；新增状态沿用本规范 tokens、控件与弹层。

## 14. 建议的前端拆分与复刻步骤

先建立一个仅使用 fixture 的页面，不接后端。按“外壳 → 树 → 右栏 → 地图 → 透镜 → 弹层 → 设置”的顺序拆分；每完成一块都对照附录的最终级联，而不是根据早期同名 CSS 声明重写。原型保留了历次调整的覆盖样式，**后出现的高优先级选择器是当前定稿**。

| 组件 / 模块 | 责任 |
|---|---|
| `AppShell` / `Toolbar` | 应用边界、位置、三视图、全局动作 |
| `WorkspaceStore` | 当前扫描、scope、selection、每视图展开状态、hover |
| `DirectoryTree` | 树虚拟列表、同级排序、覆盖选择、路径导航 |
| `SpaceMap` | treemap 布局、选中与下钻 |
| `HierarchyLens` | 双环几何、列表联动、中心返回 |
| `Inspector` | 单选 / 多选 / 空选；桌面栏与小窗口右抽屉 |
| `ActionController` | 将动作与所选集合绑定；共用弹层 |
| `ExportDialog` | 格式、范围、字段、预览、导出任务 |
| `AppearancePopover` / `theme.css` | 五强调色、浅深色、持久化设置 |
| `MockDataSource` / `NativeDataSource` | 同一查询契约，先 fixture 再真实扫描 |

CSS 可以按组件拆文件，但不要引入会改掉字体、按钮高度、圆角的 UI 框架默认皮肤。可以使用 headless 控件提供语义与键盘行为。对精确复刻来说，先锁定外观，再做代码整理；不能以“使用组件库更快”为由改变布局。

## 15. 验收清单

视觉验收使用同一 fixture，并为以下场景保留正式运行环境截图。截图对比检查几何与层次，同时人工检查文本、焦点与真实滚动；DOM 测试不能代替实际像素检查。

| 编号 | 场景 | 通过条件 |
|---|---|---|
| U01 | 1024 px、浅色、紫色、默认树 | 默认 14 行；右栏 220 px；无左导航、无底部动作栏 |
| U02 | 736 px、默认树 | 右栏仍在右边，宽 204 px；顶部保持紧凑；文本无横向溢出 |
| U03 | 640 px 与 639 px 边界 | 640 为常驻右栏；639 为详情按钮 / 右抽屉，绝不堆到底部 |
| U04 | 320 px、三个视图 | 保留主要内容和切换；抽屉可关闭、可滚动；没有文档级横向滚动 |
| U05 | Checkpoints 与 Games 多选 | 归一合计 386 GB；动作在右侧；清除不重置 scope |
| U06 | 父目录和其子文件一起选择 | 合计不重复；导出 / 移动 / 回收站使用同一集合 |
| U07 | 树 → 地图 → 透镜 → 树 | 传递主选择；深层项目在图形按直接父范围展示；主选择未变时恢复树的原范围与多选 |
| U08 | 地图下钻与返回 | 面包屑、矩形面积、右侧名称 / 大小一致；叶文件不可下钻 |
| U09 | 透镜 hover、选中、双击、中心 | hover 不改变操作对象；双击可靠；中心只表示当前范围并返回父级 |
| U10 | 五种强调色逐一切换 | 三视图、选择框、文件夹、主按钮同步；类别色与危险色不漂移 |
| U11 | 深色与高对比模式 | 字号和尺寸不变；正文可读、边界明确；不出现白色孤岛 |
| U12 | 四种导出 / 三种范围 / 递归开关 | 预览与状态一致；完整递归 33 条，根直接下级 6 条 |
| U13 | 键盘和弹层 | 所有关键动作可达；Escape / Tab / 焦点恢复正确；隐藏动作不可聚焦 |
| U14 | 长路径 / 空选 / 叶文件 / 根目录 | 换行或省略可控；无错误动作；内容不撑高或撑宽整个窗口 |
| U15 | reduced motion / 快速连续切换 | 动画可中断；无输入锁；减少动效时即刻到目标 |

交付时的验证边界：已通过 14 项基础和 18 项本版 DOM / 事件集成检查，共 32 项，覆盖三视图、右侧单选与多选、所选导出、范围转换、抽屉、强调色及无重复 ID；使用 Linkedom 运行实际脚本，测试几何为模拟值，尚未进行浏览器像素、真实滚动和真实键盘焦点验收。正式项目仍需在 Windows WebView2 中完成视觉截图、实际布局和 OS 文件操作验收；本文不把模拟 DOM 几何当成已经通过像素级测试。

## 16. 设计参考与采用方式

参考资料核查于 2026-10-11。下面说明设计来源与本产品的取舍，不要求复制其他软件的品牌、素材或每一个交互。

- [DiskBuddy · Space](https://diskbuddy.com/features/space)：参考“主区域分析 + 右侧项目检查”的信息安排，把定位文件后的操作留在视线终点。本版保留自己的顶部导航与树表工作流。
- [DaisyDisk · Understanding the disk map](https://daisydiskapp.com/guide/4/en/UnderstandingSunburst/)：参考环形层级、目录列表辅助读取、中心回到父级。我们明确区分 hover、selection 与 scope，并把单击选择和显式下钻分开。
- [Nektony Disk Space Analyzer · User Guide](https://nektony.com/help/disk-space-analyzer)：参考图形与目录列表并用，以及定位后直接处理文件的连续工作流。文件操作的真实语义以平台后端方案为准。
- [Emil Kowalski · apple-design skill](https://github.com/emilkowalski/skills/tree/main/skills/apple-design)：采用克制的层次、清晰字排、即时按压反馈、可中断动作与有来源位置的弹层。本版没有机械复刻 macOS 窗口外观。

## 附录 A：完整可交互参考源码

下方为本版实际展示的完整原型片段。格式为 UTF-8 / LF；SHA-256 为 `898f933a7dd8f3fb50b4b086e53ef45122c936981fbdbcf18325dd89d98b7f2e`，共 `218,004` 字节。源码包含完整演示数据，三种视图和共享交互，不依赖之前交付的文件。

它是可嵌入应用内容区的 HTML 片段，不包含 document 外壳。在独立页面中运行时，外层设置 `color-scheme:light dark`、`margin:0`、准确容器宽度，并在脚本执行前本地提供 Lucide。浅色 / 深色验收可在宿主分别固定 `color-scheme:light` / `dark`。把片段当文本塞进 React 的 `innerHTML` 不会可靠执行脚本；参考页面应通过构建期 HTML 或 iframe `srcdoc` 正常解析，正式实现则把行为迁移成组件。不得通过外部 fetch 才取得主数据。

可用以下脚本从**本文件自身**提取原型，不需要另一个下载链接：

```python
from pathlib import Path
import hashlib

doc = Path('spacefold-ui-spec.md').read_text(encoding='utf-8')
begin = '\n<!-- SPACEFOLD_REFERENCE_BEGIN -->\n````html\n'
end = '\n````\n<!-- SPACEFOLD_REFERENCE_END -->'
html = doc.split(begin, 1)[1].split(end, 1)[0]
data = html.encode('utf-8')
assert hashlib.sha256(data).hexdigest() == '898f933a7dd8f3fb50b4b086e53ef45122c936981fbdbcf18325dd89d98b7f2e'
Path('spacefold-reference.html').write_bytes(data)
```


<!-- SPACEFOLD_REFERENCE_BEGIN -->
````html
<style>
#spacefold-mac .disk-dialog-layer,#spacefold-mac .disk-action-toast{--da-panel:light-dark(#ffffff,#24292f);--da-ink:light-dark(#26313b,#edf2f5);--da-muted:light-dark(#6a7780,#a9b5bf);--da-line:light-dark(#e1e7eb,#3d4852);--da-soft:light-dark(#f2f5f7,#303840);--da-main:light-dark(#2a404e,#d5e4eb);--da-onmain:light-dark(#ffffff,#20313b);--da-success:light-dark(#2c775e,#8fd0b6);--da-danger:light-dark(#ac4949,#f0aaa6);font:13px/1.5 "Segoe UI Variable Text","Segoe UI","Microsoft YaHei",sans-serif;color:var(--da-ink)}
#spacefold-mac .disk-dialog-layer{position:absolute;inset:0;z-index:80;display:flex;align-items:flex-start;justify-content:center;padding:44px 18px 22px;background:light-dark(#13233155,#00000088);border-radius:inherit;box-sizing:border-box}
#spacefold-mac .disk-dialog-layer *{box-sizing:border-box}
#spacefold-mac .disk-dialog-layer .disk-dialog{width:min(560px,100%);max-height:min(570px,100%);overflow:auto;border:1px solid var(--da-line);border-radius:15px;background:var(--da-panel);box-shadow:0 18px 64px light-dark(#14233330,#00000066);color:var(--da-ink)}
#spacefold-mac .disk-dialog-layer .disk-dialog.da-compact{width:min(460px,100%)}
#spacefold-mac .disk-dialog-layer .da-head{display:flex;align-items:center;justify-content:space-between;gap:12px;padding:19px 22px 15px;border-bottom:1px solid var(--da-line)}
#spacefold-mac .disk-dialog-layer .da-title-wrap{display:flex;align-items:center;gap:10px;min-width:0}
#spacefold-mac .disk-dialog-layer .da-title{font-size:17px;line-height:1.4;margin:0;font-weight:500;color:var(--da-ink)}
#spacefold-mac .disk-dialog-layer .da-demo{font-size:11px;color:var(--da-muted);background:var(--da-soft);border-radius:5px;padding:2px 6px;white-space:nowrap;font-weight:400}
#spacefold-mac .disk-dialog-layer .da-body{padding:17px 22px 18px}
#spacefold-mac .disk-dialog-layer .da-footer{display:flex;justify-content:flex-end;align-items:center;gap:8px;padding:14px 22px;border-top:1px solid var(--da-line);background:var(--da-panel)}
#spacefold-mac .disk-dialog-layer .da-footer-note{margin-right:auto;color:var(--da-muted);font-size:11px}
#spacefold-mac .disk-dialog-layer .disk-dialog button{font:inherit;line-height:1.4;color:var(--da-ink);background:var(--da-panel);border:1px solid var(--da-line);padding:8px 12px;border-radius:8px;display:inline-flex;justify-content:center;align-items:center;gap:7px;min-height:35px;box-shadow:none;letter-spacing:normal;touch-action:manipulation}
#spacefold-mac .disk-dialog-layer .disk-dialog button:hover{background:var(--da-soft)}
#spacefold-mac .disk-dialog-layer .disk-dialog button.da-primary{background:var(--da-main);color:var(--da-onmain);border-color:transparent}
#spacefold-mac .disk-dialog-layer .disk-dialog button.da-danger{background:var(--da-danger);color:light-dark(#ffffff,#351b1b);border-color:transparent}
#spacefold-mac .disk-dialog-layer .disk-dialog button:disabled{opacity:.45}
#spacefold-mac .disk-dialog-layer .disk-dialog button.da-close{padding:6px;min-height:30px;width:30px;flex:none;border:0;background:transparent;color:var(--da-muted)}
#spacefold-mac .disk-dialog-layer .disk-dialog svg{width:16px;height:16px;flex:none}
#spacefold-mac .disk-dialog-layer .da-formats{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:8px;margin-bottom:15px}
#spacefold-mac .disk-dialog-layer .disk-dialog .da-format{display:flex;flex-direction:column;gap:2px;padding:9px 5px;border-radius:9px;font-weight:500}
#spacefold-mac .disk-dialog-layer .disk-dialog .da-format small{font-size:11px;font-weight:400;color:var(--da-muted)}
#spacefold-mac .disk-dialog-layer .disk-dialog .da-format[aria-pressed="true"]{background:var(--da-main);color:var(--da-onmain);border-color:var(--da-main)}
#spacefold-mac .disk-dialog-layer .disk-dialog .da-format[aria-pressed="true"] small{color:inherit;opacity:.8}
#spacefold-mac .disk-dialog-layer .da-field{display:flex;flex-direction:column;gap:7px;margin-bottom:13px;color:var(--da-ink);font-size:12px}
#spacefold-mac .disk-dialog-layer .da-field-title{font-weight:500;display:block;margin-bottom:7px;font-size:12px}
#spacefold-mac .disk-dialog-layer .disk-dialog input[type="text"],#spacefold-mac .disk-dialog-layer .disk-dialog select{display:block;width:100%;min-width:0;font:inherit;font-size:13px;line-height:1.4;padding:9px 10px;min-height:37px;border:1px solid var(--da-line);border-radius:7px;background:var(--da-panel);color:var(--da-ink)}
#spacefold-mac .disk-dialog-layer .da-checks{display:flex;flex-wrap:wrap;gap:8px 17px;margin-bottom:16px}
#spacefold-mac .disk-dialog-layer .da-check{display:inline-flex;gap:6px;align-items:center;font-size:12px;min-height:23px}
#spacefold-mac .disk-dialog-layer .da-check input{width:14px;height:14px;margin:0;accent-color:var(--da-main)}
#spacefold-mac .disk-dialog-layer .da-preview{border:1px solid var(--da-line);border-radius:9px;overflow:hidden;background:var(--da-panel)}
#spacefold-mac .disk-dialog-layer .da-preview-head{display:flex;justify-content:space-between;gap:10px;align-items:center;padding:8px 11px;background:var(--da-soft);font-size:11px;color:var(--da-muted)}
#spacefold-mac .disk-dialog-layer .da-preview-body{max-height:133px;min-height:110px;overflow:auto;padding:9px 11px}
#spacefold-mac .disk-dialog-layer .da-preview-body pre{padding:0;margin:0;font:11px/1.55 Consolas,"SFMono-Regular",monospace;color:var(--da-ink);white-space:pre-wrap;overflow-wrap:anywhere;background:transparent;border:0}
#spacefold-mac .disk-dialog-layer .da-preview-body table{width:100%;border-collapse:collapse;font-size:11px;color:var(--da-ink);margin:0}
#spacefold-mac .disk-dialog-layer .da-preview-body th,#spacefold-mac .disk-dialog-layer .da-preview-body td{padding:5px 7px;text-align:left;border:0;border-bottom:1px solid var(--da-line);font-weight:400;overflow-wrap:anywhere;vertical-align:top;background:transparent}
#spacefold-mac .disk-dialog-layer .da-preview-body th{font-weight:500;white-space:nowrap}
#spacefold-mac .disk-dialog-layer .da-preview-body tr:last-child td{border-bottom:0}
#spacefold-mac .disk-dialog-layer .da-export-result{min-height:18px;margin-top:8px;color:var(--da-success);font-size:11px}
#spacefold-mac .disk-dialog-layer .da-object{display:flex;align-items:center;gap:12px;padding:12px;background:var(--da-soft);border-radius:9px;margin-bottom:17px}
#spacefold-mac .disk-dialog-layer .da-object>svg{width:23px;height:23px;color:var(--da-muted)}
#spacefold-mac .disk-dialog-layer .da-object div{min-width:0}
#spacefold-mac .disk-dialog-layer .da-object strong{display:block;font-weight:500;overflow-wrap:anywhere}
#spacefold-mac .disk-dialog-layer .da-path{font-size:12px;overflow-wrap:anywhere;color:var(--da-muted);margin-top:3px}
#spacefold-mac .disk-dialog-layer .da-note{font-size:12px;color:var(--da-muted);margin:0 0 13px}
#spacefold-mac .disk-dialog-layer .da-kv{display:grid;grid-template-columns:auto minmax(0,1fr);gap:11px 18px;margin:0;font-size:13px}
#spacefold-mac .disk-dialog-layer .da-kv dt{color:var(--da-muted)}
#spacefold-mac .disk-dialog-layer .da-kv dd{margin:0;text-align:right;overflow-wrap:anywhere}
#spacefold-mac .disk-dialog-layer .da-error{font-size:12px;color:var(--da-danger);margin:0;min-height:18px}
#spacefold-mac .disk-dialog-layer .da-success{padding:14px 0 9px;text-align:center}
#spacefold-mac .disk-dialog-layer .da-success-icon{width:42px;height:42px;display:grid;place-items:center;margin:0 auto 13px;border-radius:50%;background:var(--da-soft);color:var(--da-success)}
#spacefold-mac .disk-dialog-layer .da-success-icon svg{width:23px;height:23px}
#spacefold-mac .disk-dialog-layer .da-success h3{font-size:17px;font-weight:500;margin:0 0 9px;color:var(--da-ink)}
#spacefold-mac .disk-dialog-layer .da-success p{font-size:12px;color:var(--da-muted);margin:0;overflow-wrap:anywhere}
#spacefold-mac .disk-dialog-layer .da-menu{display:grid;grid-template-columns:1fr 1fr;gap:7px}
#spacefold-mac .disk-dialog-layer .disk-dialog .da-menu button{justify-content:flex-start;text-align:left;padding:11px 10px}
#spacefold-mac .disk-dialog-layer .disk-dialog .da-menu button[data-kind="trash"]{color:var(--da-danger)}
#spacefold-mac .disk-action-toast{position:absolute;z-index:100;left:50%;top:63px;transform:translateX(-50%);max-width:calc(100% - 28px);display:flex;align-items:center;gap:8px;padding:11px 15px;background:var(--da-panel);border:1px solid var(--da-line);border-radius:10px;box-shadow:0 7px 25px light-dark(#20304020,#00000055);overflow-wrap:anywhere}
#spacefold-mac .disk-action-toast svg{width:17px;height:17px;flex:none;color:var(--da-success)}
@media(max-width:600px){#spacefold-mac .disk-dialog-layer{padding:24px 10px 18px}#spacefold-mac .disk-dialog-layer .disk-dialog{max-height:calc(100% - 2px);border-radius:12px}#spacefold-mac .disk-dialog-layer .da-head{padding:15px 15px 12px}#spacefold-mac .disk-dialog-layer .da-body{padding:14px 15px}#spacefold-mac .disk-dialog-layer .da-footer{padding:12px 15px;flex-wrap:wrap}#spacefold-mac .disk-dialog-layer .da-footer-note{width:100%;margin:0 0 2px}#spacefold-mac .disk-dialog-layer .da-checks{gap:7px 12px}#spacefold-mac .disk-dialog-layer .da-formats{gap:5px}#spacefold-mac .disk-dialog-layer .da-menu{grid-template-columns:1fr}#spacefold-mac .disk-dialog-layer .da-preview-body{max-height:115px;min-height:92px}}
@media(pointer:coarse){#spacefold-mac .disk-dialog-layer .disk-dialog button{min-height:44px}#spacefold-mac .disk-dialog-layer .disk-dialog .da-close{min-height:36px;width:36px}#spacefold-mac .disk-dialog-layer .disk-dialog input[type="text"],#spacefold-mac .disk-dialog-layer .disk-dialog select{font-size:16px}#spacefold-mac .disk-dialog-layer .da-check{min-height:32px}}
#spacefold-mac .disk-dialog-layer .disk-dialog textarea{display:block;width:100%;min-width:0;resize:vertical;font:13px/1.5 "Segoe UI Variable Text","Segoe UI","Microsoft YaHei",sans-serif;padding:9px 10px;border:1px solid var(--da-line);border-radius:7px;background:var(--da-panel);color:var(--da-ink)}
</style>
<style>
#disk-map{container-name:sfmap;container-type:inline-size;--dm-bg:light-dark(#ffffff,#252528);--dm-panel:light-dark(#f8f8fa,#2b2c30);--dm-ink:light-dark(#242427,#f0f0f2);--dm-sub:light-dark(#78787e,#a6a6af);--dm-line:light-dark(#e7e6e9,#3a3a40);--dm-soft:light-dark(#ededf1,#37383e);--dm-accent:light-dark(#7370be,#b4aaf0);--dm-accent-bg:light-dark(#eceaf9,#363149);--dm-main:light-dark(#007aff,#0a84ff);--dm-onmain:#ffffff;background:var(--dm-bg);color:var(--dm-ink);font:13px/1.45 -apple-system,BlinkMacSystemFont,"SF Pro Text","Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;position:relative;min-height:584px;display:flex;flex-direction:column;isolation:isolate}
#disk-map *{box-sizing:border-box}#disk-map [hidden]{display:none!important}#disk-map button{font:inherit;color:inherit;border:0;background:none;touch-action:manipulation;transition:background .14s ease,box-shadow .14s ease,transform .12s ease}#disk-map button:disabled{opacity:.38}#disk-map button:active:not(:disabled){filter:brightness(.94)}#disk-map button svg{width:15px;height:15px;flex:none}
#disk-map .dm-content{display:grid;grid-template-columns:minmax(0,1fr) 188px;flex:1;min-height:554px}#disk-map .dm-canvas-wrap{min-width:0;display:flex;flex-direction:column}#disk-map .dm-breadcrumb{display:flex;align-items:center;gap:3px;flex-wrap:wrap;min-height:40px;padding:5px 14px 5px 10px;font-size:12px;color:var(--dm-sub)}#disk-map .dm-breadcrumb button{max-width:155px;padding:4px 5px;border-radius:5px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}#disk-map .dm-breadcrumb button:hover{background:var(--dm-soft)}#disk-map .dm-breadcrumb button:last-of-type{color:var(--dm-ink);font-weight:600}#disk-map .dm-breadcrumb .dm-back{display:grid;place-items:center;width:24px;padding:4px;margin-right:2px;color:var(--dm-sub)}#disk-map .dm-breadcrumb>svg{width:11px;height:11px;opacity:.55}#disk-map .dm-area-label{margin-left:auto;white-space:nowrap;font-variant-numeric:tabular-nums}
#disk-map .dm-map{position:relative;flex:1;min-height:464px;margin:0 10px 0 12px}#disk-map .dm-tile{position:absolute;padding:0!important;border:0;border-radius:5px;overflow:hidden;text-align:left;background:var(--dm-tile-bg);color:var(--dm-tile-ink);display:flex;flex-direction:column;transition:box-shadow .14s ease,filter .14s ease}#disk-map .dm-tile:hover{filter:brightness(.977)}#disk-map .dm-tile[aria-pressed=true]{box-shadow:inset 0 0 0 2px light-dark(#8b7fbd,#bcb0df)}#disk-map .dm-tile[aria-pressed=true]:after{content:"";position:absolute;inset:3px;border:1px solid light-dark(#ffffffaa,#ffffff2b);border-radius:3px;pointer-events:none}#disk-map .dm-tile-heading{display:flex;gap:6px;justify-content:space-between;align-items:baseline;padding:11px 11px 5px;min-width:0;flex:none}#disk-map .dm-tile-name{display:block;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-weight:600;font-size:13px;letter-spacing:-.1px}#disk-map .dm-tile-size{white-space:nowrap;font-variant-numeric:tabular-nums;font-size:12px;font-weight:500;opacity:.82;flex:none}#disk-map .dm-tile-size small{font-size:11px;font-weight:400}#disk-map .dm-tile-detail{font-size:11px;opacity:.72;padding:0 11px 8px}#disk-map .dm-submap{position:absolute;top:58px;left:7px;right:7px;bottom:7px;overflow:hidden}#disk-map .dm-subtile{position:absolute;display:flex;flex-direction:column;align-items:flex-start;justify-content:flex-end;gap:2px;padding:8px 9px;background:color-mix(in srgb,var(--dm-tile-ink) var(--dm-sub-opacity),var(--dm-tile-bg));border-radius:3px;overflow:hidden}#disk-map .dm-subtile-name{font-size:11px;font-weight:500;max-width:100%;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}#disk-map .dm-subtile-size{font-size:11px;opacity:.76;font-variant-numeric:tabular-nums}#disk-map .dm-tile.compact .dm-tile-heading{display:block;padding:8px 9px 3px}#disk-map .dm-tile.compact .dm-tile-name{font-size:12px}#disk-map .dm-tile.compact .dm-tile-size{display:block;margin-top:2px}#disk-map .dm-tile.compact .dm-tile-detail{display:none}#disk-map .dm-tile.compact .dm-submap{top:49px;left:5px;right:5px;bottom:5px}#disk-map .dm-tile.compact .dm-subtile{padding:5px}#disk-map .dm-tile.micro .dm-tile-heading{padding:6px 7px;display:flex;gap:5px;align-items:center;flex-wrap:wrap}#disk-map .dm-tile.micro .dm-tile-name{font-size:11px}#disk-map .dm-tile.micro .dm-tile-size{font-size:11px;margin:0}#disk-map .dm-tile.micro .dm-submap,#disk-map .dm-tile.micro .dm-tile-detail{display:none}#disk-map .dm-tile-file .dm-tile-heading{display:block}#disk-map .dm-tile-file .dm-tile-name{white-space:normal;overflow-wrap:anywhere}#disk-map .dm-tile-file .dm-tile-size{font-size:22px;display:block;margin-top:8px;font-weight:400;letter-spacing:-.5px}#disk-map .dm-tile-file .dm-tile-size small{font-size:12px;letter-spacing:0}
#disk-map .dm-map-footer{min-height:42px;display:flex;align-items:center;gap:10px;padding:9px 14px 9px 13px;font-size:11px;color:var(--dm-sub)}#disk-map .dm-legend{display:flex;align-items:center;gap:10px;flex-wrap:wrap}#disk-map .dm-legend>span{display:inline-flex;align-items:center;gap:4px;white-space:nowrap}#disk-map .dm-dot{width:6px;height:6px;border-radius:2px;background:var(--dm-legend-color)}#disk-map .dm-map-scope{margin-left:auto;white-space:nowrap;font-variant-numeric:tabular-nums}
#disk-map .dm-inspector{background:var(--dm-panel);border-left:1px solid var(--dm-line);display:flex;flex-direction:column;min-width:0;padding:0 14px 13px;gap:0}#disk-map .dm-inspect-label{font-size:11px;font-weight:600;color:var(--dm-sub);height:40px;display:flex;align-items:center;justify-content:space-between}#disk-map .dm-inspect-label svg{width:14px;height:14px;opacity:.7}#disk-map .dm-folder-icon{display:flex;align-items:center;justify-content:center;color:light-dark(#75b9ea,#73a8d2);height:69px;flex:none}#disk-map .dm-folder-icon svg{width:54px;height:54px;stroke-width:1.15;fill:light-dark(#d0ecfc,#2c4f66)}#disk-map .dm-inspector h3{font-size:14px;font-weight:600;margin:3px 0 0;overflow-wrap:anywhere;letter-spacing:-.15px;text-align:center}#disk-map .dm-path{font-size:11px;line-height:1.5;overflow-wrap:anywhere;color:var(--dm-sub);margin:6px 0 0;text-align:center}#disk-map .dm-size-block{text-align:center;margin:23px 0 18px}#disk-map .dm-selected-size{font-size:27px;font-weight:400;letter-spacing:-.7px;line-height:1.2;font-variant-numeric:tabular-nums}#disk-map .dm-size-label{font-size:11px;color:var(--dm-sub);margin-top:3px}#disk-map .dm-inspector dl{margin:0 0 17px;display:grid;grid-template-columns:1fr auto;gap:8px 5px;font-size:11px}#disk-map .dm-inspector dt{color:var(--dm-sub)}#disk-map .dm-inspector dd{margin:0;text-align:right;font-variant-numeric:tabular-nums}#disk-map .dm-button{display:inline-flex;align-items:center;justify-content:center;gap:5px;min-height:26px;padding:4px 8px;border:1px solid light-dark(#d7d7dd,#505157);border-radius:6px;background:linear-gradient(light-dark(#ffffff,#4a4b50),light-dark(#fafafa,#414247));box-shadow:0 1px 2px light-dark(#0000000b,#00000024);font-size:13px;white-space:nowrap}#disk-map .dm-button:hover:not(:disabled){background:var(--dm-soft)}#disk-map .dm-button:active:not(:disabled){transform:scale(.98)}#disk-map .dm-enter{width:100%;flex:none}#disk-map .dm-enter svg{width:12px;height:12px}#disk-map .dm-inspect-separator{margin-top:auto;padding-top:20px;display:flex;flex-direction:column;gap:1px}#disk-map .dm-op{display:flex;align-items:center;gap:7px;width:100%;padding:7px 5px;border-radius:5px;font-size:13px;white-space:nowrap;text-align:left;min-height:28px}#disk-map .dm-op:hover{background:var(--dm-soft)}#disk-map .dm-op svg{width:13px;height:13px;color:var(--dm-sub)}#disk-map .dm-op span{margin-left:auto;color:var(--dm-sub);font-size:11px}#disk-map .dm-op.dm-destructive,#disk-map .dm-op.dm-destructive svg{color:light-dark(#c35453,#ed8b88)}
#disk-map .dm-status{min-height:27px;display:flex;align-items:center;justify-content:space-between;gap:10px;padding:5px 13px;font-size:11px;color:var(--dm-sub);background:var(--dm-panel);border-top:1px solid var(--dm-line)}#disk-map .dm-status-left{display:flex;align-items:center;gap:6px}#disk-map .dm-status-left svg{width:11px;height:11px;opacity:.7}#disk-map .dm-live{font-variant-numeric:tabular-nums}#disk-map .dm-status-end{white-space:nowrap}
@container sfmap (max-width:760px){#disk-map .dm-content{grid-template-columns:minmax(0,1fr) 168px}#disk-map .dm-inspector{padding-left:11px;padding-right:11px}#disk-map .dm-map-scope{display:none}#disk-map .dm-tile-heading{display:block}#disk-map .dm-tile-size{display:block;margin-top:2px}#disk-map .dm-submap{top:67px}#disk-map .dm-tile-detail{display:none}#disk-map .dm-submap{top:53px}}
@container sfmap (max-width:580px){#disk-map{min-height:0}#disk-map .dm-content{display:flex;flex-direction:column;min-height:0}#disk-map .dm-map{flex:none;height:364px;min-height:364px;margin:0 10px}#disk-map .dm-inspector{display:grid;grid-template-columns:54px minmax(0,1fr) 104px;column-gap:12px;row-gap:4px;border-left:0;border-top:1px solid var(--dm-line);padding:11px 13px 13px}#disk-map .dm-inspect-label{display:none}#disk-map .dm-folder-icon{height:57px;grid-column:1;grid-row:1/3}#disk-map .dm-folder-icon svg{width:43px;height:43px}#disk-map .dm-inspector h3{text-align:left;margin:0;font-size:13px}#disk-map .dm-inspector .dm-path{text-align:left;margin-top:3px}#disk-map .dm-size-block{grid-column:3;grid-row:1;margin:0;text-align:right}#disk-map .dm-selected-size{font-size:22px}#disk-map .dm-inspector dl{display:none}#disk-map .dm-enter{grid-column:3;grid-row:2;align-self:start}#disk-map .dm-inspect-separator{grid-column:1/-1;display:grid;grid-template-columns:1fr 1fr;gap:0 10px;margin:4px 0 0;padding-top:5px;border-top:1px solid var(--dm-line)}#disk-map .dm-status-end{display:none}#disk-map .dm-map-footer{padding:10px 12px}#disk-map .dm-legend{gap:8px}#disk-map .dm-breadcrumb{padding-left:7px}#disk-map .dm-area-label{display:none}}
@media(pointer:coarse){#disk-map .dm-button,#disk-map .dm-op,#disk-map .dm-breadcrumb button{min-height:44px}#disk-map .dm-breadcrumb .dm-back{min-width:36px}#disk-map .dm-inspect-separator{gap:3px}}
@media(prefers-reduced-motion:reduce){#disk-map *{transition:none!important;animation:none!important}}
#disk-map{--dm-bg:var(--sf-panel,light-dark(#ffffff,#252528));--dm-panel:light-dark(#faf9fc,#29282e);--dm-ink:var(--sf-ink,light-dark(#24242b,#f0f0f3));--dm-sub:var(--sf-muted,light-dark(#72717c,#aaa8b4));--dm-line:var(--sf-line,light-dark(#e8e7ee,#3a3943));--dm-accent:var(--sf-accent,light-dark(#7654bb,#bf9ff5));--dm-accent-bg:var(--sf-accent-soft,light-dark(#eee9f8,#363040));min-height:596px}
#disk-map .dm-content{grid-template-columns:minmax(0,1fr) 220px;min-height:596px}
#disk-map .dm-breadcrumb{min-height:51px;padding:10px 18px 9px 12px;font-size:13px;gap:3px}
#disk-map .dm-breadcrumb button{padding:5px 6px;max-width:176px;font-weight:400}
#disk-map .dm-breadcrumb button:last-of-type{font-weight:500}
#disk-map .dm-breadcrumb .dm-back{width:28px;padding:5px;margin-right:4px}
#disk-map .dm-area-label{font-size:12px;color:var(--dm-sub)}
#disk-map .dm-map{margin:0 16px 0 18px;min-height:480px}
#disk-map .dm-tile{border-radius:7px;background:var(--dm-tile-bg);color:var(--dm-ink)}
#disk-map .dm-tile[aria-pressed=true]{box-shadow:inset 0 0 0 2px var(--dm-accent)}
#disk-map .dm-tile[aria-pressed=true]:after{display:none}
#disk-map .dm-tile-heading{padding:13px 12px 4px;gap:8px}
#disk-map .dm-tile-name{font-size:14px;font-weight:500;letter-spacing:0}
#disk-map .dm-tile-size{font-size:13px;font-weight:500;opacity:1}
#disk-map .dm-tile-detail{font-size:11px;padding:0 12px 8px;color:var(--dm-sub);opacity:1}
#disk-map .dm-submap{top:61px;left:7px;right:7px;bottom:7px}
#disk-map .dm-subtile{background:color-mix(in srgb,var(--dm-tile-color) var(--dm-sub-opacity),var(--dm-bg));border-radius:4px;padding:9px;color:var(--dm-ink)}
#disk-map .dm-subtile-name{font-size:12px;font-weight:400}
#disk-map .dm-subtile-size{font-size:11px;color:var(--dm-sub);opacity:1}
#disk-map .dm-tile.compact .dm-tile-name{font-size:13px}
#disk-map .dm-tile.compact .dm-tile-heading{padding:10px 10px 4px}
#disk-map .dm-tile.compact .dm-submap{top:52px;left:6px;right:6px;bottom:6px}
#disk-map .dm-tile.micro .dm-tile-name,#disk-map .dm-tile.micro .dm-tile-size{font-size:11px}
#disk-map .dm-tile-file .dm-tile-name{font-size:13px}
#disk-map .dm-tile-file .dm-tile-size{font-size:24px;font-weight:400;letter-spacing:-.6px;margin-top:11px}
#disk-map .dm-map-footer{min-height:43px;padding:10px 19px;justify-content:space-between;gap:10px;font-size:11px}
#disk-map .dm-map-scope{display:block;margin:0}
#disk-map .dm-inspector{padding:0 18px 16px;background:var(--dm-panel);border-left:1px solid var(--dm-line);gap:0}
#disk-map .dm-inspect-label{height:51px;font-size:12px;font-weight:400}
#disk-map .dm-object{display:flex;align-items:center;gap:9px;margin-top:5px;min-width:0}
#disk-map .dm-folder-icon{height:auto;flex:none;color:var(--sf-accent-folder,var(--dm-accent))}
#disk-map .dm-folder-icon svg{width:28px;height:28px;stroke-width:1.45;fill:var(--sf-accent-folder-fill,var(--dm-accent-bg))}
#disk-map .dm-inspector h3{font-size:14px;font-weight:500;line-height:1.4;text-align:left;margin:0;letter-spacing:0;min-width:0}
#disk-map .dm-path{text-align:left;font-size:11px;margin:8px 0 0;color:var(--dm-sub);overflow-wrap:anywhere}
#disk-map .dm-size-block{text-align:left;margin:21px 0 18px}
#disk-map .dm-selected-size{font-size:29px;line-height:1.2;font-weight:400;letter-spacing:-.9px}
#disk-map .dm-selected-size span{font-size:13px;letter-spacing:0;color:var(--dm-sub);margin-left:3px}
#disk-map .dm-size-label{margin-top:4px;font-size:11px}
#disk-map .dm-inspector dl{font-size:12px;gap:8px;margin:0 0 17px}
#disk-map .dm-button{background:var(--dm-accent-bg);border:0;box-shadow:none;color:var(--dm-accent);font-size:13px;min-height:33px;border-radius:6px;padding:7px 9px}
#disk-map .dm-enter{justify-content:space-between}
#disk-map .dm-children{margin-top:23px}
#disk-map .dm-children-title{display:flex;align-items:center;justify-content:space-between;font-size:11px;color:var(--dm-sub);margin-bottom:7px}
#disk-map .dm-child-list{display:flex;flex-direction:column;gap:1px}
#disk-map .dm-child{display:flex;align-items:center;gap:7px;width:100%;min-width:0;padding:7px 3px;text-align:left;border-radius:5px;min-height:31px}
#disk-map .dm-child:hover{background:var(--dm-accent-bg)}
#disk-map .dm-child-name{min-width:0;flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:12px}
#disk-map .dm-child-size{font-size:11px;white-space:nowrap;font-variant-numeric:tabular-nums;color:var(--dm-sub)}
#disk-map .dm-child svg{width:11px;height:11px;color:var(--dm-sub)}
#disk-map .dm-child-more{font-size:11px;color:var(--dm-sub);padding-top:5px}
#disk-map .dm-inspect-separator{margin-top:auto;padding-top:19px;display:grid;grid-template-columns:1fr 1fr;gap:2px 7px}
#disk-map .dm-op{font-size:12px;gap:6px;min-height:31px;padding:6px 2px}
#disk-map .dm-op svg{width:13px;height:13px}
#disk-map .dm-live{position:absolute;width:1px;height:1px;margin:-1px;padding:0;overflow:hidden;clip-path:inset(50%);white-space:nowrap}
@container sfmap (max-width:790px){#disk-map .dm-content{grid-template-columns:minmax(0,1fr) 210px}#disk-map .dm-inspector{padding:0 15px 15px}#disk-map .dm-map{margin-left:14px;margin-right:12px}#disk-map .dm-tile-heading{display:block}#disk-map .dm-tile-size{display:block;margin-top:3px}#disk-map .dm-tile-detail{display:none}#disk-map .dm-submap{top:61px}}
@container sfmap (max-width:640px){#disk-map .dm-content{display:flex;flex-direction:column;min-height:0}#disk-map .dm-map{height:385px;min-height:385px;flex:none;margin:0 13px}#disk-map .dm-inspector{display:grid;grid-template-columns:minmax(0,1fr) 105px;gap:8px 16px;border-left:0;border-top:1px solid var(--dm-line);padding:17px}#disk-map .dm-inspect-label{display:none}#disk-map .dm-object{grid-column:1;grid-row:1;margin:0}#disk-map .dm-folder-icon{grid-column:auto;grid-row:auto;height:auto}#disk-map .dm-folder-icon svg{width:24px;height:24px}#disk-map .dm-inspector h3{font-size:14px}#disk-map .dm-path{grid-column:1;grid-row:2;margin:0;font-size:11px}#disk-map .dm-size-block{grid-column:2;grid-row:1/3;margin:0;text-align:right;align-self:center}#disk-map .dm-selected-size{font-size:24px}#disk-map .dm-enter{grid-column:2;grid-row:3;align-self:start}#disk-map .dm-inspector dl{display:grid;grid-column:1;grid-row:3;grid-template-columns:auto auto;justify-content:start;gap:4px 12px;margin:0;font-size:11px}#disk-map .dm-inspector dl dt:nth-of-type(2),#disk-map .dm-inspector dl dd:nth-of-type(2){display:none}#disk-map .dm-children{grid-column:1/-1;margin:11px 0 0}#disk-map .dm-child-list{display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1fr);gap:3px 20px}#disk-map .dm-inspect-separator{grid-column:1/-1;grid-template-columns:repeat(3,minmax(0,1fr));gap:3px 9px;margin:7px 0 0;padding:10px 0 0;border-top:1px solid var(--dm-line)}#disk-map .dm-area-label{display:none}#disk-map .dm-map-scope{display:block}#disk-map .dm-map-footer{padding:10px 15px}}
@container sfmap (max-width:360px){#disk-map .dm-inspector{padding:14px;gap:8px 11px}#disk-map .dm-child-list{display:flex;gap:1px}#disk-map .dm-inspect-separator{grid-template-columns:1fr 1fr}#disk-map .dm-map{height:350px;min-height:350px}#disk-map .dm-map-footer{font-size:11px;gap:8px;padding:10px 14px}#disk-map .dm-breadcrumb{padding-left:8px}#disk-map .dm-breadcrumb button{max-width:140px}}
@media(pointer:coarse){#disk-map .dm-button,#disk-map .dm-op,#disk-map .dm-child,#disk-map .dm-breadcrumb button{min-height:44px}#disk-map .dm-breadcrumb .dm-back{min-width:44px}}
</style>
<style>
#disk-workbench {
  --wb-background: var(--sf-panel, light-dark(#ffffff, #202022));
  --wb-surface: color-mix(in srgb, var(--sf-soft, light-dark(#f0f0f2, #333336)) 33%, var(--wb-background));
  --wb-soft: var(--sf-soft, light-dark(#f0f0f2, #333336));
  --wb-stripe: color-mix(in srgb, var(--wb-soft) 26%, var(--wb-background));
  --wb-text: var(--sf-ink, light-dark(#232326, #eeeef0));
  --wb-muted: var(--sf-muted, light-dark(#717178, #acacb4));
  --wb-line: var(--sf-line, light-dark(#e3e3e6, #3a3a3d));
  --wb-guide-line: color-mix(in srgb, var(--wb-muted) 16%, var(--wb-background));
  --wb-accent: var(--sf-accent, light-dark(#7654bb, #bf9ff5));
  --wb-accent-soft: var(--sf-accent-soft, color-mix(in srgb, var(--wb-accent) 10%, var(--wb-background)));
  --wb-selected: var(--sf-accent-selected, color-mix(in srgb, var(--wb-accent) 15%, var(--wb-background)));
  --wb-covered: var(--sf-accent-covered, color-mix(in srgb, var(--wb-accent) 4%, var(--wb-background)));
  --wb-bar: color-mix(in srgb, var(--wb-muted) 52%, var(--wb-background));
  --wb-folder: var(--sf-accent-folder, var(--wb-accent));
  --wb-folder-fill: var(--sf-accent-folder-fill, color-mix(in srgb, var(--wb-accent) 24%, var(--wb-background)));
  --wb-neutral-button: var(--wb-background);
  --wb-neutral-foreground: var(--wb-text);
  --wb-danger: light-dark(#c33d39, #ff8984);
  --wb-danger-soft: light-dark(#fff0ee, #472b2b);
  --wb-success: light-dark(#3a9664, #6eb48e);
  color: var(--wb-text); background: var(--wb-background);
  font: 400 14px/1.45 -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
  text-align: left; width: 100%; min-width: 0; min-height: 520px;
  display: flex; flex-direction: column; position: relative; isolation: isolate;
  box-sizing: border-box; container-name: mac-tree; container-type: inline-size;
}
#disk-workbench *, #disk-workbench *::before, #disk-workbench *::after { box-sizing: border-box; }
#disk-workbench button, #disk-workbench input { font: inherit; }
#disk-workbench button { color: inherit; transition: background-color 120ms ease, opacity 120ms ease, transform 120ms ease; }
#disk-workbench button:active:not(:disabled) { transform: scale(.975); transition-duration: 0ms; }
#disk-workbench button:disabled { opacity: .35; }
#disk-workbench svg { width: 16px; height: 16px; flex-shrink: 0; stroke-width: 1.7; }
#disk-workbench .wb-number { font-variant-numeric: tabular-nums; }
#disk-workbench .wb-topbar { display: none; }
#disk-workbench .wb-button { min-height: 28px; display: inline-flex; justify-content: center; align-items: center; gap: 7px; padding: 4px 9px; border: 1px solid var(--wb-line); border-radius: 6px; background: var(--wb-neutral-button); color: var(--wb-neutral-foreground); white-space: nowrap; font-size: 13px; }
#disk-workbench .wb-button:hover:not(:disabled) { background: var(--wb-soft); }
#disk-workbench .wb-button-ghost { border-color: transparent; background: transparent; }
#disk-workbench .wb-icon-button { width: 28px; min-width: 28px; padding: 5px; }
#disk-workbench .wb-body, #disk-workbench .wb-main { min-width: 0; display: flex; flex-direction: column; flex: 1; }
#disk-workbench .wb-pathbar { display: flex; align-items: center; min-height: 44px; padding: 6px 14px; gap: 7px; border-bottom: 1px solid var(--wb-line); background: var(--wb-background); }
#disk-workbench .wb-breadcrumbs { min-width: 0; display: flex; align-items: center; gap: 2px; font-size: 12px; flex: 1; flex-wrap: wrap; }
#disk-workbench .wb-breadcrumbs button { min-height: 27px; padding: 3px 5px; border: 0; border-radius: 5px; background: transparent; white-space: nowrap; font-size: 13px; color: var(--wb-muted); }
#disk-workbench .wb-breadcrumbs button:hover { background: var(--wb-soft); color: var(--wb-text); }
#disk-workbench .wb-breadcrumbs button:last-child { color: var(--wb-text); font-weight: 500; }
#disk-workbench .wb-breadcrumbs > svg { width: 11px; height: 11px; color: var(--wb-muted); }
#disk-workbench .wb-path-size { color: var(--wb-muted); font-size: 13px; white-space: nowrap; margin: 0 7px; }
#disk-workbench .wb-tree-tools { display: flex; gap: 1px; padding-left: 7px; border-left: 1px solid var(--wb-line); }
#disk-workbench .wb-tree-tools .wb-button { color: var(--wb-muted); }
#disk-workbench .wb-content { min-width: 0; display: grid; grid-template-columns: minmax(0, 1fr) 220px; flex: 1; }
#disk-workbench .wb-directory { min-width: 0; padding: 0 10px 18px; }
#disk-workbench .wb-table-zone { min-width: 0; }
#disk-workbench .wb-table { width: 100%; border-collapse: separate; border-spacing: 0; table-layout: fixed; font-size: 14px; }
#disk-workbench .wb-table col.wb-check-col { width: 28px; }
#disk-workbench .wb-table col.wb-name-col { width: auto; }
#disk-workbench .wb-table col.wb-size-col { width: 86px; }
#disk-workbench .wb-table col.wb-share-col { width: 122px; }
#disk-workbench .wb-table col.wb-type-col { width: 0; }
#disk-workbench .wb-table col.wb-open-col { width: 30px; }
#disk-workbench .wb-table th { height: 35px; padding: 0 9px; border-bottom: 1px solid var(--wb-line); color: var(--wb-muted); text-align: left; font-size: 12px; font-weight: 400; white-space: nowrap; }
#disk-workbench .wb-table th.wb-align-right, #disk-workbench .wb-table td.wb-align-right { text-align: right; }
#disk-workbench .wb-table th button { border: 0; background: transparent; padding: 3px 0; display: inline-flex; align-items: center; gap: 3px; color: inherit; font-size: 12px; }
#disk-workbench .wb-table th button svg { width: 11px; height: 11px; }
#disk-workbench .wb-table td { height: 36px; padding: 0 9px; border: 0; }
#disk-workbench .wb-table td.wb-check-cell, #disk-workbench .wb-table th.wb-check-head { padding: 0; }
#disk-workbench .wb-table td.wb-name-cell { padding: 0; }
#disk-workbench .wb-table td:last-child { padding: 0; }
#disk-workbench .wb-table tbody tr:nth-child(even) { background: var(--wb-stripe); }
#disk-workbench .wb-table tbody tr:hover { background: var(--wb-soft); }
#disk-workbench .wb-table tbody tr.wb-covered { background: var(--wb-covered); }
#disk-workbench .wb-table tbody tr.wb-selected { background: color-mix(in srgb, var(--wb-selected) 76%, var(--wb-background)); }
#disk-workbench .wb-table tbody tr td:first-child { border-radius: 5px 0 0 5px; }
#disk-workbench .wb-table tbody tr td:last-child { border-radius: 0 5px 5px 0; }
#disk-workbench .wb-check-hit { display: flex; align-items: center; justify-content: center; min-height: 36px; width: 100%; }
#disk-workbench .wb-check { appearance: auto; width: 14px; height: 14px; margin: 0; accent-color: var(--wb-accent); }
#disk-workbench .wb-covered .wb-check { opacity: .55; }
#disk-workbench .wb-node { --wb-step: 17px; --wb-chevron-mid: 10px; position: relative; display: flex; align-items: stretch; min-width: 0; min-height: 36px; padding-left: calc(var(--wb-depth) * var(--wb-step)); }
#disk-workbench .wb-guide { display: block; position: absolute; top: 0; bottom: 0; left: calc(var(--wb-level) * var(--wb-step) + var(--wb-chevron-mid)); width: calc(var(--wb-step) - 5px); border-left: 1px solid var(--wb-guide-line); pointer-events: none; }
#disk-workbench .wb-elbow::after { content: ""; position: absolute; top: 50%; left: 0; right: 0; border-top: 1px solid var(--wb-guide-line); }
#disk-workbench .wb-elbow.wb-last { bottom: 50%; }
#disk-workbench .wb-elbow.wb-last::after { top: 100%; }
#disk-workbench .wb-chevron, #disk-workbench .wb-chevron-space { width: 20px; flex: 0 0 20px; min-height: 36px; display: flex; align-items: center; justify-content: center; padding: 0; border: 0; background: transparent; position: relative; }
#disk-workbench .wb-chevron { color: var(--wb-muted); }
#disk-workbench .wb-chevron:hover { color: var(--wb-text); }
#disk-workbench .wb-chevron svg { width: 12px; height: 12px; stroke-width: 2.2; }
#disk-workbench .wb-folder-select { display: flex; flex: 1; width: 0; min-width: 0; gap: 7px; align-items: center; border: 0; padding: 6px 3px 6px 3px; background: transparent; text-align: left; font-size: 14px; }
#disk-workbench .wb-folder-icon { display: flex; align-items: center; justify-content: center; flex: 0 0 18px; width: 18px; height: 22px; color: var(--wb-folder); }
#disk-workbench .wb-folder-icon svg { width: 18px; height: 18px; stroke-width: 1.3; fill: var(--wb-folder-fill); }
#disk-workbench [data-wb-node-type="file"] .wb-folder-icon svg, #disk-workbench .wb-root-row .wb-folder-icon svg { fill: none; color: var(--wb-muted); stroke-width: 1.6; }
#disk-workbench .wb-folder-text { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
#disk-workbench .wb-root-row .wb-folder-text { font-weight: 500; }
#disk-workbench .wb-folder-size { font-weight: 400; white-space: nowrap; }
#disk-workbench .wb-root-row .wb-folder-size, #disk-workbench .wb-selected .wb-folder-size { font-weight: 500; }
#disk-workbench [data-category="AI Models"] { --wb-row-color: var(--sf-ai, var(--wb-bar)); }
#disk-workbench [data-category="Games"] { --wb-row-color: var(--sf-games, var(--wb-bar)); }
#disk-workbench [data-category="Media"] { --wb-row-color: var(--sf-media, var(--wb-bar)); }
#disk-workbench [data-category="Projects"] { --wb-row-color: var(--sf-projects, var(--wb-bar)); }
#disk-workbench [data-category="Downloads"] { --wb-row-color: var(--sf-downloads, var(--wb-bar)); }
#disk-workbench [data-category="Others"] { --wb-row-color: var(--sf-others, var(--wb-bar)); }
#disk-workbench .wb-share { display: grid; grid-template-columns: minmax(0, 1fr) 43px; align-items: center; gap: 8px; }
#disk-workbench .wb-share-track { height: 4px; border-radius: 3px; background: var(--wb-soft); overflow: hidden; }
#disk-workbench .wb-share-track span { display: block; height: 100%; background: var(--wb-row-color, var(--wb-bar)); border-radius: inherit; }
#disk-workbench .wb-share-value { color: var(--wb-muted); font-size: 12px; text-align: right; }
#disk-workbench .wb-type-col, #disk-workbench .wb-type-cell, #disk-workbench .wb-type-head { display: none; }
#disk-workbench .wb-enter { display: inline-flex; align-items: center; justify-content: center; width: 29px; min-height: 30px; border: 0; border-radius: 5px; padding: 0; background: transparent; color: var(--wb-muted); }
#disk-workbench .wb-enter svg { width: 13px; height: 13px; }
#disk-workbench .wb-enter:hover { background: var(--wb-accent-soft); color: var(--wb-accent); }
#disk-workbench .wb-inspector { min-width: 0; margin: 0; padding: 18px 16px; border: 0; border-left: 1px solid var(--wb-line); background: var(--wb-surface); }
#disk-workbench .wb-inspector-heading { margin: 0 0 17px; color: var(--wb-muted); font-size: 12px; font-weight: 400; }
#disk-workbench .wb-selection-top { display: flex; align-items: flex-start; gap: 11px; text-align: left; }
#disk-workbench .wb-selection-icon { display: grid; place-items: center; width: 32px; min-width: 32px; height: 34px; color: var(--wb-folder); }
#disk-workbench .wb-selection-icon svg { width: 30px; height: 30px; stroke-width: 1.1; fill: var(--wb-folder-fill); }
#disk-workbench .wb-selection-icon[data-kind="file"] svg, #disk-workbench .wb-selection-icon[data-kind="disk"] svg { fill: none; stroke-width: 1.4; color: var(--wb-muted); }
#disk-workbench .wb-selection-text { min-width: 0; width: 100%; }
#disk-workbench .wb-selection-title { font-weight: 500; font-size: 15px; overflow-wrap: anywhere; }
#disk-workbench .wb-selection-kind { display: block; margin-top: 3px; font-size: 12px; font-weight: 400; color: var(--wb-muted); }
#disk-workbench .wb-selection-metric { padding: 23px 0 18px; text-align: left; }
#disk-workbench .wb-selection-size { font-size: 32px; line-height: 1.12; font-weight: 400; letter-spacing: -.8px; }
#disk-workbench .wb-selection-meta { margin-top: 7px; color: var(--wb-muted); font-size: 12px; }
#disk-workbench .wb-selection-track { height: 4px; margin-top: 11px; overflow: hidden; border-radius: 4px; background: var(--wb-soft); }
#disk-workbench .wb-selection-track span { display: block; height: 100%; width: var(--wb-selected-percent, 38.5%); background: var(--wb-accent); border-radius: inherit; }
#disk-workbench .wb-location { padding: 17px 0 0; }
#disk-workbench .wb-location-label { display: block; font-size: 12px; color: var(--wb-muted); margin-bottom: 5px; }
#disk-workbench .wb-selection-path { color: var(--wb-text); font-size: 12px; overflow-wrap: anywhere; }
#disk-workbench .wb-actions { display: flex; flex-direction: column; gap: 7px; padding: 0 0 17px; border-bottom: 1px solid var(--wb-line); }
#disk-workbench .wb-primary-actions { display: grid; grid-template-columns: minmax(0, 1fr) 68px; gap: 6px; }
#disk-workbench .wb-secondary-actions { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 3px 4px; }
#disk-workbench .wb-actions .wb-button { justify-content: flex-start; min-width: 0; min-height: 31px; font-size: 12px; padding: 5px; gap: 5px; border: 0; background: transparent; border-radius: 6px; }
#disk-workbench .wb-actions .wb-button svg { width: 14px; height: 14px; color: var(--wb-muted); }
#disk-workbench .wb-actions .wb-primary-actions .wb-button { justify-content: center; min-height: 33px; font-size: 13px; padding: 6px; background: var(--wb-background); box-shadow: inset 0 0 0 1px var(--wb-line); }
#disk-workbench .wb-actions .wb-primary-actions .wb-action-open { color: var(--wb-accent); background: var(--wb-accent-soft); box-shadow: none; }
#disk-workbench .wb-actions .wb-primary-actions .wb-action-open svg { color: inherit; }
#disk-workbench .wb-actions .wb-button:hover:not(:disabled) { background: var(--wb-soft); }
#disk-workbench .wb-actions .wb-action-trash { color: var(--wb-danger); }
#disk-workbench .wb-actions .wb-action-trash svg { color: inherit; }
#disk-workbench .wb-actions .wb-action-trash:hover:not(:disabled) { background: var(--wb-danger-soft); }
#disk-workbench .wb-inspector[data-wb-selection-mode="multiple"] .wb-actions { display: none; }
#disk-workbench .wb-breakdown { padding-top: 15px; }
#disk-workbench .wb-breakdown[hidden], #disk-workbench .wb-breakdown-more[hidden], #disk-workbench [data-wb-focus][hidden] { display: none; }
#disk-workbench .wb-breakdown-heading { display: flex; justify-content: space-between; align-items: center; gap: 8px; margin-bottom: 5px; }
#disk-workbench .wb-breakdown-heading h4 { margin: 0; font-size: 12px; font-weight: 400; color: var(--wb-muted); }
#disk-workbench .wb-breakdown-heading button { display: inline-flex; align-items: center; gap: 3px; min-height: 25px; padding: 3px 0 3px 5px; font-size: 12px; color: var(--wb-accent); border: 0; border-radius: 4px; background: transparent; }
#disk-workbench .wb-breakdown-heading button svg { width: 12px; height: 12px; }
#disk-workbench .wb-breakdown-heading button:hover { background: var(--wb-accent-soft); }
#disk-workbench .wb-breakdown-list { display: grid; gap: 1px; }
#disk-workbench .wb-breakdown-item { display: flex; flex-direction: column; justify-content: center; gap: 5px; min-width: 0; width: 100%; min-height: 37px; padding: 4px 5px; border: 0; border-radius: 5px; background: transparent; text-align: left; }
#disk-workbench .wb-breakdown-item:hover { background: var(--wb-soft); }
#disk-workbench .wb-breakdown-label { display: flex; gap: 8px; align-items: baseline; justify-content: space-between; width: 100%; font-size: 12px; }
#disk-workbench .wb-breakdown-name { min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
#disk-workbench .wb-breakdown-size { flex-shrink: 0; font-size: 12px; color: var(--wb-muted); }
#disk-workbench .wb-breakdown-bar { width: 100%; height: 3px; background: var(--wb-soft); border-radius: 3px; overflow: hidden; }
#disk-workbench .wb-breakdown-bar span { display: block; height: 100%; width: var(--wb-part); background: var(--wb-row-color, var(--wb-bar)); border-radius: inherit; }
#disk-workbench .wb-breakdown-more { margin: 8px 5px 0; font-size: 12px; color: var(--wb-muted); }
#disk-workbench .wb-statusbar { min-height: 31px; padding: 6px 15px; border-top: 1px solid var(--wb-line); background: var(--wb-background); display: flex; align-items: center; gap: 7px; color: var(--wb-muted); font-size: 12px; }
#disk-workbench .wb-status-dot { display: block; width: 4px; height: 4px; border-radius: 50%; background: var(--wb-success); flex-shrink: 0; }
#disk-workbench .wb-status-right { margin-left: auto; white-space: nowrap; }
#disk-workbench .wb-accessible { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
@container mac-tree (max-width: 780px) {
  #disk-workbench .wb-content { display: flex; flex-direction: column; }
  #disk-workbench .wb-directory { padding-bottom: 14px; }
  #disk-workbench .wb-inspector { display: grid; grid-template-columns: minmax(0, 1fr) minmax(150px, .6fr); gap: 13px 28px; padding: 20px; border-left: 0; border-top: 1px solid var(--wb-line); }
  #disk-workbench .wb-inspector-heading { display: none; }
  #disk-workbench .wb-selection-top { grid-column: 1; grid-row: 1; }
  #disk-workbench .wb-selection-metric { grid-column: 2; grid-row: 1 / span 2; padding: 0; text-align: right; }
  #disk-workbench .wb-selection-size { font-size: 28px; }
  #disk-workbench .wb-selection-track { max-width: 160px; margin-left: auto; }
  #disk-workbench .wb-location { grid-column: 1; grid-row: 2; padding: 0; }
  #disk-workbench .wb-location-label { display: none; }
  #disk-workbench .wb-actions { grid-column: 1; grid-row: 3; padding: 0; border: 0; align-self: start; }
  #disk-workbench .wb-primary-actions { grid-template-columns: minmax(0, 1fr) minmax(68px, .7fr); }
  #disk-workbench .wb-breakdown { grid-column: 2; grid-row: 3; padding: 0; }
  #disk-workbench .wb-inspector[data-wb-selection-mode="multiple"] .wb-breakdown { grid-column: 1 / -1; }
  #disk-workbench .wb-inspector[data-wb-selection-mode="multiple"] .wb-breakdown-list { grid-template-columns: repeat(2, minmax(0, 1fr)); column-gap: 20px; }
}
@container mac-tree (max-width: 480px) {
  #disk-workbench .wb-pathbar { padding: 5px 7px; }
  #disk-workbench .wb-path-size { display: none; }
  #disk-workbench .wb-directory { padding-left: 5px; padding-right: 5px; }
  #disk-workbench .wb-share-col, #disk-workbench .wb-share-cell, #disk-workbench .wb-share-head, #disk-workbench .wb-open-col, #disk-workbench .wb-open-cell, #disk-workbench .wb-open-head { display: none; }
  #disk-workbench .wb-table col.wb-size-col { width: 78px; }
  #disk-workbench .wb-table col.wb-check-col { width: 27px; }
  #disk-workbench .wb-table th, #disk-workbench .wb-table td { padding-left: 5px; padding-right: 5px; }
  #disk-workbench .wb-node { --wb-step: 12px; }
  #disk-workbench .wb-folder-select { gap: 4px; }
  #disk-workbench .wb-folder-icon { width: 16px; flex-basis: 16px; }
  #disk-workbench .wb-folder-icon svg { width: 16px; height: 16px; }
  #disk-workbench .wb-inspector { grid-template-columns: minmax(0, 1fr) 110px; gap: 13px 16px; padding: 18px 14px; }
  #disk-workbench .wb-selection-size { font-size: 26px; }
  #disk-workbench .wb-selection-meta { max-width: 110px; margin-left: auto; }
  #disk-workbench .wb-selection-metric { grid-row: 1; }
  #disk-workbench .wb-location { grid-column: 1 / -1; }
  #disk-workbench .wb-actions { grid-column: 1 / -1; }
  #disk-workbench .wb-secondary-actions { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  #disk-workbench .wb-breakdown { grid-column: 1 / -1; grid-row: 4; padding-top: 12px; border-top: 1px solid var(--wb-line); }
  #disk-workbench .wb-inspector[data-wb-selection-mode="multiple"] .wb-breakdown { grid-row: 3; }
  #disk-workbench .wb-breakdown-list { grid-template-columns: repeat(2, minmax(0, 1fr)); column-gap: 16px; }
  #disk-workbench .wb-status-right { display: none; }
}
@media (pointer: coarse) {
  #disk-workbench .wb-button, #disk-workbench .wb-actions .wb-button, #disk-workbench .wb-actions .wb-primary-actions .wb-button, #disk-workbench .wb-enter, #disk-workbench .wb-check-hit, #disk-workbench .wb-breadcrumbs button, #disk-workbench .wb-breakdown-item, #disk-workbench .wb-breakdown-heading button { min-height: 44px; }
  #disk-workbench .wb-enter, #disk-workbench .wb-icon-button { width: 44px; min-width: 44px; }
  #disk-workbench .wb-table col.wb-open-col, #disk-workbench .wb-table col.wb-check-col { width: 44px; }
  #disk-workbench .wb-chevron, #disk-workbench .wb-chevron-space { width: 30px; flex-basis: 30px; min-height: 44px; }
  #disk-workbench .wb-folder-select, #disk-workbench .wb-node { --wb-chevron-mid: 15px; min-height: 44px; }
}
@media (prefers-reduced-motion: reduce) { #disk-workbench button { transition: none; } #disk-workbench button:active:not(:disabled) { transform: none; } }
</style>
<style>
#disk-lens{--dl-bg:light-dark(#ffffff,#252528);--dl-panel:light-dark(#f8f8fa,#2b2b2f);--dl-soft:light-dark(#f0eff5,#36343e);--dl-text:light-dark(#242427,#f0f0f2);--dl-muted:light-dark(#78787e,#a6a6af);--dl-line:light-dark(#e7e6e9,#3a3a40);--dl-active:light-dark(#007aff,#479bff);--dl-active-text:#ffffff;--dl-teal:light-dark(#8b7fdb,#a497e9);--dl-amber:light-dark(#59aec2,#79b7c5);--dl-blue:light-dark(#df9689,#d79d94);--dl-purple:light-dark(#ddb077,#caae86);--dl-clay:light-dark(#a4b879,#adbd8b);--dl-gray:light-dark(#a9aab3,#989aa7);--dl-danger:light-dark(#c35453,#ed8b88);position:relative;container-name:sflens;container-type:inline-size;background:var(--dl-bg);color:var(--dl-text);font:13px/1.45 -apple-system,BlinkMacSystemFont,"SF Pro Text","Segoe UI","PingFang SC","Microsoft YaHei",sans-serif}
#disk-lens *{box-sizing:border-box}#disk-lens [hidden]{display:none!important}#disk-lens button{font:inherit;color:inherit;border:0;background:none;touch-action:manipulation;transition:background .14s ease,transform .12s ease}#disk-lens button:disabled{opacity:.38}#disk-lens button:active:not(:disabled){filter:brightness(.94)}#disk-lens button i,#disk-lens button svg{width:15px;height:15px;flex:none}
#disk-lens .dl-shell{min-height:584px;display:flex;flex-direction:column;position:relative;background:var(--dl-bg)}#disk-lens .dl-breadcrumbs{display:flex;align-items:center;gap:3px;min-height:40px;padding:5px 16px 5px 10px;flex-wrap:wrap;font-size:12px;color:var(--dl-muted)}#disk-lens .dl-crumb{padding:4px 5px;border-radius:5px;max-width:160px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}#disk-lens .dl-crumb:hover{background:var(--dl-soft)}#disk-lens .dl-crumb:last-of-type{color:var(--dl-text);font-weight:600}#disk-lens .dl-breadcrumbs>svg{width:11px;height:11px;opacity:.55}#disk-lens .dl-back{display:grid;place-items:center;min-width:25px;padding:4px;margin-right:2px;border-radius:5px}#disk-lens .dl-back:hover{background:var(--dl-soft)}#disk-lens .dl-scope-size{margin-left:auto;font-variant-numeric:tabular-nums;white-space:nowrap;font-size:12px}
#disk-lens .dl-workspace{display:grid;grid-template-columns:minmax(0,1fr) 230px;gap:24px;padding:18px 20px 25px;flex:1;min-height:482px;align-items:center}#disk-lens .dl-radial-column{min-width:0;display:flex;flex-direction:column;align-items:center}#disk-lens .dl-chart-stage{position:relative;width:100%;max-width:430px;aspect-ratio:1;margin:0 auto;flex:none}#disk-lens .dl-chart{display:block;width:100%;height:100%;overflow:visible}#disk-lens .dl-chart path{transition:opacity .16s ease;transform-box:fill-box;transform-origin:center}#disk-lens .dl-chart path:hover{opacity:1!important}#disk-lens .dl-chart-label{font-size:12px;font-weight:600;fill:light-dark(#302c43,#25202e);pointer-events:none;text-anchor:middle;dominant-baseline:middle;letter-spacing:-.2px}#disk-lens .dl-center{position:absolute;left:50%;top:50%;transform:translate(-50%,-50%);width:38%;display:flex;flex-direction:column;align-items:center;text-align:center;gap:5px;pointer-events:none}#disk-lens .dl-center-name{font-size:12px;color:var(--dl-muted);max-width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}#disk-lens .dl-center-size{font-size:38px;line-height:1.15;letter-spacing:-1.5px;font-weight:400;font-variant-numeric:tabular-nums;white-space:nowrap}#disk-lens .dl-center-size span{font-size:12px;letter-spacing:0;margin-left:5px;font-weight:400;color:var(--dl-muted)}#disk-lens .dl-center-share{font-size:11px;color:var(--dl-muted);white-space:nowrap}#disk-lens .dl-center-enter{pointer-events:auto;margin-top:7px;display:flex;align-items:center;gap:3px;border-radius:5px;background:transparent;padding:4px 6px;font-size:11px;color:var(--dl-active);white-space:nowrap}#disk-lens .dl-center-enter:hover{background:var(--dl-soft)}#disk-lens .dl-center-enter svg{width:11px;height:11px}#disk-lens .dl-chart-caption{display:flex;align-items:center;justify-content:center;gap:15px;font-size:11px;color:var(--dl-muted);padding-top:11px;flex-wrap:wrap}#disk-lens .dl-chart-caption>span{display:inline-flex;align-items:center;gap:5px}#disk-lens .dl-ring-dot{width:7px;height:7px;border-radius:50%;background:light-dark(#a5a3b0,#aaa7b6)}#disk-lens .dl-ring-dot.dl-light{opacity:.42}
#disk-lens .dl-directory-column{min-width:0;align-self:stretch;display:flex;flex-direction:column;justify-content:center;padding:7px 0 3px}#disk-lens .dl-list-heading{display:flex;align-items:center;justify-content:space-between;gap:8px;margin:0 8px 10px;font-size:11px;color:var(--dl-muted)}#disk-lens .dl-list-heading>span{font-weight:600}#disk-lens .dl-list-heading small{font-size:11px;font-weight:400;color:var(--dl-muted)}#disk-lens .dl-folders{display:flex;flex-direction:column;gap:3px;margin:0;padding:0;list-style:none}#disk-lens .dl-folder-row{display:flex;min-width:0;align-items:center;border-radius:7px;background:transparent;position:relative;transition:background .14s ease}#disk-lens .dl-folder-row.dl-selected{background:var(--dl-soft)}#disk-lens .dl-folder-select{display:grid;grid-template-columns:7px minmax(0,1fr) auto;align-items:center;gap:9px;flex:1;min-width:0;padding:10px 3px 10px 10px;text-align:left;border:0;background:transparent;border-radius:7px}#disk-lens .dl-folder-select:hover{background:var(--dl-soft)}#disk-lens .dl-swatch{width:6px;height:24px;border-radius:3px;background:var(--dl-item-color)}#disk-lens .dl-folder-info{min-width:0}#disk-lens .dl-folder-name{display:block;font-size:13px;font-weight:500;letter-spacing:-.1px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}#disk-lens .dl-folder-meta{display:block;font-size:11px;color:var(--dl-muted);margin-top:2px}#disk-lens .dl-folder-size{font-size:12px;font-weight:500;white-space:nowrap;text-align:right;font-variant-numeric:tabular-nums}#disk-lens .dl-folder-size small{display:block;font-size:11px;font-weight:400;color:var(--dl-muted);margin-top:2px}#disk-lens .dl-folder-enter{display:grid;place-items:center;min-width:23px;min-height:34px;padding:3px;border-radius:5px;background:transparent;color:var(--dl-muted);margin-right:3px}#disk-lens .dl-folder-enter:hover{background:var(--dl-panel);color:var(--dl-text)}#disk-lens .dl-folder-enter svg{width:12px;height:12px}#disk-lens .dl-list-footer{font-size:11px;line-height:1.6;color:var(--dl-muted);margin:18px 9px 0;padding-top:13px;border-top:1px solid var(--dl-line);display:flex;justify-content:space-between;gap:8px}#disk-lens .dl-button{display:inline-flex;align-items:center;gap:5px;padding:4px 6px;border-radius:5px;font-size:11px;color:var(--dl-muted);white-space:nowrap}#disk-lens .dl-button:hover{background:var(--dl-soft);color:var(--dl-text)}#disk-lens .dl-button svg{width:12px;height:12px}
#disk-lens .dl-actionbar{border-top:1px solid var(--dl-line);background:var(--dl-panel);display:flex;align-items:center;justify-content:space-between;gap:12px;padding:10px 16px;min-height:62px}#disk-lens .dl-selected-detail{display:flex;align-items:center;gap:9px;min-width:0;flex:1}#disk-lens .dl-selection-icon{display:grid;place-items:center;color:light-dark(#68addb,#80b6dc);flex:none}#disk-lens .dl-selection-icon svg{width:27px;height:27px;stroke-width:1.3;fill:light-dark(#dceffb,#344e60)}#disk-lens .dl-selection-copy{min-width:0}#disk-lens .dl-selection-title{font-size:13px;font-weight:500;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:240px}#disk-lens .dl-selection-path{font-size:11px;color:var(--dl-muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:250px;margin-top:3px}#disk-lens .dl-actions{display:flex;align-items:center;gap:3px;flex-wrap:wrap;justify-content:flex-end}#disk-lens .dl-action{display:flex;align-items:center;justify-content:center;gap:5px;border-radius:5px;padding:6px 7px;min-height:29px;font-size:13px;white-space:nowrap}#disk-lens .dl-action svg{width:14px;height:14px;color:var(--dl-muted)}#disk-lens .dl-action:hover{background:var(--dl-soft)}#disk-lens .dl-action:active:not(:disabled){transform:scale(.97)}#disk-lens .dl-action.dl-danger,#disk-lens .dl-action.dl-danger svg{color:var(--dl-danger)}#disk-lens .dl-action.dl-icon-action{padding:7px}#disk-lens .dl-separator{height:15px;width:1px;background:var(--dl-line);margin:0 3px}#disk-lens .dl-live{position:absolute;width:1px;height:1px;margin:-1px;padding:0;overflow:hidden;clip-path:inset(50%);white-space:nowrap}
@container sflens (max-width:740px){#disk-lens .dl-workspace{gap:15px;grid-template-columns:minmax(0,1fr) 205px;padding:16px 14px 24px;min-height:474px}#disk-lens .dl-center-size{font-size:33px}#disk-lens .dl-actionbar{padding-left:13px;padding-right:13px;flex-wrap:wrap}#disk-lens .dl-selection-title{max-width:225px}#disk-lens .dl-selection-path{max-width:230px}#disk-lens .dl-actions{margin-left:auto}#disk-lens .dl-center{width:42%}#disk-lens .dl-center-enter{margin-top:4px}#disk-lens .dl-folder-select{padding-left:8px;gap:7px}}
@container sflens (max-width:570px){#disk-lens .dl-workspace{display:flex;flex-direction:column;gap:23px;padding:8px 16px 22px;min-height:0}#disk-lens .dl-radial-column{width:100%}#disk-lens .dl-chart-stage{max-width:390px}#disk-lens .dl-directory-column{width:100%;padding:0}#disk-lens .dl-list-heading{margin-bottom:7px}#disk-lens .dl-folder-select{padding:9px 8px 9px 11px}#disk-lens .dl-folder-enter{min-width:29px}#disk-lens .dl-list-footer{margin-top:13px}#disk-lens .dl-actionbar{gap:12px}#disk-lens .dl-selected-detail{width:100%;flex-basis:100%}#disk-lens .dl-actions{width:100%;margin:0;justify-content:space-between;gap:2px}#disk-lens .dl-action{padding-left:5px;padding-right:5px}#disk-lens .dl-separator{display:none}#disk-lens .dl-scope-size{display:none}#disk-lens .dl-center-size{font-size:34px}#disk-lens .dl-center-enter{margin-top:5px}}
@container sflens (max-width:350px){#disk-lens .dl-center-name{font-size:11px}#disk-lens .dl-center-size{font-size:30px}#disk-lens .dl-center{gap:3px}#disk-lens .dl-center-enter{margin-top:3px}#disk-lens .dl-chart-caption{gap:11px}#disk-lens .dl-action{padding:6px 4px}}
@media(pointer:coarse){#disk-lens .dl-action,#disk-lens .dl-crumb,#disk-lens .dl-back,#disk-lens .dl-folder-enter,#disk-lens .dl-center-enter,#disk-lens .dl-button{min-height:44px}#disk-lens .dl-folder-enter{min-width:34px}#disk-lens .dl-folder-select{min-height:52px}#disk-lens .dl-center{gap:2px}}
@media(prefers-reduced-motion:reduce){#disk-lens *{transition:none!important;animation:none!important}}
#disk-lens{--dl-bg:var(--sf-panel,light-dark(#ffffff,#252528));--dl-panel:light-dark(#faf9fc,#29282e);--dl-text:var(--sf-ink,light-dark(#24242b,#f0f0f3));--dl-muted:var(--sf-muted,light-dark(#72717c,#aaa8b4));--dl-line:var(--sf-line,light-dark(#e8e7ee,#3a3943));--dl-active:var(--sf-accent,light-dark(#7654bb,#bf9ff5));--dl-soft:var(--sf-accent-soft,light-dark(#eee9f8,#363040));--dl-teal:var(--sf-ai,light-dark(#9181c7,#ab9bdf));--dl-amber:var(--sf-games,light-dark(#67a9b8,#85bdca));--dl-blue:var(--sf-media,light-dark(#ce958a,#d8aba2));--dl-purple:var(--sf-projects,light-dark(#c6aa7b,#cdb68f));--dl-clay:var(--sf-downloads,light-dark(#98ad77,#acbf8e));--dl-gray:var(--sf-others,light-dark(#a1a2ad,#a6a7b3))}
#disk-lens .dl-shell{min-height:596px}
#disk-lens .dl-breadcrumbs{min-height:51px;padding:10px 19px 9px 12px;font-size:13px}
#disk-lens .dl-crumb{padding:5px 6px;max-width:176px;font-weight:400}
#disk-lens .dl-crumb:last-of-type{font-weight:500}
#disk-lens .dl-back{min-width:28px;padding:5px;margin-right:4px}
#disk-lens .dl-workspace{grid-template-columns:minmax(0,1fr) 220px;gap:31px;padding:8px 23px 26px 24px;min-height:483px}
#disk-lens .dl-chart-stage{max-width:440px}
#disk-lens .dl-chart path:hover{opacity:var(--dl-hover-opacity,1)!important}
#disk-lens .dl-chart-label{font-size:13px;font-weight:500;fill:var(--dl-text);letter-spacing:0;transition:opacity .15s ease}
#disk-lens .dl-center{width:44%;aspect-ratio:1;border:0;border-radius:50%;display:flex;align-items:center;justify-content:center;gap:6px;padding:7px;pointer-events:auto;color:var(--dl-text);background:transparent}
#disk-lens .dl-center:disabled{opacity:1;filter:none}
#disk-lens .dl-center:hover:not(:disabled){background:color-mix(in srgb,var(--dl-active) 4%,var(--dl-bg))}
#disk-lens .dl-center-name{font-size:13px;color:var(--dl-muted);line-height:1.4;max-width:100%;font-weight:400}
#disk-lens .dl-center-size{font-size:39px;line-height:1.15;letter-spacing:-1.2px;font-weight:400}
#disk-lens .dl-center-size span{font-size:12px;margin-left:5px;letter-spacing:0}
#disk-lens .dl-center-share{font-size:11px;color:var(--dl-muted)}
#disk-lens .dl-center-back{display:flex;align-items:center;gap:3px;font-size:11px;color:var(--dl-active);margin-top:5px;white-space:nowrap}
#disk-lens .dl-center-back svg{width:12px;height:12px}
#disk-lens .dl-chart-caption{font-size:11px;padding-top:10px;gap:17px}
#disk-lens .dl-directory-column{padding:8px 0;justify-content:center;align-self:stretch}
#disk-lens .dl-list-heading{margin:0 6px 12px;font-size:12px}
#disk-lens .dl-list-heading>span{font-weight:400}
#disk-lens .dl-folders{gap:5px}
#disk-lens .dl-folder-row{display:block;border-radius:6px;background:transparent}
#disk-lens .dl-folder-row.dl-selected{background:transparent}
#disk-lens .dl-row-main{display:flex;align-items:center;border-radius:6px;min-width:0;background:transparent;transition:background .14s ease}
#disk-lens .dl-folder-row.dl-selected>.dl-row-main{background:var(--dl-soft)}
#disk-lens .dl-folder-row.dl-hovered>.dl-row-main{background:color-mix(in srgb,var(--dl-active) 7%,var(--dl-bg))}
#disk-lens .dl-folder-select{padding:10px 4px 10px 8px;gap:8px;border-radius:6px;min-height:53px}
#disk-lens .dl-folder-name{font-size:13px;font-weight:500;letter-spacing:0}
#disk-lens .dl-folder-meta{font-size:11px;margin-top:3px}
#disk-lens .dl-folder-size{font-size:13px;font-weight:400}
#disk-lens .dl-folder-size small{font-size:11px;margin-top:3px}
#disk-lens .dl-swatch{width:6px;height:25px;border-radius:3px;opacity:.82}
#disk-lens .dl-folder-enter{min-width:22px;min-height:35px;margin-right:4px}
#disk-lens .dl-subfolders{list-style:none;margin:4px 0 5px 12px;padding:0 0 0 10px;border-left:1px solid var(--dl-line);display:flex;flex-direction:column;gap:1px}
#disk-lens .dl-subrow{display:flex;align-items:center;min-width:0;border-radius:5px}
#disk-lens .dl-subrow.dl-selected{background:var(--dl-soft)}
#disk-lens .dl-subrow.dl-hovered{background:color-mix(in srgb,var(--dl-active) 7%,var(--dl-bg))}
#disk-lens .dl-sub-select{display:flex;align-items:center;justify-content:space-between;gap:9px;min-width:0;flex:1;text-align:left;min-height:29px;padding:6px 5px;border-radius:5px}
#disk-lens .dl-sub-select:hover{background:var(--dl-soft)}
#disk-lens .dl-sub-name{font-size:12px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0;flex:1}
#disk-lens .dl-sub-size{font-size:11px;white-space:nowrap;font-variant-numeric:tabular-nums;color:var(--dl-muted)}
#disk-lens .dl-subrow .dl-folder-enter{min-width:21px;min-height:29px;margin-right:1px}
#disk-lens .dl-actionbar{background:var(--dl-panel);padding:12px 19px;min-height:64px;gap:16px}
#disk-lens .dl-selection-icon{color:var(--sf-accent-folder,var(--dl-active))}
#disk-lens .dl-selection-icon svg{width:25px;height:25px;fill:var(--sf-accent-folder-fill,var(--dl-soft));stroke-width:1.45}
#disk-lens .dl-selection-title{font-size:13px;max-width:255px}
#disk-lens .dl-selection-path{font-size:11px;max-width:255px}
#disk-lens .dl-actions{gap:4px}
#disk-lens .dl-action{font-size:12px;gap:5px;min-height:31px;padding:7px 6px}
#disk-lens .dl-action svg{width:14px;height:14px}
#disk-lens .dl-actionbar .dl-action:first-child{background:var(--dl-soft);color:var(--dl-active);padding-inline:9px}
#disk-lens .dl-actionbar .dl-action:first-child svg{color:var(--dl-active)}
@container sflens (max-width:790px){#disk-lens .dl-workspace{grid-template-columns:minmax(0,1fr) 210px;gap:19px;padding:11px 17px 23px 17px}#disk-lens .dl-center-size{font-size:34px}#disk-lens .dl-center{width:44%;gap:5px;padding:4px}#disk-lens .dl-center-name{font-size:13px}#disk-lens .dl-folder-select{padding-left:7px;gap:7px}#disk-lens .dl-actionbar{flex-wrap:wrap;padding:12px 16px}#disk-lens .dl-actions{margin-left:auto}}
@container sflens (max-width:640px){#disk-lens .dl-workspace{display:flex;flex-direction:column;gap:26px;padding:5px 18px 24px;min-height:0}#disk-lens .dl-radial-column{width:100%}#disk-lens .dl-chart-stage{max-width:400px}#disk-lens .dl-directory-column{width:100%;padding:0;align-self:stretch}#disk-lens .dl-list-heading{margin:0 7px 9px}#disk-lens .dl-folder-select{padding:10px 7px 10px 10px}#disk-lens .dl-subfolders{margin-left:14px;padding-left:12px}#disk-lens .dl-sub-select{padding:8px;min-height:34px}#disk-lens .dl-sub-name{font-size:13px}#disk-lens .dl-sub-size{font-size:12px}#disk-lens .dl-folder-enter{min-width:30px}#disk-lens .dl-actionbar{gap:12px;padding:13px 16px}#disk-lens .dl-selected-detail{width:100%;flex-basis:100%}#disk-lens .dl-actions{width:100%;margin:0;justify-content:space-between;gap:3px}#disk-lens .dl-selection-title,#disk-lens .dl-selection-path{max-width:100%}#disk-lens .dl-separator{display:none}#disk-lens .dl-scope-size{display:none}#disk-lens .dl-center-size{font-size:37px}#disk-lens .dl-center{gap:5px}#disk-lens .dl-center-back{margin-top:4px}}
@container sflens (max-width:360px){#disk-lens .dl-workspace{padding:5px 13px 22px;gap:23px}#disk-lens .dl-center-size{font-size:31px}#disk-lens .dl-center{gap:4px}#disk-lens .dl-center-name{font-size:12px}#disk-lens .dl-center-share,#disk-lens .dl-center-back{font-size:11px}#disk-lens .dl-center-back{margin-top:2px}#disk-lens .dl-actionbar{padding:12px 13px}#disk-lens .dl-actions{gap:3px;justify-content:flex-start}#disk-lens .dl-action{padding:7px 5px}#disk-lens .dl-actionbar .dl-action:first-child{padding-inline:8px}#disk-lens .dl-crumb{max-width:140px}}
@media(pointer:coarse){#disk-lens .dl-sub-select,#disk-lens .dl-action,#disk-lens .dl-crumb,#disk-lens .dl-back,#disk-lens .dl-folder-enter{min-height:44px}#disk-lens .dl-folder-enter{min-width:36px}#disk-lens .dl-center{min-height:44px}}
</style>
<style>
#spacefold-mac .disk-dialog-layer,#spacefold-mac .disk-action-toast{--da-panel:light-dark(#f9f8fb,#303035);--da-ink:var(--sf-ink);--da-muted:var(--sf-muted);--da-line:var(--sf-line);--da-soft:light-dark(#eeedf2,#242429);--da-main:light-dark(#007aff,#2488f5);--da-onmain:light-dark(#ffffff,#ffffff);--da-success:light-dark(#318658,#9aceaa);--da-danger:light-dark(#d94e46,#c94740);font-family:-apple-system,BlinkMacSystemFont,"SF Pro Text","Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;font-size:13px}
#spacefold-mac .disk-dialog-layer{padding:64px 20px 24px;background:light-dark(#17112320,#00000059);z-index:85;align-items:flex-start;backdrop-filter:blur(1px)}
#spacefold-mac .disk-dialog-layer .disk-dialog{max-height:min(580px,100%);border:1px solid light-dark(#d8d5df,#60606b);border-radius:15px;background:var(--da-panel);box-shadow:0 22px 65px light-dark(#29203538,#00000077),0 2px 7px light-dark(#27203319,#00000033),inset 0 1px 0 light-dark(#ffffffed,#ffffff1c);transform-origin:center top}
#spacefold-mac .disk-dialog-layer .da-head{padding:19px 22px 12px;border-bottom:0}
#spacefold-mac .disk-dialog-layer .da-title{font-size:16px;font-weight:600;letter-spacing:-.22px;line-height:1.3}
#spacefold-mac .disk-dialog-layer .da-demo{background:transparent;padding:0;font-size:11px;font-weight:400}
#spacefold-mac .disk-dialog-layer .da-body{padding:10px 22px 18px}
#spacefold-mac .disk-dialog-layer .da-footer{padding:13px 22px 16px;background:transparent;border-top:1px solid var(--da-line);gap:9px}
#spacefold-mac .disk-dialog-layer .disk-dialog button{min-height:29px;padding:5px 11px;border-radius:6px;font-size:13px;border:1px solid light-dark(#dedbe4,#585860);background:light-dark(#ffffff,#4a4a52);box-shadow:0 1px 2px light-dark(#211a2c0f,#00000023)}
#spacefold-mac .disk-dialog-layer .disk-dialog button:hover{background:light-dark(#f2f1f5,#55555f)}
#spacefold-mac .disk-dialog-layer .disk-dialog button.da-primary{background:linear-gradient(light-dark(#2991ff,#308ffa),light-dark(#0078f7,#0874e7));border-color:light-dark(#1680ed,#2588f5);color:var(--da-onmain);box-shadow:inset 0 1px 0 #ffffff24,0 1px 2px #0000000f}
#spacefold-mac .disk-dialog-layer .disk-dialog button.da-primary:hover{filter:brightness(1.04)}
#spacefold-mac .disk-dialog-layer .disk-dialog button.da-danger{background:var(--da-danger);color:light-dark(#fff,#fff);border-color:transparent}
#spacefold-mac .disk-dialog-layer .disk-dialog button.da-close{width:25px;min-height:25px;padding:4px;border:0;border-radius:50%;background:light-dark(#e8e6ed,#45454e);box-shadow:none;color:var(--da-muted)}
#spacefold-mac .disk-dialog-layer .disk-dialog button.da-close svg{width:12px;height:12px;stroke-width:2}
#spacefold-mac .disk-dialog-layer .da-formats{display:flex;padding:3px;gap:2px;border-radius:8px;background:light-dark(#e7e5ec,#242429);margin-bottom:19px}
#spacefold-mac .disk-dialog-layer .disk-dialog .da-format{flex:1;border:0;background:transparent;box-shadow:none;min-height:38px;border-radius:5px;padding:4px 7px;line-height:1.3;gap:1px;color:var(--da-muted)}
#spacefold-mac .disk-dialog-layer .disk-dialog .da-format small{font-size:11px}
#spacefold-mac .disk-dialog-layer .disk-dialog .da-format[aria-pressed="true"]{background:light-dark(#ffffff,#595962);color:var(--da-ink);box-shadow:0 1px 4px light-dark(#20182c29,#0000003a)}
#spacefold-mac .disk-dialog-layer .disk-dialog .da-format[aria-pressed="true"] small{color:var(--da-muted);opacity:1}
#spacefold-mac .disk-dialog-layer .da-field{gap:6px;margin-bottom:15px;font-size:12px;font-weight:500}
#spacefold-mac .disk-dialog-layer .disk-dialog input[type="text"],#spacefold-mac .disk-dialog-layer .disk-dialog select,#spacefold-mac .disk-dialog-layer .disk-dialog textarea{background:light-dark(#ffffff,#212125);color:var(--da-ink);border:1px solid light-dark(#d4d1db,#54545e);border-radius:5px;min-height:31px;padding:6px 8px;font:400 13px/1.4 -apple-system,BlinkMacSystemFont,"SF Pro Text","Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;box-shadow:inset 0 1px 2px light-dark(#1e173309,#0000001a)}
#spacefold-mac .disk-dialog-layer .da-field-title{font-size:12px;font-weight:500}
#spacefold-mac .disk-dialog-layer .da-checks{gap:6px 14px;margin-bottom:15px}
#spacefold-mac .disk-dialog-layer .da-check{font-size:12px;min-height:22px;font-weight:400}
#spacefold-mac .disk-dialog-layer .da-preview{border:1px solid light-dark(#dad7e1,#53535d);border-radius:6px;background:light-dark(#ffffff,#252529)}
#spacefold-mac .disk-dialog-layer .da-preview-head{padding:7px 10px;background:light-dark(#efedf3,#393941);border-bottom:1px solid var(--da-line)}
#spacefold-mac .disk-dialog-layer .da-preview-body{max-height:118px;min-height:102px;padding:9px 10px}
#spacefold-mac .disk-dialog-layer .da-preview-body pre{font-family:"SFMono-Regular",Consolas,"Liberation Mono",monospace;font-size:11px;line-height:1.6}
#spacefold-mac .disk-dialog-layer .da-preview-body td,#spacefold-mac .disk-dialog-layer .da-preview-body th{font-size:11px;line-height:1.5;padding:5px}
#spacefold-mac .disk-dialog-layer .da-note{font-size:12px;line-height:1.6}
#spacefold-mac .disk-dialog-layer .da-export-result{font-size:11px;min-height:16px;margin-top:6px}
#spacefold-mac .disk-dialog-layer .da-object{background:transparent;padding:3px 0 13px;gap:12px;border-radius:0;border-bottom:1px solid var(--da-line);margin-bottom:17px}
#spacefold-mac .disk-dialog-layer .da-object>svg{width:37px;height:37px;color:light-dark(#259bd9,#52b8ee);fill:light-dark(#a8dcf9,#306e91);stroke-width:1.1}
#spacefold-mac .disk-dialog-layer .da-path{font-size:12px}
#spacefold-mac .disk-dialog-layer .da-success-icon{background:light-dark(#e7f1ea,#2b4033);border-radius:50%}
#spacefold-mac .disk-dialog-layer .da-menu{grid-template-columns:1fr;gap:2px}
#spacefold-mac .disk-dialog-layer .disk-dialog .da-menu button{border:0;box-shadow:none;background:transparent;padding:7px 8px}
#spacefold-mac .disk-dialog-layer .disk-dialog .da-menu button:hover{background:var(--da-soft)}
#spacefold-mac .disk-action-toast{border:1px solid light-dark(#d9d6df,#575761);background:light-dark(#f8f7fa,#333339);border-radius:9px;top:46px;font-size:12px;padding:9px 13px;box-shadow:0 8px 28px light-dark(#201a2925,#00000044)}
#spacefold-mac #disk-workbench [data-category="AI Models"] .wb-share-track span{background:var(--sf-ai)}
#spacefold-mac #disk-workbench [data-category="Games"] .wb-share-track span{background:var(--sf-games)}
#spacefold-mac #disk-workbench [data-category="Media"] .wb-share-track span{background:var(--sf-media)}
#spacefold-mac #disk-workbench [data-category="Projects"] .wb-share-track span{background:var(--sf-projects)}
#spacefold-mac #disk-workbench [data-category="Downloads"] .wb-share-track span{background:var(--sf-downloads)}
#spacefold-mac #disk-workbench [data-category="Others"] .wb-share-track span{background:var(--sf-others)}
@container(max-width:530px){#spacefold-mac .disk-dialog-layer{padding:53px 9px 18px}#spacefold-mac .disk-dialog-layer .da-head{padding:17px 16px 10px}#spacefold-mac .disk-dialog-layer .da-title-wrap{flex-wrap:wrap;gap:5px 9px}#spacefold-mac .disk-dialog-layer .da-body{padding:9px 16px 16px}#spacefold-mac .disk-dialog-layer .da-footer{padding:12px 16px 14px;flex-wrap:wrap}#spacefold-mac .disk-dialog-layer .da-footer-note{width:100%;margin:0}#spacefold-mac .disk-dialog-layer .da-checks{gap:5px 10px}#spacefold-mac .disk-dialog-layer .disk-dialog{max-height:min(620px,calc(100% - 12px))}}
@media(prefers-reduced-transparency:reduce){#spacefold-mac .disk-dialog-layer{backdrop-filter:none}}
@media(prefers-contrast:more){#spacefold-mac .disk-dialog-layer .disk-dialog{border:1px solid var(--da-ink)}}
@media(pointer:coarse){#spacefold-mac .disk-dialog-layer .disk-dialog button{min-height:44px}#spacefold-mac .disk-dialog-layer .disk-dialog button.da-close{min-height:36px;width:36px}#spacefold-mac .disk-dialog-layer .disk-dialog input[type="text"],#spacefold-mac .disk-dialog-layer .disk-dialog select,#spacefold-mac .disk-dialog-layer .disk-dialog textarea{font-size:16px}#spacefold-mac .disk-dialog-layer .da-check{min-height:34px}}
</style>

<style>
#spacefold-mac{--sf-ink:light-dark(#24242b,#f0f0f3);--sf-muted:light-dark(#797983,#a6a6b2);--sf-panel:light-dark(#ffffff,#252528);--sf-line:light-dark(#e8e7ee,#3a3943);--sf-soft:light-dark(#f3f2f7,#33313c);--sf-blue:light-dark(#7165aa,#b2a5ed);--sf-button:light-dark(#ffffff,#44424d);--sf-ai:light-dark(#8b7fdb,#a499ed);--sf-games:light-dark(#59aec2,#76c1d2);--sf-media:light-dark(#df9689,#e7ac9f);--sf-projects:light-dark(#ddb077,#e5c08f);--sf-downloads:light-dark(#a4b879,#b5c991);--sf-others:light-dark(#a9aab3,#a5a6b1);position:relative;isolation:isolate;width:100%;color:var(--sf-ink);background:var(--sf-panel);font:13px/1.45 -apple-system,BlinkMacSystemFont,"Segoe UI Variable Text","Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;font-optical-sizing:auto;-webkit-font-smoothing:antialiased;border:1px solid light-dark(#dddae5,#45424e);border-radius:12px;box-shadow:0 14px 44px light-dark(#24203410,#00000025);overflow:hidden;container-type:inline-size;container-name:spacefold}
#spacefold-mac *{box-sizing:border-box}
#spacefold-mac [hidden]{display:none!important}
#spacefold-mac button,#spacefold-mac input,#spacefold-mac select,#spacefold-mac textarea{font-family:inherit;touch-action:manipulation}
#spacefold-mac button{letter-spacing:0}
#spacefold-mac button:not(:disabled):active{filter:brightness(.94)}
#spacefold-mac .sf-topbar{position:relative;z-index:10;display:flex;align-items:center;gap:22px;min-height:68px;padding:12px 18px;background:light-dark(#faf9fc,#2c2a32);border-bottom:1px solid var(--sf-line)}
#spacefold-mac .sf-brand{display:flex;align-items:center;gap:9px;flex:none;color:var(--sf-ink);font-size:16px;font-weight:600;letter-spacing:-.2px}
#spacefold-mac .sf-brand-symbol{display:grid;place-items:center;width:29px;height:29px;border-radius:8px;background:light-dark(#ebe6f7,#443956);color:light-dark(#7b6baf,#c3b5e8)}
#spacefold-mac .sf-brand-symbol svg{width:18px;height:18px;stroke-width:1.65}
#spacefold-mac .sf-source{display:flex;align-items:center;gap:9px;border:1px solid transparent;border-radius:8px;background:transparent;padding:3px 8px;min-height:41px;flex:none;color:var(--sf-ink);text-align:left;min-width:137px}
#spacefold-mac .sf-source:hover,#spacefold-mac .sf-source[aria-expanded="true"]{background:var(--sf-soft);border-color:var(--sf-line)}
#spacefold-mac .sf-source>svg{width:18px;height:18px;stroke-width:1.65;color:var(--sf-muted)}
#spacefold-mac .sf-source>svg:last-child{width:12px;height:12px;margin-left:6px}
#spacefold-mac .sf-source-copy{display:flex;flex-direction:column;gap:1px;min-width:0}
#spacefold-mac .sf-source-name{font-size:13px;line-height:1.4;font-weight:500;white-space:nowrap}
#spacefold-mac .sf-source-meta{font-size:11px;color:var(--sf-muted);line-height:1.4;white-space:nowrap;font-variant-numeric:tabular-nums}
#spacefold-mac .sf-switch{position:relative;display:flex;align-items:center;gap:0;padding:3px;background:light-dark(#eeecf2,#211f28);border-radius:8px;margin-left:auto;flex:none}
#spacefold-mac .sf-switch-thumb{position:absolute;left:0;top:3px;height:calc(100% - 6px);border-radius:6px;background:light-dark(#ffffff,#514b61);box-shadow:0 1px 4px light-dark(#3021401b,#00000029);pointer-events:none;z-index:0}
#spacefold-mac .sf-tab{position:relative;z-index:1;min-height:31px;padding:6px 11px;border:0;border-radius:6px;background:transparent;color:var(--sf-muted);font-size:13px;line-height:1.4;font-weight:500;display:flex;align-items:center;justify-content:center;gap:6px;white-space:nowrap}
#spacefold-mac .sf-tab[aria-selected="true"]{color:var(--sf-ink)}
#spacefold-mac .sf-tab svg{width:15px;height:15px;stroke-width:1.7}
#spacefold-mac .sf-tab:hover{color:var(--sf-ink)}
#spacefold-mac .sf-header-actions{display:flex;align-items:center;gap:9px;margin-left:auto}
#spacefold-mac .sf-tool{border:0;border-radius:7px;background:transparent;color:var(--sf-muted);display:inline-flex;align-items:center;justify-content:center;gap:6px;padding:6px 8px;min-width:31px;min-height:32px;font-size:13px;line-height:1.4;white-space:nowrap}
#spacefold-mac .sf-tool:hover{background:var(--sf-soft);color:var(--sf-ink)}
#spacefold-mac .sf-tool:disabled{opacity:.38;filter:none}
#spacefold-mac .sf-tool svg{width:16px;height:16px;stroke-width:1.7}
#spacefold-mac .sf-export{background:light-dark(#eee9f7,#463d57);color:light-dark(#65518f,#d7c7f2);padding:7px 11px}
#spacefold-mac .sf-export:hover{background:light-dark(#e6dff2,#534568);color:light-dark(#59437e,#e8dcfa)}
#spacefold-mac .sf-body,#spacefold-mac .sf-content,#spacefold-mac .sf-view{min-width:0;background:var(--sf-panel)}
#spacefold-mac #disk-map,#spacefold-mac #disk-workbench,#spacefold-mac #disk-lens{border:0;border-radius:0;box-shadow:none;font-family:inherit}
#spacefold-mac #disk-map .dm-titlebar,#spacefold-mac #disk-workbench .wb-topbar,#spacefold-mac #disk-lens .dl-topbar{display:none}
#spacefold-mac #disk-map .dm-controls [data-map-action="export"],#spacefold-mac #disk-map .dm-controls [data-map-action="scan"],#spacefold-mac #disk-workbench [data-wb-action="scan"]{display:none}
#spacefold-mac .sf-popover{position:absolute;z-index:65;top:60px;padding:6px;background:light-dark(#ffffff,#302c38);border:1px solid light-dark(#ddd7e8,#534a61);border-radius:11px;box-shadow:0 14px 42px light-dark(#30234022,#00000055),0 2px 7px light-dark(#3023400b,#00000022);transform-origin:20% 0;color:var(--sf-ink)}
#spacefold-mac .sf-action-menu{right:16px;width:min(230px,calc(100% - 20px));transform-origin:90% 0}
#spacefold-mac .sf-menu-title{padding:7px 9px 9px;font-size:11px;font-weight:500;color:var(--sf-muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
#spacefold-mac .sf-menu-item{width:100%;display:flex;align-items:center;gap:9px;text-align:left;background:transparent;border:0;border-radius:6px;color:var(--sf-ink);font-size:13px;line-height:1.4;padding:8px 9px;min-height:33px}
#spacefold-mac .sf-menu-item svg{width:15px;height:15px;stroke-width:1.65;color:var(--sf-muted)}
#spacefold-mac .sf-menu-item:not(:disabled):hover{background:var(--sf-soft);color:var(--sf-ink)}
#spacefold-mac .sf-menu-item:disabled{opacity:.35}
#spacefold-mac .sf-menu-divider{height:1px;background:var(--sf-line);margin:5px 8px}
#spacefold-mac .sf-menu-item[data-sf-file="trash"],#spacefold-mac .sf-menu-item[data-sf-file="trash"] svg{color:light-dark(#c95752,#ef9990)}
#spacefold-mac .sf-source-menu{left:110px;width:min(318px,calc(100% - 20px));padding:16px}
#spacefold-mac .sf-source-heading{margin:0 0 12px;display:flex;align-items:baseline;justify-content:space-between;gap:9px;font-size:13px;font-weight:500}
#spacefold-mac .sf-source-heading small{font-size:11px;font-weight:400;color:var(--sf-muted)}
#spacefold-mac .sf-disk-option{width:100%;display:flex;align-items:center;gap:11px;padding:10px 11px;border:0;border-radius:8px;background:var(--sf-soft);color:var(--sf-ink);text-align:left}
#spacefold-mac .sf-disk-option>svg{width:23px;height:23px;color:var(--sf-blue);stroke-width:1.55}
#spacefold-mac .sf-disk-option .sf-source-copy{flex:1}
#spacefold-mac .sf-disk-option .sf-source-name{font-size:14px}
#spacefold-mac .sf-disk-option .sf-source-meta{margin-top:3px}
#spacefold-mac .sf-disk-option>svg:last-child{width:14px;height:14px;color:var(--sf-muted)}
#spacefold-mac .sf-capacity{padding:13px 2px 15px;border-bottom:1px solid var(--sf-line);margin-bottom:13px}
#spacefold-mac .sf-capacity-label{display:flex;align-items:center;justify-content:space-between;gap:12px;font-size:11px;color:var(--sf-muted);font-variant-numeric:tabular-nums}
#spacefold-mac .sf-capacity-label strong{font-size:12px;color:var(--sf-ink);font-weight:500}
#spacefold-mac .sf-storage-track{display:flex;height:5px;background:light-dark(#e4e1ea,#49414f);border-radius:3px;overflow:hidden;margin:8px 0}
#spacefold-mac .sf-storage-track span{flex:none}
#spacefold-mac .sf-jump-heading{font-size:11px;font-weight:500;color:var(--sf-muted);margin:0 3px 5px}
#spacefold-mac .sf-place{display:flex;align-items:center;gap:10px;width:100%;min-height:34px;padding:7px 9px;border:0;border-radius:6px;background:transparent;color:var(--sf-ink);font-size:13px;line-height:1.4;text-align:left}
#spacefold-mac .sf-place:hover,#spacefold-mac .sf-place[aria-current="location"]{background:var(--sf-soft)}
#spacefold-mac .sf-place-name{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
#spacefold-mac .sf-place small{color:var(--sf-muted);font-size:12px;font-variant-numeric:tabular-nums;white-space:nowrap}
#spacefold-mac .sf-category-dot{width:7px;height:7px;border-radius:2px;flex:none;background:var(--sf-item-color)}
#spacefold-mac .sf-source-footer{margin-top:12px;padding-top:10px;border-top:1px solid var(--sf-line)}
#spacefold-mac .sf-source-footer .sf-menu-item{padding:7px 4px}
#spacefold-mac .sf-source-footer .sf-menu-item svg{color:var(--sf-blue)}
#spacefold-mac .sf-live{position:absolute;width:1px;height:1px;overflow:hidden;clip-path:inset(50%);white-space:nowrap}
@container spacefold (max-width:850px){#spacefold-mac .sf-topbar{gap:15px;padding:11px 14px}#spacefold-mac .sf-tab{padding-inline:9px}#spacefold-mac .sf-header-actions{gap:5px}#spacefold-mac .sf-source{min-width:125px;padding-inline:5px}#spacefold-mac .sf-tool{padding-inline:6px}#spacefold-mac .sf-scan span{display:none}}
@container spacefold (max-width:720px){#spacefold-mac .sf-topbar{display:grid;grid-template-columns:auto minmax(0,1fr) auto;gap:10px 13px;padding:11px 13px}#spacefold-mac .sf-source{justify-self:start;min-width:0}#spacefold-mac .sf-switch{grid-row:2;grid-column:1/-1;margin:0;justify-content:stretch}#spacefold-mac .sf-tab{flex:1;min-height:32px}#spacefold-mac .sf-header-actions{margin:0;gap:6px}#spacefold-mac .sf-export{padding-inline:9px}}
@container spacefold (max-width:420px){#spacefold-mac .sf-topbar{gap:9px 8px;padding:10px}#spacefold-mac .sf-brand{gap:0}#spacefold-mac .sf-brand-name{display:none}#spacefold-mac .sf-brand-symbol{width:27px;height:27px}#spacefold-mac .sf-source{gap:7px;padding-inline:2px}#spacefold-mac .sf-source>svg:first-child{display:none}#spacefold-mac .sf-source>svg:last-child{margin-left:0}#spacefold-mac .sf-header-actions{gap:2px}#spacefold-mac .sf-header-actions .sf-scan{display:none}#spacefold-mac .sf-tab{padding-inline:6px;gap:5px}#spacefold-mac .sf-tool{min-width:29px;padding-inline:6px}#spacefold-mac .sf-source-menu{padding:13px}}
@media(prefers-reduced-motion:reduce){#spacefold-mac button:active{transform:none!important}}
@media(prefers-contrast:more){#spacefold-mac{--sf-muted:light-dark(#55515f,#c6bfd2);--sf-line:light-dark(#b9b2c6,#756980)}#spacefold-mac .sf-source[aria-expanded="true"]{border-color:var(--sf-blue)}}
@media(pointer:coarse){#spacefold-mac .sf-tool,#spacefold-mac .sf-tab,#spacefold-mac .sf-source,#spacefold-mac .sf-place,#spacefold-mac .sf-menu-item{min-height:44px}#spacefold-mac .sf-tool{min-width:40px}#spacefold-mac .sf-header-actions{gap:3px}#spacefold-mac .sf-tab{padding-inline:7px}}
</style>
<style>
#spacefold-mac {
  --sf-accent:light-dark(#7654bb,#bf9ff5);
  --sf-accent-button:light-dark(#7652bb,#7652bb);
  --sf-accent-onbutton:light-dark(#ffffff,#ffffff);
  --sf-accent-soft:color-mix(in srgb,var(--sf-accent) 10%,var(--sf-panel));
  --sf-accent-selected:color-mix(in srgb,var(--sf-accent) 15%,var(--sf-panel));
  --sf-accent-covered:color-mix(in srgb,var(--sf-accent) 4%,var(--sf-panel));
  --sf-accent-folder:color-mix(in srgb,var(--sf-accent) 85%,var(--sf-panel));
  --sf-accent-folder-fill:color-mix(in srgb,var(--sf-accent) 26%,var(--sf-panel));
  --sf-blue:var(--sf-accent);
}
#spacefold-mac[data-sf-accent="blue"]{--sf-accent:light-dark(#226bce,#89b8fa);--sf-accent-button:light-dark(#236ac6,#236ac6)}
#spacefold-mac[data-sf-accent="teal"]{--sf-accent:light-dark(#067f87,#7bcdd2);--sf-accent-button:light-dark(#087d82,#087d82)}
#spacefold-mac[data-sf-accent="amber"]{--sf-accent:light-dark(#a45d18,#e4b477);--sf-accent-button:light-dark(#a0601c,#a0601c)}
#spacefold-mac[data-sf-accent="rose"]{--sf-accent:light-dark(#ad4f80,#e6a2c6);--sf-accent-button:light-dark(#ad4f80,#ad4f80)}
#spacefold-mac .sf-brand-symbol{background:var(--sf-accent-soft);color:var(--sf-accent)}
#spacefold-mac .sf-tab[aria-selected="true"]{color:var(--sf-accent)}
#spacefold-mac .sf-switch-thumb{background:var(--sf-panel)}
#spacefold-mac .sf-export{background:var(--sf-accent-soft);color:var(--sf-accent)}
#spacefold-mac .sf-export:hover{background:var(--sf-accent-selected);color:var(--sf-accent)}
#spacefold-mac .sf-tool[aria-expanded="true"]{background:var(--sf-accent-soft);color:var(--sf-accent)}
#spacefold-mac .sf-source[aria-expanded="true"],#spacefold-mac .sf-place[aria-current="location"]{background:var(--sf-accent-soft)}
#spacefold-mac .sf-menu-item:not([data-sf-file="trash"]):not(:disabled):hover{background:var(--sf-accent-soft)}
#spacefold-mac .sf-appearance-menu{right:15px;width:min(296px,calc(100% - 20px));padding:15px 17px 17px;transform-origin:85% 0}
#spacefold-mac .sf-appearance-heading{display:flex;align-items:center;justify-content:space-between;gap:12px;margin-bottom:18px}
#spacefold-mac .sf-appearance-heading h2{font-size:15px;line-height:1.4;font-weight:600;margin:0;color:var(--sf-ink)}
#spacefold-mac .sf-appearance-close{display:grid;place-items:center;width:25px;min-height:25px;padding:5px;border:0;border-radius:50%;background:var(--sf-soft);color:var(--sf-muted)}
#spacefold-mac .sf-appearance-close:hover{background:var(--sf-line);color:var(--sf-ink)}
#spacefold-mac .sf-appearance-close svg{width:13px;height:13px;stroke-width:1.8}
#spacefold-mac .sf-accent-label{display:flex;justify-content:space-between;align-items:center;gap:12px;margin:0 1px 11px;font-size:13px;font-weight:500}
#spacefold-mac .sf-accent-label [data-sf-accent-name]{color:var(--sf-muted);font-size:12px;font-weight:400}
#spacefold-mac .sf-accent-options{display:grid;grid-template-columns:repeat(5,minmax(0,1fr));gap:5px}
#spacefold-mac .sf-accent-option{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:8px;min-height:64px;padding:7px 2px 6px;border:0;border-radius:7px;background:transparent;color:var(--sf-muted);font-size:12px;line-height:1.4}
#spacefold-mac .sf-accent-option:hover{background:var(--sf-soft);color:var(--sf-ink)}
#spacefold-mac .sf-accent-option[aria-pressed="true"]{background:var(--sf-accent-soft);color:var(--sf-accent)}
#spacefold-mac .sf-accent-dot{display:grid;place-items:center;width:27px;height:27px;border-radius:50%;background:var(--sf-swatch);color:light-dark(#ffffff,#26222f);box-shadow:inset 0 0 0 1px light-dark(#00000006,#ffffff06)}
#spacefold-mac .sf-accent-dot svg{width:14px;height:14px;stroke-width:2.2;opacity:0}
#spacefold-mac .sf-accent-option[aria-pressed="true"] .sf-accent-dot svg{opacity:1}
#spacefold-mac .sf-accent-description{margin:15px 1px 0;padding-top:13px;border-top:1px solid var(--sf-line);font-size:11px;line-height:1.6;color:var(--sf-muted)}
#spacefold-mac input[type="checkbox"],#spacefold-mac input[type="radio"]{accent-color:var(--sf-accent-button)}
#spacefold-mac #disk-workbench{
  --wb-accent:var(--sf-accent);
  --wb-accent-soft:var(--sf-accent-soft);
  --wb-selected:var(--sf-accent-selected);
  --wb-covered:var(--sf-accent-covered);
  --wb-bar:color-mix(in srgb,var(--sf-accent) 46%,var(--sf-panel));
  --wb-folder:var(--sf-accent-folder);
  --wb-folder-fill:var(--sf-accent-folder-fill);
}
#spacefold-mac #disk-workbench .wb-actions .wb-button:not(.wb-action-trash):hover:not(:disabled),#spacefold-mac #disk-workbench .wb-enter:hover{background:var(--sf-accent-soft);color:var(--sf-accent)}
#spacefold-mac #disk-workbench .wb-actions .wb-button:not(.wb-action-trash):hover:not(:disabled) svg{color:var(--sf-accent)}
#spacefold-mac #disk-workbench .wb-selected .wb-chevron{color:var(--sf-accent)}
#spacefold-mac #disk-map{--dm-accent:var(--sf-accent);--dm-accent-bg:var(--sf-accent-soft);--dm-main:var(--sf-accent-button);--dm-onmain:var(--sf-accent-onbutton)}
#spacefold-mac #disk-map .dm-tile[aria-pressed="true"]{box-shadow:inset 0 0 0 2px var(--sf-accent)}
#spacefold-mac #disk-map .dm-folder-icon{color:var(--sf-accent-folder)}
#spacefold-mac #disk-map .dm-folder-icon svg{fill:var(--sf-accent-folder-fill)}
#spacefold-mac #disk-map .dm-enter{background:var(--sf-accent-soft);border-color:transparent;color:var(--sf-accent);box-shadow:none}
#spacefold-mac #disk-map .dm-enter:hover:not(:disabled){background:var(--sf-accent-selected)}
#spacefold-mac #disk-map .dm-op:not(.dm-destructive):hover{background:var(--sf-accent-soft)}
#spacefold-mac #disk-map .dm-op:not(.dm-destructive):hover svg{color:var(--sf-accent)}
#spacefold-mac #disk-lens{--dl-active:var(--sf-accent);--dl-active-text:var(--sf-accent-onbutton)}
#spacefold-mac #disk-lens .dl-folder-row.dl-selected{background:var(--sf-accent-soft)}
#spacefold-mac #disk-lens .dl-folder-row.dl-selected .dl-folder-enter{color:var(--sf-accent)}
#spacefold-mac #disk-lens .dl-folder-select:hover{background:var(--sf-accent-soft)}
#spacefold-mac #disk-lens .dl-chart path[data-lens-selected="true"]{stroke:var(--sf-accent);stroke-width:2;vector-effect:non-scaling-stroke}
#spacefold-mac #disk-lens .dl-selection-icon{color:var(--sf-accent-folder)}
#spacefold-mac #disk-lens .dl-selection-icon svg{fill:var(--sf-accent-folder-fill)}
#spacefold-mac #disk-lens .dl-action:not(.dl-danger):hover{background:var(--sf-accent-soft)}
#spacefold-mac #disk-lens .dl-action:not(.dl-danger):hover svg{color:var(--sf-accent)}
#spacefold-mac .disk-dialog-layer,#spacefold-mac .disk-action-toast{--da-main:var(--sf-accent-button);--da-onmain:var(--sf-accent-onbutton)}
#spacefold-mac .disk-dialog-layer .disk-dialog button.da-primary{background:var(--sf-accent-button);border-color:transparent;color:var(--sf-accent-onbutton)}
#spacefold-mac .disk-dialog-layer .disk-dialog .da-format[aria-pressed="true"]{background:var(--sf-panel);color:var(--sf-accent)}
#spacefold-mac .disk-dialog-layer .da-object>svg{color:var(--sf-accent-folder);fill:var(--sf-accent-folder-fill)}
@container spacefold (max-width:900px){#spacefold-mac .sf-topbar{gap:12px}#spacefold-mac .sf-scan span{display:none}#spacefold-mac .sf-header-actions{gap:4px}}
@container spacefold (max-width:720px){#spacefold-mac .sf-topbar{gap:10px 12px}}
@container spacefold (max-width:420px){#spacefold-mac .sf-topbar{gap:9px 7px}#spacefold-mac .sf-header-actions{gap:0}#spacefold-mac .sf-export{padding-inline:7px}#spacefold-mac .sf-appearance-menu{padding:14px 14px 16px}}
@media(pointer:coarse){#spacefold-mac .sf-settings-trigger{min-width:44px}#spacefold-mac .sf-appearance-close{width:44px;min-height:44px}#spacefold-mac .sf-accent-option{min-height:68px}}
</style>

<style>
#spacefold-mac{
  --sf-ink:light-dark(#272830,#efedf4);
  --sf-muted:light-dark(#71737e,#a7a5b2);
  --sf-panel:light-dark(#ffffff,#25252b);
  --sf-line:light-dark(#e9e9ef,#3b3944);
  --sf-soft:light-dark(#f3f3f7,#302e38);
  --sf-chrome:light-dark(#fafafb,#29282f);
  --sf-danger:light-dark(#ba4c54,#eea1a5);
  border-color:light-dark(#dedee7,#45424e);
  border-radius:14px;
  box-shadow:0 14px 40px light-dark(#2723350c,#00000024);
}
#spacefold-mac .sf-topbar{min-height:62px;padding:10px 17px;gap:16px;background:var(--sf-chrome)}
#spacefold-mac .sf-brand{font-size:15px;gap:8px;letter-spacing:-.15px}
#spacefold-mac .sf-brand-symbol{width:27px;height:27px;border-radius:8px}
#spacefold-mac .sf-brand-symbol svg{width:17px;height:17px}
#spacefold-mac .sf-source{gap:9px;min-width:157px;padding:4px 8px;min-height:38px;border-radius:7px}
#spacefold-mac .sf-source>svg{width:17px;height:17px}
#spacefold-mac .sf-source>svg:last-child{width:11px;height:11px;margin-left:2px}
#spacefold-mac .sf-source-copy{gap:2px}
#spacefold-mac .sf-source-meta{font-size:11px;line-height:1.35}
#spacefold-mac .sf-switch{padding:3px;border-radius:8px;background:var(--sf-soft)}
#spacefold-mac .sf-switch-thumb{border-radius:6px;box-shadow:0 1px 3px light-dark(#27233512,#00000023)}
#spacefold-mac .sf-tab{padding:6px 12px;min-height:31px;font-size:13px;gap:6px}
#spacefold-mac .sf-header-actions{gap:5px}
#spacefold-mac .sf-tool{min-height:33px;min-width:32px;padding:7px 8px;border-radius:7px}
#spacefold-mac .sf-tool svg{width:16px;height:16px}
#spacefold-mac .sf-export{margin-left:3px;padding:7px 11px}
#spacefold-mac .sf-popover{background:var(--sf-panel);border-color:var(--sf-line);border-radius:12px;box-shadow:0 14px 38px light-dark(#24203120,#00000057),0 2px 5px light-dark(#24203108,#0000001a)}
#spacefold-mac .sf-source-menu{padding:16px}
#spacefold-mac .sf-source-heading{margin-bottom:13px}
#spacefold-mac .sf-place{min-height:36px;padding:7px 9px}
#spacefold-mac .sf-category-dot{width:7px;height:7px;border-radius:2px}
#spacefold-mac .sf-menu-item{min-height:35px;padding:8px 10px}
#spacefold-mac .sf-menu-item[data-sf-file="trash"],#spacefold-mac .sf-menu-item[data-sf-file="trash"] svg{color:var(--sf-danger)}
#spacefold-mac .sf-appearance-menu{padding:16px 17px 17px}
#spacefold-mac .sf-appearance-heading{margin-bottom:18px}
#spacefold-mac .sf-appearance-close{width:27px;min-height:27px}
#spacefold-mac .sf-accent-description{font-size:12px;margin-top:14px;line-height:1.5}



















#spacefold-mac #disk-lens .dl-folder-row.dl-selected{background:transparent}
#spacefold-mac #disk-lens .dl-folder-row.dl-selected>.dl-row-main{background:var(--sf-accent-soft)}
#spacefold-mac .disk-dialog-layer{padding-top:56px;background:light-dark(#201e2a22,#0000005c)}
#spacefold-mac .disk-dialog-layer .disk-dialog{background:var(--sf-panel);border-color:var(--sf-line);border-radius:14px;box-shadow:0 20px 65px light-dark(#211b3630,#00000070)}
#spacefold-mac .disk-dialog-layer .da-head{padding:21px 22px 12px}
#spacefold-mac .disk-dialog-layer .da-title{font-size:18px;font-weight:600;letter-spacing:-.3px}
#spacefold-mac .disk-dialog-layer .da-body{padding:10px 22px 19px}
#spacefold-mac .disk-dialog-layer .da-footer{padding:14px 22px 17px;gap:8px}
#spacefold-mac .disk-dialog-layer .disk-dialog button{min-height:33px;border-radius:7px;background:var(--sf-panel);border-color:var(--sf-line);box-shadow:none}
#spacefold-mac .disk-dialog-layer .disk-dialog button:hover{background:var(--sf-soft)}
#spacefold-mac .disk-dialog-layer .disk-dialog button.da-primary{background:var(--sf-accent-button);color:var(--sf-accent-onbutton);border-color:transparent}
#spacefold-mac .disk-dialog-layer .disk-dialog button.da-danger{background:light-dark(#bc4d55,#b94a51);color:light-dark(#ffffff,#ffffff);border-color:transparent}
#spacefold-mac .disk-dialog-layer .disk-dialog button.da-close{width:28px;min-height:28px;background:var(--sf-soft);border-radius:50%}
#spacefold-mac .disk-dialog-layer .da-formats{background:var(--sf-soft);gap:3px;border-radius:9px;padding:3px}
#spacefold-mac .disk-dialog-layer .disk-dialog .da-format{min-height:42px;background:transparent;border-radius:7px}
#spacefold-mac .disk-dialog-layer .disk-dialog .da-format[aria-pressed="true"]{background:var(--sf-panel);color:var(--sf-accent);box-shadow:0 1px 3px light-dark(#24203112,#00000022)}
#spacefold-mac .disk-dialog-layer .disk-dialog input[type="text"],#spacefold-mac .disk-dialog-layer .disk-dialog select,#spacefold-mac .disk-dialog-layer .disk-dialog textarea{min-height:35px;border-radius:7px;border-color:var(--sf-line);background:var(--sf-panel);box-shadow:none}
#spacefold-mac .disk-dialog-layer .da-preview{border-color:var(--sf-line);border-radius:8px;background:var(--sf-panel)}
#spacefold-mac .disk-dialog-layer .da-preview-head{flex-wrap:wrap;gap:5px 10px;min-width:0;background:var(--sf-soft)}
#spacefold-mac .disk-dialog-layer .da-preview-head>*{min-width:0;overflow-wrap:anywhere}
#spacefold-mac .disk-dialog-layer .da-path{overflow-wrap:anywhere}
@container spacefold (max-width:960px){#spacefold-mac .sf-topbar{gap:12px;padding-inline:14px}#spacefold-mac .sf-scan span{display:none}#spacefold-mac .sf-tab{padding-inline:10px}#spacefold-mac .sf-source{min-width:145px}#spacefold-mac .sf-header-actions{gap:3px}}
@container spacefold (max-width:780px){#spacefold-mac .sf-topbar{display:grid;grid-template-columns:auto minmax(0,1fr) auto;gap:10px 14px;padding:11px 15px}#spacefold-mac .sf-source{justify-self:start;min-width:0}#spacefold-mac .sf-switch{grid-row:2;grid-column:1/-1;margin:0;justify-content:stretch}#spacefold-mac .sf-tab{flex:1;min-height:33px}#spacefold-mac .sf-header-actions{margin:0;gap:5px}}
@container spacefold (max-width:530px){#spacefold-mac .disk-dialog-layer{padding:52px 9px 15px}#spacefold-mac .disk-dialog-layer .da-head{padding:18px 16px 10px}#spacefold-mac .disk-dialog-layer .da-body{padding:9px 16px 16px}#spacefold-mac .disk-dialog-layer .da-footer{padding:12px 16px 15px}}
@container spacefold (max-width:420px){#spacefold-mac .sf-topbar{gap:9px 8px;padding:10px}#spacefold-mac .sf-brand-name{display:none}#spacefold-mac .sf-brand{gap:0}#spacefold-mac .sf-source{gap:6px;padding-inline:2px}#spacefold-mac .sf-source-total{display:none}#spacefold-mac .sf-source>svg:first-child{display:none}#spacefold-mac .sf-source>svg:last-child{margin-left:0}#spacefold-mac .sf-header-actions{gap:1px}#spacefold-mac .sf-header-actions .sf-scan{display:none}#spacefold-mac .sf-export{margin-left:0;padding-inline:8px}#spacefold-mac .sf-tool{padding-inline:7px;min-width:30px}#spacefold-mac .sf-tab{padding-inline:6px;gap:5px}}
@media(pointer:coarse){#spacefold-mac .sf-tool{min-width:44px}#spacefold-mac .disk-dialog-layer .disk-dialog button{min-height:44px}#spacefold-mac .disk-dialog-layer .disk-dialog button.da-close{min-height:44px;width:44px}#spacefold-mac .disk-dialog-layer .disk-dialog input[type="text"],#spacefold-mac .disk-dialog-layer .disk-dialog select,#spacefold-mac .disk-dialog-layer .disk-dialog textarea{font-size:16px}}
@media(prefers-contrast:more){#spacefold-mac{--sf-muted:light-dark(#565560,#c8c4d4);--sf-line:light-dark(#b9b5c5,#797283)}}
</style>
<style>
/* Final geometry: operations always live in the right inspector. */
#spacefold-mac .sf-inspector-toggle{display:none;flex:none;align-items:center;justify-content:center;gap:5px;min-height:30px;padding:5px 7px;border:0;border-radius:6px;background:var(--sf-accent-soft);color:var(--sf-accent);font-size:13px;line-height:1.4;font-weight:400;white-space:nowrap}
#spacefold-mac .sf-inspector-toggle svg{width:14px;height:14px;stroke-width:1.6}
#spacefold-mac .sf-inspector-close{display:none;align-items:center;justify-content:center;flex:none;width:27px;min-height:27px;padding:5px;border:0;border-radius:6px;background:transparent;color:var(--sf-muted)}
#spacefold-mac .sf-inspector-close svg{width:15px;height:15px;stroke-width:1.7}
#spacefold-mac .sf-inspector-close:hover{background:var(--sf-soft)}
#spacefold-mac .sf-inspector-heading{display:flex;align-items:center;justify-content:space-between;gap:8px}
#spacefold-mac .sf-inspector-heading-actions{display:flex;align-items:center;gap:4px}
#spacefold-mac .sf-inspector-clear{min-height:25px;padding:3px 5px;border:0;border-radius:5px;background:transparent;color:var(--sf-muted);font-size:12px;line-height:1.4}
#spacefold-mac .sf-inspector-clear:hover{background:var(--sf-soft);color:var(--sf-ink)}
#spacefold-mac .sf-inspector-scrim{position:absolute;inset:44px 0 0;z-index:20;border:0;border-radius:0;background:light-dark(#201d2918,#00000045);width:100%;padding:0}
#spacefold-mac .sf-inspector-scrim:active{filter:none}

#spacefold-mac #disk-workbench .wb-content{display:grid;grid-template-columns:minmax(0,1fr) 220px}
#spacefold-mac #disk-workbench .wb-directory{padding:0 10px 18px}
#spacefold-mac #disk-workbench .wb-inspector{display:block;padding:14px 16px 16px;border-left:1px solid var(--sf-line);border-top:0;background:var(--wb-surface)}
#spacefold-mac #disk-workbench .wb-inspector-heading{display:flex;margin:0 0 14px;min-height:25px}
#spacefold-mac #disk-workbench .wb-selection-top{gap:9px}
#spacefold-mac #disk-workbench .wb-selection-metric{padding:20px 0 16px;text-align:left}
#spacefold-mac #disk-workbench .wb-selection-size{font-size:30px}
#spacefold-mac #disk-workbench .wb-selection-meta{max-width:none;margin:7px 0 0}
#spacefold-mac #disk-workbench .wb-selection-track{max-width:none;margin:10px 0 0}
#spacefold-mac #disk-workbench .wb-actions{display:flex;padding:0 0 15px;border-bottom:1px solid var(--sf-line);gap:7px}
#spacefold-mac #disk-workbench .wb-primary-actions{grid-template-columns:minmax(0,1fr) 68px}
#spacefold-mac #disk-workbench .wb-secondary-actions{grid-template-columns:repeat(2,minmax(0,1fr))}
#spacefold-mac #disk-workbench .wb-inspector[data-wb-selection-mode="multiple"] .wb-actions{display:flex}
#spacefold-mac #disk-workbench .wb-inspector[data-wb-selection-mode="multiple"] [data-wb-action="open"],#spacefold-mac #disk-workbench .wb-inspector[data-wb-selection-mode="multiple"] [data-wb-action="rename"]{display:none}
#spacefold-mac #disk-workbench .wb-inspector[data-wb-selection-mode="multiple"] .wb-primary-actions{grid-template-columns:1fr}
#spacefold-mac #disk-workbench .wb-inspector[data-wb-selection-mode="multiple"] [data-wb-action="move"]{color:var(--sf-accent);background:var(--sf-accent-soft);box-shadow:none}
#spacefold-mac #disk-workbench .wb-inspector[data-wb-selection-mode="multiple"] [data-wb-action="move"] svg{color:inherit}
#spacefold-mac #disk-workbench .wb-breakdown{padding:14px 0 0;border:0}
#spacefold-mac #disk-workbench .wb-breakdown-list,#spacefold-mac #disk-workbench .wb-inspector[data-wb-selection-mode="multiple"] .wb-breakdown-list{grid-template-columns:1fr}
#spacefold-mac #disk-workbench .wb-location{padding:15px 0 0}
#spacefold-mac #disk-workbench .wb-location-label{display:block}
#spacefold-mac #disk-workbench .wb-statusbar{display:flex}

#spacefold-mac #disk-map .dm-content{display:grid;grid-template-columns:minmax(0,1fr) 220px;min-height:608px;position:relative}
#spacefold-mac #disk-map .dm-pathbar{display:flex;align-items:center;gap:4px;min-height:44px;padding-right:10px}
#spacefold-mac #disk-map .dm-breadcrumb{flex:1;min-width:0;min-height:44px;padding:6px 10px 6px 12px}
#spacefold-mac #disk-map .dm-map{min-height:520px;margin:0 14px 0 16px}
#spacefold-mac #disk-map .dm-inspector{display:flex;flex-direction:column;padding:0 16px 16px;gap:0;border-left:1px solid var(--sf-line);border-top:0}
#spacefold-mac #disk-map .dm-inspect-label{display:flex;height:44px;font-size:12px}
#spacefold-mac #disk-map .dm-object{display:flex;gap:9px;min-width:0;margin-top:3px}
#spacefold-mac #disk-map .dm-folder-icon{height:auto}
#spacefold-mac #disk-map .dm-folder-icon svg{width:28px;height:28px}
#spacefold-mac #disk-map .dm-path{font-size:12px;text-align:left;margin:7px 0 0}
#spacefold-mac #disk-map .dm-size-block{margin:17px 0 14px;text-align:left}
#spacefold-mac #disk-map .dm-selected-size{font-size:30px}
#spacefold-mac #disk-map .dm-inspector dl{display:grid;grid-template-columns:1fr auto;justify-content:stretch;gap:7px 5px;font-size:12px;margin:0 0 14px}
#spacefold-mac #disk-map .dm-inspector dl dt:nth-of-type(2),#spacefold-mac #disk-map .dm-inspector dl dd:nth-of-type(2){display:block}
#spacefold-mac #disk-map .dm-children{margin:17px 0 0}
#spacefold-mac #disk-map .dm-child-list{display:flex;flex-direction:column;gap:1px}
#spacefold-mac #disk-map .dm-child{min-height:30px;padding:6px 3px}
#spacefold-mac #disk-map .dm-inspect-separator{display:grid;grid-template-columns:1fr 1fr;gap:2px 6px;margin-top:auto;padding-top:16px;border-top:0}
#spacefold-mac #disk-map .dm-op{min-height:31px;font-size:12px;padding:6px 2px}

#spacefold-mac #disk-lens .dl-shell{min-height:608px}
#spacefold-mac #disk-lens .dl-pathbar{display:flex;align-items:center;gap:4px;min-height:44px;padding-right:10px;border-bottom:1px solid var(--sf-line)}
#spacefold-mac #disk-lens .dl-breadcrumbs{flex:1;min-width:0;min-height:43px;padding:6px 12px 6px 12px}
#spacefold-mac #disk-lens .dl-workspace{display:grid;grid-template-columns:minmax(0,1fr) 220px;gap:0;padding:0;min-height:564px;position:relative;align-items:stretch}
#spacefold-mac #disk-lens .dl-radial-column{width:auto;min-width:0;align-self:stretch;justify-content:center;padding:18px 24px 25px}
#spacefold-mac #disk-lens .dl-chart-stage{max-width:440px}
#spacefold-mac #disk-lens .dl-directory-column{display:flex;flex-direction:column;align-self:stretch;justify-content:flex-start;width:auto;min-width:0;padding:14px 12px 16px;border-left:1px solid var(--sf-line);background:var(--dl-panel)}
#spacefold-mac #disk-lens .dl-list-heading{min-height:25px;margin:0 3px 9px;font-size:12px}
#spacefold-mac #disk-lens .dl-folders{gap:3px}
#spacefold-mac #disk-lens .dl-folder-select{min-height:44px;padding:6px 3px 6px 6px;gap:6px}
#spacefold-mac #disk-lens .dl-folder-name{font-size:13px}
#spacefold-mac #disk-lens .dl-folder-meta{margin-top:2px;font-size:11px}
#spacefold-mac #disk-lens .dl-folder-size{font-size:12px}
#spacefold-mac #disk-lens .dl-folder-size small{margin-top:2px;font-size:11px}
#spacefold-mac #disk-lens .dl-folder-enter{min-width:22px;margin-right:2px;min-height:34px}
#spacefold-mac #disk-lens .dl-swatch{height:24px;width:5px}
#spacefold-mac #disk-lens .dl-actionbar{display:flex;flex-direction:column;align-items:stretch;gap:13px;min-height:0;margin-top:auto;padding:16px 3px 0;border-top:1px solid var(--sf-line);background:transparent}
#spacefold-mac #disk-lens .dl-selected-detail{width:auto;flex:none;align-items:flex-start;gap:8px}
#spacefold-mac #disk-lens .dl-selection-icon svg{width:24px;height:24px}
#spacefold-mac #disk-lens .dl-selection-copy{width:100%;min-width:0}
#spacefold-mac #disk-lens .dl-selection-title{max-width:100%;white-space:normal;overflow-wrap:anywhere;font-size:13px}
#spacefold-mac #disk-lens .dl-selection-path{max-width:100%;white-space:normal;overflow-wrap:anywhere;font-size:11px;margin-top:4px}
#spacefold-mac #disk-lens .dl-actions{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));width:100%;margin:0;gap:3px 4px;justify-content:stretch}
#spacefold-mac #disk-lens .dl-action{justify-content:flex-start;min-height:31px;padding:6px 3px;font-size:12px;gap:5px}
#spacefold-mac #disk-lens .dl-actionbar .dl-action:first-child{padding-inline:5px;justify-content:center}
#spacefold-mac #disk-lens .dl-separator{display:none}

@container spacefold (min-width:720px) and (max-width:850px){
  #spacefold-mac .sf-topbar{display:flex;gap:10px;min-height:62px;padding:10px 13px}
  #spacefold-mac .sf-brand-name{display:none}
  #spacefold-mac .sf-source{min-width:132px;padding-inline:4px;gap:7px}
  #spacefold-mac .sf-source-total{display:none}
  #spacefold-mac .sf-switch{grid-row:auto;grid-column:auto;margin-left:auto;flex:none}
  #spacefold-mac .sf-tab{flex:none;min-height:31px;padding:6px 8px;gap:5px}
  #spacefold-mac .sf-header-actions{gap:2px;margin-left:auto}
  #spacefold-mac .sf-tool{min-width:30px;padding:7px 6px}
  #spacefold-mac .sf-export{padding:7px 8px;margin-left:2px}
}
@container spacefold (max-width:800px){
  #spacefold-mac #disk-workbench .wb-content,#spacefold-mac #disk-map .dm-content,#spacefold-mac #disk-lens .dl-workspace{grid-template-columns:minmax(0,1fr) 204px}
  #spacefold-mac #disk-workbench .wb-inspector{padding-inline:13px}
  #spacefold-mac #disk-workbench .wb-table col.wb-size-col{width:82px}
  #spacefold-mac #disk-workbench .wb-table col.wb-share-col{width:89px}
  #spacefold-mac #disk-workbench .wb-share{gap:6px}
  #spacefold-mac #disk-workbench .wb-node{--wb-step:15px}
  #spacefold-mac #disk-map .dm-inspector{padding-inline:13px}
  #spacefold-mac #disk-lens .dl-directory-column{padding-inline:10px}
  #spacefold-mac #disk-lens .dl-radial-column{padding-inline:16px}
}
@container spacefold (max-width:639.98px){
  #spacefold-mac .sf-inspector-toggle,#spacefold-mac .sf-inspector-close{display:inline-flex}
  #spacefold-mac #disk-workbench,#spacefold-mac #disk-map,#spacefold-mac #disk-lens .dl-shell{min-height:650px}
  #spacefold-mac #disk-workbench .wb-content,#spacefold-mac #disk-map .dm-content,#spacefold-mac #disk-lens .dl-workspace{display:grid;grid-template-columns:minmax(0,1fr)}
  #spacefold-mac #disk-workbench .wb-inspector,#spacefold-mac #disk-map .dm-inspector,#spacefold-mac #disk-lens .dl-directory-column{display:none;position:absolute;top:44px;right:0;z-index:30;width:min(304px,calc(100% - 16px));height:calc(100% - 44px);min-height:0;overflow-y:auto;overscroll-behavior:contain;padding:14px 17px 18px;border-left:1px solid var(--sf-line);border-top:0;background:var(--sf-panel);box-shadow:-10px 0 32px light-dark(#26213014,#0000003d)}
  #spacefold-mac #disk-workbench[data-sf-inspector-open="true"] .wb-inspector{display:block}
  #spacefold-mac #disk-map[data-sf-inspector-open="true"] .dm-inspector,#spacefold-mac #disk-lens[data-sf-inspector-open="true"] .dl-directory-column{display:flex}
  #spacefold-mac #disk-workbench .wb-inspector-heading{margin-bottom:14px}
  #spacefold-mac #disk-workbench .wb-selection-size{font-size:30px}
  #spacefold-mac #disk-workbench .wb-tree-tools{padding-left:4px}
  #spacefold-mac #disk-workbench .wb-pathbar{gap:4px;padding-inline:7px}
  #spacefold-mac #disk-workbench .wb-path-size{display:none}
  #spacefold-mac #disk-workbench .wb-selection-metric{padding:17px 0}
  #spacefold-mac #disk-map .dm-content{min-height:650px}
  #spacefold-mac #disk-map .dm-map{min-height:520px;height:auto;flex:1;margin:0 12px}
  #spacefold-mac #disk-map .dm-inspect-label{height:27px;min-height:27px;margin-bottom:9px}
  #spacefold-mac #disk-map .dm-map-footer{min-height:43px;padding-inline:14px}
  #spacefold-mac #disk-map .dm-inspector dl{display:grid}
  #spacefold-mac #disk-map .dm-inspect-separator{margin-top:16px}
  #spacefold-mac #disk-lens .dl-workspace{min-height:606px;position:static}
  #spacefold-mac #disk-lens .dl-radial-column{padding:20px 18px;justify-content:center;width:100%}
  #spacefold-mac #disk-lens .dl-chart-stage{max-width:430px}
  #spacefold-mac #disk-lens .dl-actionbar{margin-top:20px}
  #spacefold-mac #disk-lens .dl-center-size{font-size:34px}
  #spacefold-mac #disk-lens .dl-scope-size{display:none}
}
@container spacefold (max-width:480px){
  #spacefold-mac #disk-workbench .wb-directory{padding-inline:5px}
  #spacefold-mac #disk-workbench .wb-table col.wb-size-col{width:78px}
  #spacefold-mac #disk-workbench .wb-share-col,#spacefold-mac #disk-workbench .wb-share-head,#spacefold-mac #disk-workbench .wb-share-cell{display:none}
  #spacefold-mac #disk-workbench .wb-node{--wb-step:12px}
  #spacefold-mac #disk-workbench .wb-statusbar{flex-wrap:wrap;gap:3px 7px}
  #spacefold-mac #disk-workbench .wb-status-right{margin-left:auto}
}
@container spacefold (max-width:360px){#spacefold-mac #disk-lens .dl-center-size{font-size:30px}#spacefold-mac #disk-lens .dl-center-name{font-size:12px}#spacefold-mac #disk-workbench .wb-tree-tools [data-wb-expand]{display:none}}
@media(pointer:coarse){
  #spacefold-mac .sf-inspector-toggle,#spacefold-mac .sf-inspector-close,#spacefold-mac .sf-inspector-clear{min-height:44px}
  #spacefold-mac .sf-inspector-close{min-width:44px}
  #spacefold-mac #disk-lens .dl-folder-select,#spacefold-mac #disk-lens .dl-folder-enter,#spacefold-mac #disk-lens .dl-action,#spacefold-mac #disk-map .dm-op,#spacefold-mac #disk-map .dm-child{min-height:44px}
}
</style>

<div id="spacefold-mac" data-sf-accent="purple" aria-label="间隙磁盘空间分析，三种视图交互演示">
  <header class="sf-topbar">
    <div class="sf-brand" aria-label="间隙"><span class="sf-brand-symbol"><i data-lucide="layers" aria-hidden="true"></i></span><span class="sf-brand-name">间隙</span></div>
    <button type="button" class="sf-source cursor-interaction" data-sf-source aria-label="浏览扫描位置：Data，D盘，已用742GB，总容量1TB" aria-expanded="false" aria-controls="sf-source-menu"><i data-lucide="hard-drive" aria-hidden="true"></i><span class="sf-source-copy"><span class="sf-source-name">Data · D:</span><span class="sf-source-meta">已用 742 GB<span class="sf-source-total"> / 1 TB</span></span></span><i data-lucide="chevron-down" aria-hidden="true"></i></button>
    <nav class="sf-switch" role="tablist" aria-label="显示方式">
      <span class="sf-switch-thumb" aria-hidden="true"></span>
      <button id="sf-mac-tab-map" class="sf-tab cursor-interaction" type="button" role="tab" aria-controls="sf-mac-view-map" aria-selected="false" data-sf-view="map"><i data-lucide="layout-grid" aria-hidden="true"></i><span>空间地图</span></button>
      <button id="sf-mac-tab-tree" class="sf-tab cursor-interaction" type="button" role="tab" aria-controls="sf-mac-view-tree" aria-selected="true" data-sf-view="tree"><i data-lucide="list-tree" aria-hidden="true"></i><span>目录树</span></button>
      <button id="sf-mac-tab-lens" class="sf-tab cursor-interaction" type="button" role="tab" aria-controls="sf-mac-view-lens" aria-selected="false" data-sf-view="lens"><i data-lucide="aperture" aria-hidden="true"></i><span>层级透镜</span></button>
    </nav>
    <div class="sf-header-actions">
      <button type="button" class="sf-tool sf-settings-trigger cursor-interaction" data-sf-settings aria-label="设置：强调色" aria-expanded="false" aria-controls="sf-appearance-menu" data-tooltip="设置"><i data-lucide="settings-2" aria-hidden="true"></i></button>
      <button type="button" class="sf-tool sf-scan cursor-interaction" data-sf-action="scan" aria-label="选择扫描位置" data-tooltip="选择扫描位置"><i data-lucide="folder-plus" aria-hidden="true"></i><span>扫描</span></button>
      <button type="button" class="sf-tool cursor-interaction" data-sf-menu aria-label="文件操作" aria-expanded="false" aria-controls="sf-mac-menu" data-tooltip="文件操作"><i data-lucide="ellipsis" aria-hidden="true"></i></button>
      <button type="button" class="sf-tool sf-export cursor-interaction" data-sf-action="export" aria-label="导出扫描数据"><i data-lucide="download" aria-hidden="true"></i><span>导出</span></button>
    </div>
  </header>
  <div class="sf-body">
    <main class="sf-content">
      <div id="sf-mac-view-map" class="sf-view" role="tabpanel" aria-labelledby="sf-mac-tab-map" hidden><section aria-label="空间地图">
<div id="disk-map">
  <div class="dm-content">
    <div class="dm-canvas-wrap">
      <div class="dm-pathbar"><nav class="dm-breadcrumb" aria-label="当前目录"></nav><button type="button" class="sf-inspector-toggle cursor-interaction" data-sf-inspector-toggle="map" aria-expanded="false" aria-controls="sf-map-inspector"><i data-lucide="panel-right" aria-hidden="true"></i>详情</button></div>
      <div class="dm-map" aria-label="目录矩形面积图，面积代表逻辑大小，内部色块代表下级目录"></div>
      <div class="dm-map-footer"><span class="dm-map-scope">6 个项目</span><span>面积表示大小 · 显示两级结构</span></div>
    </div>
    <aside id="sf-map-inspector" class="dm-inspector" data-sf-inspector="map" aria-label="所选项目详情">
      <div class="dm-inspect-label sf-inspector-heading"><span>项目详情</span><button type="button" class="sf-inspector-close cursor-interaction" data-sf-inspector-close aria-label="关闭详情"><i data-lucide="x" aria-hidden="true"></i></button></div>
      <div class="dm-object"><div class="dm-folder-icon"><i data-lucide="folder" aria-hidden="true"></i></div><h3 class="dm-selected-name">AI Models</h3></div>
      <div class="dm-path">D:\AI Models</div>
      <div class="dm-size-block"><div class="dm-selected-size">286 <span>GB</span></div><div class="dm-size-label">逻辑大小</div></div>
      <dl><dt>占当前目录</dt><dd class="dm-selected-ratio">38.5%</dd><dt>包含项目</dt><dd class="dm-selected-children">4 个</dd></dl>
      <button type="button" class="dm-button dm-enter cursor-interaction" data-map-action="enter">进入目录<i data-lucide="chevron-right" aria-hidden="true"></i></button>
      <div class="dm-children" data-map-children><div class="dm-children-title"><span>下级项目</span><span>占用</span></div><div class="dm-child-list"></div></div>
      <div class="dm-inspect-separator">
        <button type="button" class="dm-op cursor-interaction" data-map-action="open"><i data-lucide="folder-search" aria-hidden="true"></i>打开位置</button>
        <button type="button" class="dm-op cursor-interaction" data-map-action="copy"><i data-lucide="link" aria-hidden="true"></i>复制路径</button>
        <button type="button" class="dm-op cursor-interaction" data-map-action="move"><i data-lucide="folder-input" aria-hidden="true"></i>移动到…</button>
        <button type="button" class="dm-op cursor-interaction" data-map-action="rename"><i data-lucide="text-cursor-input" aria-hidden="true"></i>重命名</button>
        <button type="button" class="dm-op dm-destructive cursor-interaction" data-map-action="trash"><i data-lucide="trash-2" aria-hidden="true"></i>回收站</button>
        <button type="button" class="dm-op cursor-interaction" data-map-action="export"><i data-lucide="download" aria-hidden="true"></i>导出所选</button>
      </div>
    </aside>
    <button type="button" class="sf-inspector-scrim cursor-interaction" data-sf-inspector-scrim aria-label="关闭详情" hidden></button>
  </div>
  <span class="dm-live" aria-live="polite"></span>
</div>
</section></div>
      <div id="sf-mac-view-tree" class="sf-view" role="tabpanel" aria-labelledby="sf-mac-tab-tree"><section aria-label="目录树：展开层级、按同层大小排序并批量整理文件">
  <div id="disk-workbench">
    <header class="wb-topbar"><span>目录树</span></header>
    <div class="wb-body">
      <main class="wb-main">
        <div class="wb-pathbar">
          <button class="wb-button wb-button-ghost wb-icon-button cursor-interaction" type="button" data-wb-up aria-label="返回上级目录" disabled><i data-lucide="chevron-left" aria-hidden="true"></i></button>
          <nav class="wb-breadcrumbs" aria-label="当前扫描范围" data-wb-breadcrumbs><button type="button" class="cursor-interaction" data-wb-nav="D:\">工作盘 D:</button></nav>
          <span class="wb-path-size wb-number" data-wb-total>742 GB</span>
          <div class="wb-tree-tools" aria-label="目录展开层级">
            <button class="wb-button wb-button-ghost wb-icon-button cursor-interaction" type="button" data-wb-collapse aria-label="折叠全部下级目录" data-tooltip="折叠全部下级目录"><i data-lucide="chevrons-down-up" aria-hidden="true"></i></button>
            <button class="wb-button wb-button-ghost wb-icon-button cursor-interaction" type="button" data-wb-expand aria-label="展开一级目录" data-tooltip="展开一级目录"><i data-lucide="chevrons-up-down" aria-hidden="true"></i></button>
          </div>
          <button type="button" class="sf-inspector-toggle cursor-interaction" data-sf-inspector-toggle="tree" aria-expanded="false" aria-controls="sf-tree-inspector"><i data-lucide="panel-right" aria-hidden="true"></i>详情</button>
        </div>
        <span class="wb-accessible" data-wb-subtitle>6 个一级目录 · 同层按占用从大到小</span>
        <div class="wb-content">
          <section class="wb-directory" aria-label="目录层级与文件占用">
            <div class="wb-table-zone">
              <table class="wb-table" role="treegrid" aria-label="按层级展示的文件与目录占用">
                <colgroup><col class="wb-check-col"><col class="wb-name-col"><col class="wb-size-col"><col class="wb-share-col"><col class="wb-type-col"><col class="wb-open-col"></colgroup>
                <thead><tr role="row">
                  <th scope="col" class="wb-check-head"><label class="wb-check-hit cursor-interaction"><input class="wb-check cursor-interaction" type="checkbox" data-wb-select-all aria-label="选择当前范围中的全部目录及文件"></label></th>
                  <th scope="col" data-wb-name-head aria-sort="none"><button class="cursor-interaction" type="button" data-wb-sort="name">名称<span data-wb-name-sort-icon></span></button></th>
                  <th scope="col" class="wb-align-right" data-wb-size-head aria-sort="descending"><button class="cursor-interaction" type="button" data-wb-sort="size" aria-label="按逻辑大小排序" data-tooltip="逻辑大小">大小<span data-wb-size-sort-icon><i data-lucide="arrow-down" aria-hidden="true"></i></span></button></th>
                  <th scope="col" class="wb-share-head">占父目录</th>
                  <th scope="col" class="wb-type-head">类型 / 选择</th>
                  <th scope="col" class="wb-open-head"><span class="wb-accessible">聚焦或打开</span></th>
                </tr></thead>
                <tbody data-wb-rows></tbody>
              </table>
            </div>
          </section>
          <aside id="sf-tree-inspector" class="wb-inspector" data-sf-inspector="tree" data-wb-inspector aria-label="选中项目及文件操作">
            <div class="wb-inspector-heading sf-inspector-heading"><span>项目详情</span><span class="sf-inspector-heading-actions"><button type="button" class="sf-inspector-clear cursor-interaction" data-wb-clear aria-label="取消全部选择">清除</button><button type="button" class="sf-inspector-close cursor-interaction" data-sf-inspector-close aria-label="关闭详情"><i data-lucide="x" aria-hidden="true"></i></button></span></div>
            <div class="wb-selection-top" aria-live="polite">
              <div class="wb-selection-icon" data-wb-selected-icon><i data-lucide="folder-open" aria-hidden="true"></i></div>
              <div class="wb-selection-text"><div class="wb-selection-title" data-wb-selected-name>AI Models</div><span class="wb-selection-kind" data-wb-selected-kind>文件夹 · 4 个下级项目</span></div>
            </div>
            <div class="wb-selection-metric wb-number"><div class="wb-selection-size" data-wb-selected-size>286 GB</div><div class="wb-selection-meta" data-wb-selected-meta>占当前范围 38.5%</div><div class="wb-selection-track" aria-hidden="true"><span></span></div></div>
            <div class="wb-actions">
              <div class="wb-primary-actions">
                <button class="wb-button wb-action-open cursor-interaction" type="button" data-wb-action="open" aria-label="打开所在位置"><i data-lucide="external-link" aria-hidden="true"></i>打开位置</button>
                <button class="wb-button cursor-interaction" type="button" data-wb-action="move" aria-label="移动到其他目录"><i data-lucide="folder-input" aria-hidden="true"></i>移动</button>
              </div>
              <div class="wb-secondary-actions">
                <button class="wb-button cursor-interaction" type="button" data-wb-action="copy"><i data-lucide="copy" aria-hidden="true"></i>复制路径</button>
                <button class="wb-button cursor-interaction" type="button" data-wb-action="rename"><i data-lucide="pencil" aria-hidden="true"></i>重命名</button>
                <button class="wb-button cursor-interaction" type="button" data-wb-action="export"><i data-lucide="share" aria-hidden="true"></i>导出所选</button>
                <button class="wb-button wb-action-trash cursor-interaction" type="button" data-wb-action="trash" aria-label="移到回收站"><i data-lucide="trash-2" aria-hidden="true"></i>回收站</button>
              </div>
            </div>
            <section class="wb-breakdown" data-wb-breakdown aria-label="所选项目的占用明细">
              <div class="wb-breakdown-heading"><h4 data-wb-breakdown-title>下级占用</h4><button type="button" class="cursor-interaction" data-wb-focus aria-label="进入所选目录">进入<i data-lucide="arrow-right" aria-hidden="true"></i></button></div>
              <div class="wb-breakdown-list" data-wb-breakdown-list></div>
              <div class="wb-breakdown-more" data-wb-breakdown-more hidden></div>
            </section>
            <div class="wb-location"><span class="wb-location-label" data-wb-location-label>位置</span><div class="wb-selection-path" data-wb-selected-path>D:\AI Models</div></div>
          </aside>
        </div>
      </main>
    </div>
    <button type="button" class="sf-inspector-scrim cursor-interaction" data-sf-inspector-scrim aria-label="关闭详情" hidden></button>
    <footer class="wb-statusbar"><span class="wb-status-dot" aria-hidden="true"></span><span data-wb-status aria-live="polite">扫描完成 · 742 GB</span><span class="wb-status-right" data-wb-selected-count>1 项已选 · 包含子目录</span></footer>
  </div>
</section></div>
      <div id="sf-mac-view-lens" class="sf-view" role="tabpanel" aria-labelledby="sf-mac-tab-lens" hidden><section id="disk-lens" aria-label="层级透镜">
  <div class="dl-shell">
    <div class="dl-pathbar"><nav class="dl-breadcrumbs" aria-label="当前目录" data-lens-breadcrumb></nav><button type="button" class="sf-inspector-toggle cursor-interaction" data-sf-inspector-toggle="lens" aria-expanded="false" aria-controls="sf-lens-inspector"><i data-lucide="panel-right" aria-hidden="true"></i>详情</button></div>
    <div class="dl-workspace">
      <div class="dl-radial-column">
        <div class="dl-chart-stage">
          <svg class="dl-chart" role="img" aria-label="目录大小旭日图。内圈显示当前层级，外圈显示下级目录。右侧列表可选择和进入目录。"></svg>
          <button class="dl-center cursor-interaction" data-lens-center-enter type="button" aria-label="返回上一级"><span class="dl-center-name" data-lens-center-name>D: 工作盘</span><span class="dl-center-size" data-lens-center-size>742<span>GB</span></span><span class="dl-center-share" data-lens-center-share>当前目录</span><span class="dl-center-back" data-lens-center-back><i data-lucide="arrow-up-left" aria-hidden="true"></i>返回上一级</span></button>
        </div>
        <div class="dl-chart-caption"><span><i class="dl-ring-dot" aria-hidden="true"></i>当前层级</span><span><i class="dl-ring-dot dl-light" aria-hidden="true"></i>下级目录</span></div>
      </div>
      <aside id="sf-lens-inspector" class="dl-directory-column" data-sf-inspector="lens" aria-label="目录列表与选中项目操作">
        <div class="dl-list-heading sf-inspector-heading"><span data-lens-list-title>当前目录 · 6</span><button type="button" class="sf-inspector-close cursor-interaction" data-sf-inspector-close aria-label="关闭详情"><i data-lucide="x" aria-hidden="true"></i></button></div>
        <ul class="dl-folders" aria-label="目录占用列表" data-lens-folders></ul>
        <section class="dl-actionbar" aria-label="选中项目操作">
      <div class="dl-selected-detail"><span class="dl-selection-icon"><i data-lucide="folder" aria-hidden="true"></i></span><div class="dl-selection-copy"><div class="dl-selection-title" data-lens-selection-title>AI Models · 286 GB</div><div class="dl-selection-path" data-lens-selection-path>D:\AI Models</div></div></div>
      <div class="dl-actions" aria-label="选中项目操作">
        <button type="button" class="dl-action cursor-interaction" data-lens-action="open"><i data-lucide="folder-search" aria-hidden="true"></i>打开位置</button>
        <button type="button" class="dl-action cursor-interaction" data-lens-action="move"><i data-lucide="folder-input" aria-hidden="true"></i>移动</button>
        <button type="button" class="dl-action cursor-interaction" data-lens-action="rename"><i data-lucide="text-cursor-input" aria-hidden="true"></i>重命名</button>
        <button type="button" class="dl-action cursor-interaction" data-lens-action="copy"><i data-lucide="link" aria-hidden="true"></i>复制路径</button>
        <button type="button" class="dl-action cursor-interaction" data-lens-action="export"><i data-lucide="download" aria-hidden="true"></i>导出所选</button>
        <button type="button" class="dl-action dl-danger cursor-interaction" data-lens-action="trash"><i data-lucide="trash-2" aria-hidden="true"></i>回收站</button>
      </div>
        </section>
      </aside>
      <button type="button" class="sf-inspector-scrim cursor-interaction" data-sf-inspector-scrim aria-label="关闭详情" hidden></button>
    </div>
    <div class="dl-live" aria-live="polite" data-lens-live></div>
  </div>
</section></div>
    </main>
  </div>
  <div id="sf-source-menu" class="sf-popover sf-source-menu" role="group" aria-label="扫描结果与目录定位" hidden>
    <h2 class="sf-source-heading">本次扫描<small>演示数据</small></h2>
    <button type="button" class="sf-disk-option cursor-interaction" data-sf-path="D:\" aria-current="location"><i data-lucide="hard-drive" aria-hidden="true"></i><span class="sf-source-copy"><span class="sf-source-name">Data · D:</span><span class="sf-source-meta">查看整个磁盘的扫描结果</span></span><i data-lucide="arrow-up-right" aria-hidden="true"></i></button>
    <div class="sf-capacity">
      <div class="sf-capacity-label"><strong>742 GB 已用</strong><span>总容量 1 TB</span></div>
      <div class="sf-storage-track" role="img" aria-label="磁盘容量 1 TB，已用 742 GB，可用 258 GB"><span style="width:28.6%;background:var(--sf-ai)"></span><span style="width:21.8%;background:var(--sf-games)"></span><span style="width:13.2%;background:var(--sf-media)"></span><span style="width:6.4%;background:var(--sf-projects)"></span><span style="width:2.8%;background:var(--sf-downloads)"></span><span style="width:1.4%;background:var(--sf-others)"></span></div>
      <div class="sf-capacity-label"><span>258 GB 可用</span><span>74.2% 已使用</span></div>
    </div>
    <h3 class="sf-jump-heading">跳转目录</h3>
    <button type="button" class="sf-place cursor-interaction" data-sf-path="D:\AI Models"><i class="sf-category-dot" style="--sf-item-color:var(--sf-ai)" aria-hidden="true"></i><span class="sf-place-name">AI Models</span><small>286 GB</small></button>
    <button type="button" class="sf-place cursor-interaction" data-sf-path="D:\Games"><i class="sf-category-dot" style="--sf-item-color:var(--sf-games)" aria-hidden="true"></i><span class="sf-place-name">Games</span><small>218 GB</small></button>
    <button type="button" class="sf-place cursor-interaction" data-sf-path="D:\Media"><i class="sf-category-dot" style="--sf-item-color:var(--sf-media)" aria-hidden="true"></i><span class="sf-place-name">Media</span><small>132 GB</small></button>
    <button type="button" class="sf-place cursor-interaction" data-sf-path="D:\Projects"><i class="sf-category-dot" style="--sf-item-color:var(--sf-projects)" aria-hidden="true"></i><span class="sf-place-name">Projects</span><small>64 GB</small></button>
    <button type="button" class="sf-place cursor-interaction" data-sf-path="D:\Downloads"><i class="sf-category-dot" style="--sf-item-color:var(--sf-downloads)" aria-hidden="true"></i><span class="sf-place-name">Downloads</span><small>28 GB</small></button>
    <button type="button" class="sf-place cursor-interaction" data-sf-path="D:\Others"><i class="sf-category-dot" style="--sf-item-color:var(--sf-others)" aria-hidden="true"></i><span class="sf-place-name">Others</span><small>14 GB</small></button>
    <div class="sf-source-footer"><button type="button" class="sf-menu-item cursor-interaction" data-sf-action="scan"><i data-lucide="folder-plus" aria-hidden="true"></i>选择位置扫描…</button></div>
  </div>
  <div class="sf-popover sf-action-menu" id="sf-mac-menu" role="group" aria-label="所选项目操作" hidden>
    <div class="sf-menu-title" data-sf-menu-title>AI Models</div>
    <button type="button" class="sf-menu-item cursor-interaction" data-sf-file="open"><i data-lucide="folder-open" aria-hidden="true"></i>打开所在位置</button>
    <button type="button" class="sf-menu-item cursor-interaction" data-sf-file="copy"><i data-lucide="copy" aria-hidden="true"></i>复制路径</button>
    <div class="sf-menu-divider"></div>
    <button type="button" class="sf-menu-item cursor-interaction" data-sf-file="move"><i data-lucide="folder-input" aria-hidden="true"></i>移动到…</button>
    <button type="button" class="sf-menu-item cursor-interaction" data-sf-file="rename"><i data-lucide="text-cursor-input" aria-hidden="true"></i>重命名…</button>
    <button type="button" class="sf-menu-item cursor-interaction" data-sf-file="properties"><i data-lucide="info" aria-hidden="true"></i>属性</button>
    <div class="sf-menu-divider"></div>
    <button type="button" class="sf-menu-item cursor-interaction" data-sf-file="trash"><i data-lucide="trash-2" aria-hidden="true"></i>移至回收站…</button>
  </div>
  <div id="sf-appearance-menu" class="sf-popover sf-appearance-menu" role="group" aria-labelledby="sf-appearance-title" hidden>
    <div class="sf-appearance-heading"><h2 id="sf-appearance-title">外观</h2><button type="button" class="sf-appearance-close cursor-interaction" data-sf-settings-close aria-label="关闭外观设置"><i data-lucide="x" aria-hidden="true"></i></button></div>
    <div class="sf-accent-label"><span>强调色</span><span data-sf-accent-name>紫色</span></div>
    <div class="sf-accent-options" role="group" aria-label="选择强调色">
      <button type="button" class="sf-accent-option cursor-interaction" data-sf-accent-choice="purple" aria-pressed="true"><span class="sf-accent-dot" style="--sf-swatch:light-dark(#855cca,#b397e8)"><i data-lucide="check" aria-hidden="true"></i></span><span>紫色</span></button>
      <button type="button" class="sf-accent-option cursor-interaction" data-sf-accent-choice="blue" aria-pressed="false"><span class="sf-accent-dot" style="--sf-swatch:light-dark(#397ad2,#83ace5)"><i data-lucide="check" aria-hidden="true"></i></span><span>蓝色</span></button>
      <button type="button" class="sf-accent-option cursor-interaction" data-sf-accent-choice="teal" aria-pressed="false"><span class="sf-accent-dot" style="--sf-swatch:light-dark(#2b979d,#7dc3c5)"><i data-lucide="check" aria-hidden="true"></i></span><span>青色</span></button>
      <button type="button" class="sf-accent-option cursor-interaction" data-sf-accent-choice="amber" aria-pressed="false"><span class="sf-accent-dot" style="--sf-swatch:light-dark(#c0884b,#d9b07a)"><i data-lucide="check" aria-hidden="true"></i></span><span>橙色</span></button>
      <button type="button" class="sf-accent-option cursor-interaction" data-sf-accent-choice="rose" aria-pressed="false"><span class="sf-accent-dot" style="--sf-swatch:light-dark(#bc6593,#dba2c1)"><i data-lucide="check" aria-hidden="true"></i></span><span>玫红</span></button>
    </div>
    <p class="sf-accent-description">用于选中项、操作按钮与文件夹图标</p>
  </div>
  <div class="sf-live" aria-live="polite" data-sf-live></div>
</div>
<script>
(()=>{
  const BS=String.fromCharCode(92);
  const node=(name,size,children=[])=>({name,size,children,type:/\.(safetensors|gguf|bin)$/i.test(name)?'file':'folder'});
  const disk=node('D:',742,[
    node('AI Models',286,[
      node('Checkpoints',168,[node('image-base.safetensors',64),node('portrait-v2.safetensors',48),node('archive',32),node('reference.safetensors',24)]),
      node('LLM',78,[node('vision-model.gguf',36),node('language-model.gguf',28),node('embeddings',14)]),
      node('LoRA',24,[node('Characters',15),node('Styles',9)]),node('VAE',16)
    ]),
    node('Games',218,[node('SteamLibrary',166),node('Epic Games',42),node('Saves',10)]),
    node('Media',132,[node('Videos',88),node('Photos',30),node('Audio',14)]),
    node('Projects',64,[node('Workspace',39),node('Build cache',18),node('Design',7)]),
    node('Downloads',28,[node('Installers',17),node('Archives',8),node('Temporary',3)]),
    node('Others',14,[node('Backups',10),node('Misc',4)])
  ]);
  const fieldLabels={name:'名称',path:'完整路径',size:'占用 GB',type:'类型',parent_path:'父级路径',depth:'层级'};
  let active=null,serial=0;
  const esc=value=>String(value??'').replace(/[&<>"']/g,ch=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
  const icon=name=>'<i data-lucide="'+name+'" aria-hidden="true"></i>';
  const icons=()=>{if(window.lucide)window.lucide.createIcons({attrs:{width:16,height:16}});};
  const pathOf=value=>{
    let path=String(value||'D:'+BS).trim().replace(/\//g,BS).replace(/\\+/g,BS);
    path=path.replace(/\\+$/,'');
    if(/^[A-Za-z]:$/.test(path))return path+BS;
    return path||'D:'+BS;
  };
  const keyOf=value=>pathOf(value).toLowerCase();
  const sizeOf=value=>Number.isFinite(Number(value))?Number(value):0;
  const indexed=new Map(),allNodes=[];
  function indexTree(item,parent=null){
    item.path=parent?pathOf(parent.path+BS+item.name):'D:'+BS;
    item.parent_path=parent?parent.path:'';item.depth=parent?parent.depth+1:0;item.id=item.path;
    indexed.set(keyOf(item.path),item);allNodes.push(item);
    item.children.forEach(child=>indexTree(child,item));
  }
  indexTree(disk);
  const rowOf=r=>{
    const path=pathOf(r.path),known=indexed.get(keyOf(path)),type=known?.type||r.type||(/\.[a-z0-9]{2,15}$/i.test(r.name||'')?'file':'folder');
    const parent=path.includes(BS)?pathOf(path.slice(0,path.lastIndexOf(BS))):'';
    return {name:String(r.name||known?.name||'所选项目'),path,size:sizeOf(r.size??known?.size),type:['file','文件'].includes(type)?'文件':'文件夹',parent_path:known?.parent_path??r.parent_path??parent,depth:known?.depth??r.depth??Math.max(0,path.split(BS).filter(Boolean).length-1)};
  };
  const within=(path,ancestor)=>keyOf(path)===keyOf(ancestor)||keyOf(path).startsWith(keyOf(ancestor).replace(/\\$/,'')+BS);
  function normalizeRows(rows){
    const unique=[...new Map(rows.map(row=>[keyOf(row.path),rowOf(row)])).values()];
    return unique.filter(row=>!unique.some(other=>keyOf(other.path)!==keyOf(row.path)&&within(row.path,other.path)));
  }
  function flattenRows(rows,recursive){
    const result=[],seen=new Set();
    function add(row){
      const key=keyOf(row.path);if(seen.has(key))return;seen.add(key);result.push(rowOf(row));
      if(recursive)(indexed.get(key)?.children||[]).forEach(add);
    }
    rows.forEach(add);return result;
  }
  const totalRows=rows=>normalizeRows(rows).reduce((sum,row)=>sum+row.size,0);
  window.diskConceptData={root:disk,nodes:allNodes,get:path=>indexed.get(keyOf(path)),normalizeRows,flattenRows};
  function close(root,restore=true){
    if(!active||(root&&active.root!==root))return;
    const old=active;active=null;old.layer.remove();if(restore&&old.origin&&old.origin.isConnected)old.origin.focus();
  }
  function toast(root,message){
    root.querySelectorAll('.disk-action-toast').forEach(node=>node.remove());
    if(getComputedStyle(root).position==='static')root.style.position='relative';
    const node=document.createElement('div');node.className='disk-action-toast';node.setAttribute('role','status');node.innerHTML=icon('circle-check')+'<span>'+esc(message)+'</span>';root.append(node);icons();setTimeout(()=>node.remove(),3500);
  }
  function button(text,extra='',attrs=''){return '<button type="button" class="cursor-interaction '+extra+'" '+attrs+'>'+text+'</button>';}
  function object(p){
    const rows=p.actionRows||[],batch=rows.length>1;
    const paths=batch?rows.map(row=>esc(row.path)).join('<br>'):esc(p.path);
    return '<div class="da-object">'+icon(batch?'folders':/\.[a-z0-9]+$/i.test(p.name)?'file':'folder')+'<div><strong>'+esc(p.name)+' <span style="font-weight:400;color:var(--da-muted)">· '+p.size+' GB</span></strong><div class="da-path">'+paths+'</div></div></div>';
  }
  function shell(root,title,body,footer,compact=false){
    const oldOrigin=active?.origin;close(undefined,false);
    if(getComputedStyle(root).position==='static')root.style.position='relative';
    const layer=document.createElement('div');layer.className='disk-dialog-layer';
    const uid='disk-action-'+(++serial);
    layer.innerHTML='<div class="disk-dialog '+(compact?'da-compact':'')+'" role="dialog" aria-modal="true" aria-labelledby="'+uid+'"><div class="da-head"><div class="da-title-wrap"><h2 class="da-title" id="'+uid+'">'+esc(title)+'</h2><span class="da-demo">模拟数据</span></div>'+button(icon('x'),'da-close','data-da-close aria-label="关闭"')+'</div><div class="da-body">'+body+'</div><div class="da-footer">'+footer+'</div></div>';
    active={root,layer,origin:oldOrigin||document.activeElement};(root.closest('#spacefold-mac')||root).append(layer);
    layer.querySelectorAll('[data-da-close]').forEach(node=>node.addEventListener('click',()=>close(root)));
    layer.addEventListener('click',event=>{if(event.target===layer)close(root);});
    layer.addEventListener('keydown',event=>{
      if(event.key==='Escape'){event.preventDefault();close(root);}
      if(event.key==='Tab'){
        const focusable=[...layer.querySelectorAll('button:not(:disabled),input:not(:disabled),select:not(:disabled),textarea:not(:disabled),a[href]')].filter(node=>node.offsetParent!==null);
        const first=focusable[0],last=focusable[focusable.length-1];
        if(event.shiftKey&&document.activeElement===first){event.preventDefault();last?.focus();}
        else if(!event.shiftKey&&document.activeElement===last){event.preventDefault();first?.focus();}
      }
    });
    icons();
    requestAnimationFrame(()=>{if(active?.layer===layer)(layer.querySelector('input[type="text"],select')||layer.querySelector('button'))?.focus();});
    return layer;
  }
  function finish(root,p,description){
    const layer=shell(root,'操作预演','<div class="da-success"><div class="da-success-icon">'+icon('check')+'</div><h3>演示操作已预演</h3><p>'+esc(description)+'</p></div>',button('完成','da-primary','data-da-close'),true);
    layer.querySelector('.da-body').setAttribute('aria-live','polite');
    root.dispatchEvent(new CustomEvent('disk-concept-action',{detail:{...p,simulation:true}}));
  }
  function exportDialog(root,p){
    let format='csv',scope=p.exportScope==='selected'?'selected':'current',recursive=true;
    const fields=new Set(['name','path','size','type']);
    const scopePath=pathOf(p.scope||disk.path),scopeNode=indexed.get(keyOf(scopePath));
    const currentRows=normalizeRows((Array.isArray(p.rows)&&p.rows.length?p.rows:scopeNode?.children||[]).map(rowOf));
    const selectedRows=normalizeRows((Array.isArray(p.selectedRows)?p.selectedRows:p.count===0?[]:[p]).map(rowOf));
    const body='<div class="da-formats" aria-label="导出格式">'+[['csv','CSV','.csv'],['xlsx','Excel','.xlsx'],['json','JSON','.json'],['html','HTML','.html']].map(([value,label,ext])=>button(label+'<small>'+ext+'</small>','da-format','data-format="'+value+'" aria-pressed="'+(value===format)+'"')).join('')+'</div><label class="da-field">导出范围<select data-export-scope><option value="full">完整扫描 · D:'+BS+' · 包含所有子目录和文件</option><option value="current" selected>当前目录 · '+esc(scopePath)+'</option><option value="selected">已选项目 · '+selectedRows.length+' 项</option></select></label><div class="da-checks"><label class="da-check cursor-interaction"><input type="checkbox" checked data-export-recursive>包含子目录</label></div><span class="da-field-title">包含字段</span><div class="da-checks">'+Object.entries(fieldLabels).map(([key,label])=>'<label class="da-check cursor-interaction"><input type="checkbox" '+(fields.has(key)?'checked ':'')+'value="'+key+'" data-export-field>'+label+'</label>').join('')+'</div><div class="da-preview"><div class="da-preview-head"><span data-preview-name>scan-report.csv</span><span data-preview-meta></span></div><div class="da-preview-body" data-preview-body></div></div><p class="da-note" style="margin-top:8px;margin-bottom:0">容量包含下级内容；汇总已扣除父子目录的重复占用。</p><div class="da-export-result" role="status" aria-live="polite" data-export-result></div>';
    const layer=shell(root,'导出扫描数据',body,'<span class="da-footer-note">预览使用演示数据</span>'+button('取消','','data-da-close')+button(icon('file-check')+'生成导出预览','da-primary','data-export-generate'));
    if(scope==='selected'&&!selectedRows.length)scope='current';
    layer.querySelector('[data-export-scope]').value=scope;
    function getBaseRows(){return scope==='full'?disk.children:scope==='selected'?selectedRows:currentRows;}
    function getRows(){return flattenRows(getBaseRows(),recursive);}
    function update(){
      const chosen=[...fields],rows=getRows(),total=scope==='full'?disk.size:scope==='current'?sizeOf(p.scopeSize??scopeNode?.size??totalRows(currentRows)):totalRows(selectedRows);
      layer.querySelectorAll('[data-format]').forEach(node=>node.setAttribute('aria-pressed',String(node.dataset.format===format)));
      layer.querySelector('[data-preview-name]').textContent='scan-report.'+format+(format==='xlsx'?' · 工作表模拟预览':format==='html'?' · 报告预览':'');
      layer.querySelector('[data-preview-meta]').textContent=rows.length+' 条记录 · '+Number(total.toFixed(2))+' GB';
      layer.querySelector('option[value="full"]').textContent='完整扫描 · D:'+BS+' · '+(recursive?'包含所有子目录和文件':'仅顶层目录');
      const preview=layer.querySelector('[data-preview-body]');
      if(!chosen.length){preview.innerHTML='<p class="da-note">至少选择一个导出字段。</p>';}
      else if(format==='csv'){
        const quote=value=>'"'+String(value).replace(/"/g,'""')+'"';
        preview.innerHTML='<pre>'+esc([chosen.map(key=>quote(fieldLabels[key])).join(','),...rows.map(row=>chosen.map(key=>quote(row[key])).join(','))].join('\n'))+'</pre>';
      }else if(format==='json'){
        preview.innerHTML='<pre>'+esc(JSON.stringify(rows.map(row=>Object.fromEntries(chosen.map(key=>[key,row[key]]))),null,2))+'</pre>';
      }else{
        preview.innerHTML='<table><thead><tr>'+chosen.map(key=>'<th>'+esc(fieldLabels[key])+'</th>').join('')+'</tr></thead><tbody>'+rows.map(row=>'<tr>'+chosen.map(key=>'<td>'+esc(row[key])+'</td>').join('')+'</tr>').join('')+'</tbody></table>';
      }
      layer.querySelector('[data-export-generate]').disabled=!chosen.length||!rows.length;
      layer.querySelector('[data-export-generate]').innerHTML=icon('file-check')+'生成导出预览';
      layer.querySelector('[data-export-result]').textContent='';
      icons();
    }
    layer.querySelectorAll('[data-format]').forEach(node=>node.addEventListener('click',()=>{format=node.dataset.format;update();}));
    layer.querySelector('[data-export-scope]').addEventListener('change',event=>{scope=event.target.value;update();});
    layer.querySelector('[data-export-recursive]').addEventListener('change',event=>{recursive=event.target.checked;update();});
    layer.querySelectorAll('[data-export-field]').forEach(node=>node.addEventListener('change',()=>{if(node.checked)fields.add(node.value);else fields.delete(node.value);update();}));
    layer.querySelector('[data-export-generate]').addEventListener('click',()=>{
      layer.querySelector('[data-export-result]').textContent='预览已生成 · '+getRows().length+' 项 · '+(format==='xlsx'?'Excel 工作表模拟，未创建实际文件':'演示数据，未创建实际文件');
      layer.querySelector('[data-export-generate]').innerHTML=icon('check')+'预览已生成';icons();
    });
    layer.querySelector('option[value="selected"]').disabled=!selectedRows.length;
    update();
  }
  function open(root,payload={}){
    if(!root||!root.append)return;
    const p={...payload,name:String(payload.name||'AI Models'),path:pathOf(payload.path||'D:'+BS+'AI Models'),size:sizeOf(payload.size??286),scope:payload.scope?pathOf(payload.scope):undefined};
    p.actionRows=normalizeRows(Array.isArray(payload.selectedRows)?payload.selectedRows:Array.isArray(payload.rows)&&payload.count>1?payload.rows:[p]);
    if(!['export','scan'].includes(p.kind)&&p.actionRows.length){
      p.count=p.actionRows.length;p.size=totalRows(p.actionRows);
      p.name=p.count>1?'已选 '+p.count+' 项':p.actionRows[0].name;
      p.path=p.count===1?p.actionRows[0].path:p.path;
    }
    if(p.kind==='export'){exportDialog(root,p);return;}
    if(p.kind==='menu'){
      const actions=[['open','folder-open','打开所在位置'],['copy','copy','复制路径'],['move','folder-input','移动到…'],['rename','text-cursor-input','重命名'],['properties','info','属性'],['trash','trash-2','移至回收站']];
      const layer=shell(root,'文件操作',object(p)+'<div class="da-menu">'+actions.map(([kind,ico,label])=>button(icon(ico)+label,'','data-kind="'+kind+'"'+(p.actionRows.length>1&&['open','rename'].includes(kind)?' disabled':''))).join('')+'</div>',button('关闭','','data-da-close'),true);
      layer.querySelectorAll('[data-kind]').forEach(node=>node.addEventListener('click',()=>open(root,{...p,kind:node.dataset.kind})));return;
    }
    if(p.kind==='scan'){
      const layer=shell(root,'选择扫描位置','<label class="da-field">扫描范围<select data-scan-type><option value="disk">整个磁盘</option><option value="folder">指定目录</option></select></label><label class="da-field">演示位置<input type="text" data-scan-path value="'+esc('D:'+BS)+'"></label><p class="da-note">选择位置后，预览该范围的扫描入口。</p><p class="da-error" role="alert" data-error></p>',button('取消','','data-da-close')+button('预演扫描','da-primary','data-da-confirm'),true);
      layer.querySelector('[data-scan-type]').addEventListener('change',event=>{layer.querySelector('[data-scan-path]').value=event.target.value==='disk'?'D:'+BS:'D:'+BS+'AI Models';});
      layer.querySelector('[data-da-confirm]').addEventListener('click',()=>{
        const value=layer.querySelector('[data-scan-path]').value.trim();
        if(!value){layer.querySelector('[data-error]').textContent='请输入一个演示路径。';return;}
        const scanPath=pathOf(value),scanNode=indexed.get(keyOf(scanPath));
        if(!scanNode||scanNode.type==='file'){layer.querySelector('[data-error]').textContent='请选择演示数据中的目录，例如 D:'+BS+'、D:'+BS+'AI Models 或 D:'+BS+'Downloads。';return;}
        close(root);toast(root,'已选择演示位置 '+scanPath+' · 扫描入口已预演');
        root.dispatchEvent(new CustomEvent('disk-concept-scan',{detail:{path:scanPath,simulation:true},bubbles:true}));
      });return;
    }
    if(p.kind==='rename'){
      if(p.actionRows.length>1){toast(root,'请选择单个项目进行重命名');return;}
      const layer=shell(root,'重命名',object(p)+'<label class="da-field">新名称<input type="text" data-new-name value="'+esc(p.name)+'" maxlength="150"></label><p class="da-error" role="alert" data-error></p>',button('取消','','data-da-close')+button('预演重命名','da-primary','data-da-confirm'),true);
      layer.querySelector('[data-da-confirm]').addEventListener('click',()=>{
        const name=layer.querySelector('[data-new-name]').value.trim();
        if(!name||/[<>:"/\\|?*]/.test(name)||/[. ]$/.test(name)){layer.querySelector('[data-error]').textContent='请输入有效名称，避免特殊符号和结尾句点。';return;}
        finish(root,{...p,newName:name},p.name+' → '+name+' · 名称更改已预演');
      });return;
    }
    if(p.kind==='move'){
      const layer=shell(root,'移动到',object(p)+'<label class="da-field">目标目录<input type="text" data-move-target value="'+esc('D:'+BS+'Archive')+'"></label><p class="da-note">保留目录结构，将整个所选项目移到目标位置。</p><p class="da-error" role="alert" data-error></p>',button('取消','','data-da-close')+button('预演移动','da-primary','data-da-confirm'),true);
      layer.querySelector('[data-da-confirm]').addEventListener('click',()=>{
        const rawTarget=layer.querySelector('[data-move-target]').value.trim();
        if(!rawTarget){layer.querySelector('[data-error]').textContent='请输入目标目录。';return;}
        const target=pathOf(rawTarget);
        if(!/^[A-Za-z]:\\/.test(target)){layer.querySelector('[data-error]').textContent='请输入演示目录，例如 D:'+BS+'Archive';return;}
        const conflicting=p.actionRows.find(row=>within(target,row.path));
        if(conflicting){layer.querySelector('[data-error]').textContent='目标目录应位于所选项目之外：'+conflicting.path;return;}
        const targetNode=indexed.get(keyOf(target));
        if(targetNode?.type==='file'){layer.querySelector('[data-error]').textContent='目标位置是文件，请选择目录。';return;}
        finish(root,{...p,target},p.name+' → '+target+' · '+p.size+' GB 移动操作已预演');
      });return;
    }
    if(p.kind==='trash'){
      const layer=shell(root,'移至回收站',object(p)+'<p class="da-note">项目将进入回收站，可在清空前还原。移入回收站本身不会释放磁盘空间。</p>',button('取消','','data-da-close')+button(icon('trash-2')+'预演移至回收站','da-danger','data-da-confirm'),true);
      layer.querySelector('[data-da-confirm]').addEventListener('click',()=>finish(root,p,p.name+' · 移入回收站已预演，磁盘可用空间保持不变'));return;
    }
    if(p.kind==='properties'){
      shell(root,'项目属性',object(p)+'<dl class="da-kv"><dt>占用空间</dt><dd>'+p.size+' GB</dd><dt>占扫描结果</dt><dd>'+(p.size/742*100).toFixed(1)+'%</dd><dt>扫描位置</dt><dd>D:'+BS+'</dd><dt>数据来源</dt><dd>界面演示数据</dd></dl>',button('完成','da-primary','data-da-close'),true);return;
    }
    if(p.kind==='copy'){
      const paths=(p.actionRows.length?p.actionRows:[p]).map(row=>row.path);
      const field=paths.length>1?'<textarea readonly rows="'+Math.min(6,paths.length+1)+'">'+esc(paths.join('\n'))+'</textarea>':'<input type="text" readonly value="'+esc(paths[0])+'">';
      const layer=shell(root,'复制路径',object(p)+'<label class="da-field">完整路径'+field+'</label>',button('取消','','data-da-close')+button('预演复制','da-primary','data-da-confirm'),true);
      layer.querySelector('[data-da-confirm]').addEventListener('click',()=>finish(root,p,'复制 '+paths.length+' 个路径的操作已预演 · '+paths.join(' · ')));return;
    }
    const layer=shell(root,'打开所在位置',object(p)+'<p class="da-note">打开所选位置并定位当前项目。</p>',button('取消','','data-da-close')+button(icon('external-link')+'预演打开','da-primary','data-da-confirm'),true);
    layer.querySelector('[data-da-confirm]').addEventListener('click',()=>finish(root,p,'打开位置已预演 · '+p.path));
  }
  window.diskConceptActions={open,close,toast};
})();
</script>
<script>
(()=>{
const root=document.getElementById('disk-map');
const palette=['var(--sf-ai,light-dark(#9181c7,#ab9bdf))','var(--sf-games,light-dark(#67a9b8,#85bdca))','var(--sf-media,light-dark(#ce958a,#d8aba2))','var(--sf-projects,light-dark(#c6aa7b,#cdb68f))','var(--sf-downloads,light-dark(#98ad77,#acbf8e))','var(--sf-others,light-dark(#a1a2ad,#a6a7b3))'];
function node(name,size,index,children=[]){return {name,size,index,children,type:/\.(safetensors|gguf|bin)$/i.test(name)?'file':'folder'};}
const models=node('AI Models',286,0,[node('Checkpoints',168,0,[node('image-base.safetensors',64,0),node('portrait-v2.safetensors',48,0),node('archive',32,0),node('reference.safetensors',24,0)]),node('LLM',78,0,[node('vision-model.gguf',36,0),node('language-model.gguf',28,0),node('embeddings',14,0)]),node('LoRA',24,0,[node('Characters',15,0),node('Styles',9,0)]),node('VAE',16,0)]);
const disk=node('D:',742,5,[models,node('Games',218,1,[node('SteamLibrary',166,1),node('Epic Games',42,1),node('Saves',10,1)]),node('Media',132,2,[node('Videos',88,2),node('Photos',30,2),node('Audio',14,2)]),node('Projects',64,3,[node('Workspace',39,3),node('Build cache',18,3),node('Design',7,3)]),node('Downloads',28,4,[node('Installers',17,4),node('Archives',8,4),node('Temporary',3,4)]),node('Others',14,5,[node('Backups',10,5),node('Misc',4,5)])]);
const BS=String.fromCharCode(92),pathIndex=new Map();
function pathKey(path){const p=String(path||'').replace(/\//g,BS).replace(/\\+/g,BS).replace(/\\+$/,'');return p.toLocaleLowerCase();}
function indexNode(n,parent=null){n.parent=parent;n.path=parent?parent.path+(parent===disk?'':BS)+n.name:'D:'+BS;pathIndex.set(pathKey(n.path),n);n.children.forEach(child=>indexNode(child,n));}
indexNode(disk);
const findNode=path=>pathIndex.get(pathKey(path));
function trailOf(n){const trail=[];for(let current=n;current;current=current.parent)trail.unshift(current);return trail;}
let stack=[disk],selected=models;
const map=root.querySelector('.dm-map'),crumb=root.querySelector('.dm-breadcrumb');
const icon=name=>'<i data-lucide="'+name+'" aria-hidden="true"></i>';
const icons=()=>{if(globalThis.lucide)lucide.createIcons({attrs:{width:16,height:16}});};
const escape=s=>String(s).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const scope=()=>stack[stack.length-1];
const basePath=()=>scope().path;
const selectedPath=()=>selected.path;
const rowOf=n=>({name:n.name,path:n.path,size:n.size,type:n.type});
function getState(){return {scopePath:basePath(),selectedPaths:[selectedPath()]};}
function getActionContext(){return {...rowOf(selected),scope:basePath(),scopeSize:scope().size,rows:scope().children.map(rowOf),selectedRows:[rowOf(selected)],total:742,capacity:1000,free:258};}
function setState(state={}){let nextScope=findNode(state.scopePath)||disk;if(!nextScope.children.length)nextScope=nextScope.parent||disk;const nextSelected=findNode(state.selectedPaths?.[0])||nextScope.children[0]||nextScope;if(nextSelected!==nextScope&&nextSelected.parent!==nextScope)nextScope=nextSelected.parent||disk;stack=trailOf(nextScope);selected=nextSelected;render();return getState();}
function rectangles(items,x,y,w,h){if(!items.length)return[];if(items.length===1)return[{node:items[0],x,y,w,h}];const total=items.reduce((s,n)=>s+n.size,0);let sum=0,k=1,best=Infinity;for(let i=0;i<items.length-1;i++){sum+=items[i].size;const delta=Math.abs(total/2-sum);if(delta<best){best=delta;k=i+1;}}const a=items.slice(0,k),b=items.slice(k),f=a.reduce((s,n)=>s+n.size,0)/total;if(w>=h)return [...rectangles(a,x,y,w*f,h),...rectangles(b,x+w*f,y,w*(1-f),h)];return [...rectangles(a,x,y,w,h*f),...rectangles(b,x,y+h*f,w,h*(1-f))];}
function viewChanged(){root.dispatchEvent(new CustomEvent('disk-view-change',{bubbles:true,detail:{view:'map'}}));}
function updateDetails(){
  const cur=scope();
  root.querySelector('.dm-selected-name').textContent=selected.name;
  root.querySelector('.dm-path').textContent=selectedPath();
  root.querySelector('.dm-path').dataset.tooltip=selectedPath();
  root.querySelector('.dm-selected-size').innerHTML=selected.size+' <span>GB</span>';
  root.querySelector('.dm-selected-ratio').textContent=(selected.size/cur.size*100).toFixed(1)+'%';
  root.querySelector('.dm-selected-children').textContent=selected.children.length?selected.children.length+' 个':selected.type==='file'?'文件':'—';
  root.querySelector('[data-map-action="enter"]').disabled=!selected.children.length;
  root.querySelector('.dm-folder-icon').innerHTML=icon(selected.type==='file'?'file':'folder');
  root.querySelectorAll('[data-map-action="move"],[data-map-action="rename"],[data-map-action="trash"]').forEach(b=>b.disabled=selected===disk);
  map.querySelectorAll('.dm-tile').forEach(b=>b.setAttribute('aria-pressed',b.dataset.path===selectedPath()?'true':'false'));
  const children=root.querySelector('[data-map-children]'),childList=children.querySelector('.dm-child-list'),parent=selected;
  children.hidden=!parent.children.length;childList.replaceChildren();
  parent.children.slice().sort((a,b)=>b.size-a.size).slice(0,4).forEach(child=>{
    const button=document.createElement('button');button.type='button';button.className='dm-child cursor-interaction';button.dataset.mapChild=child.path;
    button.setAttribute('aria-label','查看 '+child.name+'，'+child.size+' GB');button.dataset.tooltip=child.name;
    button.innerHTML='<span class="dm-child-name">'+escape(child.name)+'</span><span class="dm-child-size">'+child.size+' GB</span>'+icon('chevron-right');
    button.onclick=()=>{stack=trailOf(parent);selected=child;render();};childList.append(button);
  });
  icons();
}
function select(n){selected=n;updateDetails();root.querySelector('.dm-live').textContent='已选中 '+n.name+'，'+n.size+' GB';viewChanged();}
function enter(n){if(!n.children.length){select(n);window.diskConceptActions?.open(root,{...getActionContext(),kind:'properties'});return;}stack=trailOf(n);selected=n.children[0]||n;render();}
function render(){const cur=scope();crumb.innerHTML='<button class="dm-back cursor-interaction" type="button" aria-label="返回上一级" '+(stack.length===1?'disabled':'')+'>'+icon('arrow-left')+'</button>'+stack.map((n,i)=>'<button type="button" class="cursor-interaction" data-depth="'+i+'" data-tooltip="'+escape(n.path)+'" '+(n===cur?'aria-current="location"':'')+'>'+escape(n.name)+'</button>'+(i<stack.length-1?icon('chevron-right'):'')).join('')+'<span class="dm-area-label">'+cur.size+' GB</span>';crumb.querySelector('.dm-back').onclick=()=>{if(stack.length>1){const prev=stack.pop();selected=prev;render();}};crumb.querySelectorAll('[data-depth]').forEach(b=>b.onclick=()=>{const d=+b.dataset.depth;if(d<stack.length-1){selected=stack[d+1];stack=stack.slice(0,d+1);render();}});draw();root.querySelector('.dm-map-scope').textContent=cur.children.length+' 个项目';updateDetails();root.querySelector('.dm-live').textContent='当前目录 '+cur.path+'，'+cur.size+' GB';icons();viewChanged();}
function draw(){
  const cur=scope(),w=map.clientWidth,h=map.clientHeight;if(!w||!h)return;
  map.innerHTML='';
  rectangles(cur.children,0,0,w,h).forEach(r=>{
    const n=r.node,p=palette[n.index],b=document.createElement('button');
    const compact=r.w<170||r.h<110,micro=r.w<105||r.h<72;
    b.type='button';b.className='dm-tile cursor-interaction'+(compact?' compact':'')+(micro?' micro':'')+(n.type==='file'?' dm-tile-file':'');b.dataset.name=n.name;b.dataset.path=n.path;
    b.setAttribute('aria-label',n.name+'，'+n.size+' GB，占当前目录'+(n.size/cur.size*100).toFixed(1)+'%');b.setAttribute('aria-pressed',n===selected?'true':'false');b.setAttribute('data-tooltip',n.name+' · '+n.size+' GB');
    b.style.cssText='left:'+r.x+'px;top:'+r.y+'px;width:'+Math.max(0,r.w-4)+'px;height:'+Math.max(0,r.h-4)+'px;--dm-tile-color:'+p+';--dm-tile-bg:color-mix(in srgb,var(--dm-tile-color) 13%,var(--dm-bg));--dm-tile-ink:var(--dm-ink)';
    b.innerHTML='<span class="dm-tile-heading"><span class="dm-tile-name">'+escape(n.name)+'</span><span class="dm-tile-size">'+n.size+' <small>GB</small></span></span><span class="dm-tile-detail">'+(n.size/cur.size*100).toFixed(1)+'%'+(n.children.length?' · '+n.children.length+' 个项目':'')+'</span>';
    if(n.children.length&&!micro){
      const sub=document.createElement('span');sub.className='dm-submap';sub.setAttribute('aria-hidden','true');b.append(sub);
      const cw=Math.max(1,r.w-4-(compact?12:14)),ch=Math.max(1,r.h-4-(compact?58:68));
      rectangles(n.children,0,0,cw,ch).forEach((child,i)=>{
        const el=document.createElement('span');el.className='dm-subtile';
        el.style.cssText='left:'+(child.x/cw*100)+'%;top:'+(child.y/ch*100)+'%;width:calc('+(child.w/cw*100)+'% - 3px);height:calc('+(child.h/ch*100)+'% - 3px);--dm-sub-opacity:'+(21+(i%4)*6)+'%';
        const readable=child.w>70&&child.h>45;
        if(readable)el.innerHTML='<span class="dm-subtile-name">'+escape(child.node.name)+'</span><span class="dm-subtile-size">'+child.node.size+' GB</span>';
        sub.append(el);
      });
    }
    b.onclick=()=>select(n);b.ondblclick=()=>enter(n);b.oncontextmenu=e=>{e.preventDefault();select(n);window.diskConceptActions?.open(root,{...getActionContext(),kind:'menu'});};map.append(b);
  });
  icons();
}
root.querySelectorAll('[data-map-action]').forEach(b=>b.onclick=()=>{const kind=b.dataset.mapAction;if(kind==='enter'){enter(selected);return;}if(kind==='refresh'){root.querySelector('.dm-live').textContent='扫描中… 正在更新目录占用';b.disabled=true;setTimeout(()=>{root.querySelector('.dm-live').textContent='扫描完成 · 742 GB · 已刷新模拟数据';b.disabled=false;},650);return;}window.diskConceptActions?.open(root,{...getActionContext(),kind,exportScope:kind==='export'?'selected':undefined});});
window.diskViewAdapters=window.diskViewAdapters||{};
window.diskViewAdapters.map={getState,setState,getActionContext};
new ResizeObserver(()=>draw()).observe(map);render();
})();
</script>
<script>
(() => {
  const root = document.getElementById('disk-workbench');
  if (!root) return;
  const $ = selector => root.querySelector(selector);
  const icon = name => '<i data-lucide="' + name + '" aria-hidden="true"></i>';
  const escape = value => String(value).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  const activateIcons = () => { if (globalThis.lucide) globalThis.lucide.createIcons({ attrs: { width: 16, height: 16 } }); };
  function node(name, size, index, children = []) { return { name, size, index, children, type: /\.(safetensors|gguf|bin)$/i.test(name) ? 'file' : 'folder' }; }
  const models = node('AI Models', 286, 0, [node('Checkpoints', 168, 0, [node('image-base.safetensors', 64, 0), node('portrait-v2.safetensors', 48, 0), node('archive', 32, 0), node('reference.safetensors', 24, 0)]), node('LLM', 78, 0, [node('vision-model.gguf', 36, 0), node('language-model.gguf', 28, 0), node('embeddings', 14, 0)]), node('LoRA', 24, 0, [node('Characters', 15, 0), node('Styles', 9, 0)]), node('VAE', 16, 0)]);
  const disk = node('D:', 742, 5, [models, node('Games', 218, 1, [node('SteamLibrary', 166, 1), node('Epic Games', 42, 1), node('Saves', 10, 1)]), node('Media', 132, 2, [node('Videos', 88, 2), node('Photos', 30, 2), node('Audio', 14, 2)]), node('Projects', 64, 3, [node('Workspace', 39, 3), node('Build cache', 18, 3), node('Design', 7, 3)]), node('Downloads', 28, 4, [node('Installers', 17, 4), node('Archives', 8, 4), node('Temporary', 3, 4)]), node('Others', 14, 5, [node('Backups', 10, 5), node('Misc', 4, 5)])]);
  const byPath = new Map(), allNodes = [];
  function indexTree(item, parent = null) {
    item.parent = parent;
    item.path = !parent ? 'D:\\' : parent.path + (parent === disk ? '' : '\\') + item.name;
    item.depth = parent ? parent.depth + 1 : 0;
    item.category = !parent ? '工作盘' : parent === disk ? item.name : parent.category;
    byPath.set(item.path.toLowerCase(), item); allNodes.push(item);
    item.children.forEach(child => indexTree(child, item));
  }
  indexTree(disk);
  const find = value => {
    let path = String(value || 'D:\\').replace(/\//g, '\\').replace(/\\+/g, '\\').replace(/\\$/, '');
    if (/^[a-z]:$/i.test(path)) path += '\\';
    return byPath.get(path.toLowerCase());
  };
  const within = (item, ancestor) => { for (let p = item; p; p = p.parent) if (p === ancestor) return true; return false; };
  const normalize = paths => {
    const items = [...new Set(paths)].map(find).filter(Boolean).sort((a, b) => a.depth - b.depth);
    const chosen = [];
    for (const item of items) if (!chosen.some(ancestor => within(item, ancestor))) chosen.push(item);
    return new Set(chosen.map(item => item.path));
  };
  let currentScope = disk;
  let selectedPaths = new Set([models.path]);
  let expanded = new Set([disk.path, models.path, disk.children[1].path]);
  let sortBy = 'size', direction = -1;
  const selectedNodes = () => [...selectedPaths].map(find).filter(Boolean);
  const covered = item => selectedNodes().some(ancestor => within(item, ancestor));
  const partial = item => !covered(item) && selectedNodes().some(child => within(child, item));
  const ordered = item => [...item.children].sort((a, b) => direction * (sortBy === 'size' ? a.size - b.size : a.name.localeCompare(b.name)) || a.name.localeCompare(b.name));
  const rowPayload = item => ({ name: item.name, path: item.path, size: item.size, type: item.type, category: item.category, children: item.children.length });
  function actionContext() {
    const items = selectedNodes();
    return {
      name: items.length === 1 ? items[0].name : items.length ? '已选 ' + items.length + ' 项' : '未选择项目',
      path: items.length === 1 ? items[0].path : currentScope.path,
      size: items.reduce((sum, item) => sum + item.size, 0),
      type: items.length === 1 ? items[0].type : '批量操作',
      scope: currentScope.path, currentScope: currentScope.path, scopeSize: currentScope.size,
      rows: ordered(currentScope).map(rowPayload), selectedRows: items.map(rowPayload),
      count: items.length, source: '目录树'
    };
  }
  function visibleRows() {
    const rows = [];
    function walk(item, depth, continuations, last) {
      rows.push({ item, depth, continuations, last });
      if (!expanded.has(item.path)) return;
      const children = ordered(item);
      children.forEach((child, i) => walk(child, depth + 1, depth ? [...continuations, !last] : [], i === children.length - 1));
    }
    walk(currentScope, 0, [], true);
    return rows;
  }
  function reveal(item) {
    for (let p = item.parent; p; p = p.parent) expanded.add(p.path);
  }
  function ensureScope() {
    const items = selectedNodes();
    while (currentScope.parent && items.some(item => !within(item, currentScope))) currentScope = currentScope.parent;
  }
  function detail() {
    const items = selectedNodes(), count = items.length;
    const total = items.reduce((sum, item) => sum + item.size, 0);
    const hasDescendants = items.some(item => item.children.length);
    const single = count === 1 ? items[0] : null;
    $('[data-wb-inspector]').dataset.wbSelectionMode = count > 1 ? 'multiple' : count ? 'single' : 'empty';
    $('[data-wb-clear]').hidden = !count;
    $('[data-wb-selected-name]').textContent = count === 1 ? (items[0] === disk ? '工作盘 D:' : items[0].name) : count ? '已选 ' + count + ' 项' : '未选择项目';
    $('[data-wb-selected-kind]').textContent = count ? (count > 1 ? '批量选择' : single === disk ? '扫描根目录' : single.type === 'file' ? (single.name.endsWith('.gguf') ? 'GGUF 文件' : '模型文件') : '文件夹') + (hasDescendants ? ' · ' + (count === 1 ? items[0].children.length + ' 个下级项目' : '包含子目录') : '') : '勾选文件或目录';
    $('[data-wb-location-label]').textContent = count === 1 ? '位置' : '当前范围';
    $('[data-wb-selected-path]').textContent = single ? single.path : currentScope.path;
    $('[data-wb-selected-size]').textContent = count ? total + ' GB' : '—';
    $('[data-wb-selected-meta]').textContent = count ? '占当前范围 ' + (total / currentScope.size * 100).toFixed(1) + '%' : '支持跨层级多选';
    root.style.setProperty('--wb-selected-percent', Math.min(100, total / currentScope.size * 100).toFixed(3) + '%');
    $('[data-wb-selected-count]').textContent = count + ' 项已选' + (hasDescendants ? ' · 已包含子目录' : '');
    $('[data-wb-selected-icon]').dataset.kind = single === disk ? 'disk' : single ? single.type : 'folder';
    $('[data-wb-selected-icon]').innerHTML = icon(count > 1 ? 'folders' : single === disk ? 'hard-drive' : single && single.type === 'file' ? 'file' : 'folder-open');
    const breakdown = count > 1 ? items : single ? single.children : [];
    const ranked = [...breakdown].sort((a, b) => b.size - a.size || a.name.localeCompare(b.name));
    const remaining = ranked.slice(4);
    $('[data-wb-breakdown]').hidden = !ranked.length;
    $('[data-wb-breakdown-title]').textContent = count > 1 ? '已选项目' : '下级占用';
    $('[data-wb-focus]').hidden = !single || !single.children.length;
    $('[data-wb-breakdown-list]').innerHTML = ranked.slice(0, 4).map(item => '<button type="button" class="wb-breakdown-item cursor-interaction" data-wb-inspect="' + escape(item.path) + '" data-category="' + escape(item.category) + '" data-tooltip="' + escape(item.path) + '" aria-label="查看 ' + escape(item.name) + '，' + item.size + ' GB" style="--wb-part:' + (item.size / (total || 1) * 100).toFixed(3) + '%"><span class="wb-breakdown-label"><span class="wb-breakdown-name">' + escape(item.name) + '</span><span class="wb-breakdown-size wb-number">' + item.size + ' GB</span></span><span class="wb-breakdown-bar" aria-hidden="true"><span></span></span></button>').join('');
    $('[data-wb-breakdown-more]').hidden = !remaining.length;
    $('[data-wb-breakdown-more]').textContent = remaining.length ? '另外 ' + remaining.length + ' 项 · ' + remaining.reduce((sum, item) => sum + item.size, 0) + ' GB' : '';
    const selectAll = $('[data-wb-select-all]');
    selectAll.checked = !!currentScope.children.length && currentScope.children.every(covered);
    selectAll.indeterminate = !selectAll.checked && items.some(item => within(item, currentScope));
    selectAll.disabled = !currentScope.children.length;
    root.querySelectorAll('.wb-actions [data-wb-action]').forEach(button => {
      const kind = button.dataset.wbAction;
      button.disabled = !count || (count > 1 && ['open', 'rename'].includes(kind)) || (items.includes(disk) && ['move', 'rename', 'trash'].includes(kind));
    });
    root.querySelectorAll('[data-wb-row]').forEach(row => {
      const item = find(row.dataset.wbRow), explicit = selectedPaths.has(item.path), inherited = !explicit && covered(item);
      row.classList.toggle('wb-selected', explicit); row.classList.toggle('wb-covered', inherited);
      row.setAttribute('aria-selected', String(explicit || inherited));
      const checkbox = row.querySelector('[data-wb-check]');
      if (checkbox) { checkbox.checked = covered(item); checkbox.indeterminate = partial(item); }
      row.querySelector('[data-wb-select]').setAttribute('aria-pressed', String(explicit));
      const typeCell = row.querySelector('[data-wb-type]');
      typeCell.innerHTML = explicit && item.children.length ? '<span class="wb-included">已包含子目录</span>' : inherited ? '随上级选中' : item === disk ? '扫描根目录' : item.type === 'file' ? (item.name.endsWith('.gguf') ? 'GGUF 文件' : '模型文件') : item.children.length ? item.children.length + ' 个下级项目' : '文件夹';
    });
    activateIcons();
    root.dispatchEvent(new CustomEvent('disk-view-change', { bubbles: true, detail: { view: 'tree', state: { scopePath: currentScope.path, selectedPaths: [...selectedPaths] } } }));
  }
  function render() {
    const rows = visibleRows();
    $('[data-wb-total]').textContent = currentScope.size + ' GB';
    $('[data-wb-subtitle]').textContent = currentScope.children.length + (currentScope === disk ? ' 个一级目录' : ' 个直接下级项目') + ' · 同层' + (sortBy === 'size' ? (direction === -1 ? '按占用从大到小' : '按占用从小到大') : (direction === 1 ? '按名称 A–Z 排序' : '按名称 Z–A 排序'));
    $('[data-wb-up]').disabled = !currentScope.parent;
    $('[data-wb-status]').textContent = '扫描完成 · ' + currentScope.size + ' GB · ' + rows.length + ' 项可见';
    const ancestors = []; for (let item = currentScope; item; item = item.parent) ancestors.unshift(item);
    $('[data-wb-breadcrumbs]').innerHTML = ancestors.map((item, index) => (index ? icon('chevron-right') : '') + '<button type="button" class="cursor-interaction" data-wb-nav="' + escape(item.path) + '">' + escape(item === disk ? '工作盘 D:' : item.name) + '</button>').join('');
    $('[data-wb-size-head]').setAttribute('aria-sort', sortBy === 'size' ? (direction === -1 ? 'descending' : 'ascending') : 'none');
    $('[data-wb-name-head]').setAttribute('aria-sort', sortBy === 'name' ? (direction === 1 ? 'ascending' : 'descending') : 'none');
    $('[data-wb-size-sort-icon]').innerHTML = sortBy === 'size' ? icon(direction === -1 ? 'arrow-down' : 'arrow-up') : '';
    $('[data-wb-name-sort-icon]').innerHTML = sortBy === 'name' ? icon(direction === 1 ? 'arrow-down-a-z' : 'arrow-up-z-a') : '';
    $('[data-wb-expand]').disabled = !rows.some(({item}) => item.children.length && !expanded.has(item.path));
    $('[data-wb-rows]').innerHTML = rows.map(({item, depth, continuations, last}) => {
      const share = item === currentScope ? 100 : item.size / item.parent.size * 100;
      let guides = '';
      for (let level = 0; level < depth; level++) {
        if (level === depth - 1) guides += '<span class="wb-guide wb-elbow' + (last ? ' wb-last' : '') + '" style="--wb-level:' + level + '" aria-hidden="true"></span>';
        else if (continuations[level]) guides += '<span class="wb-guide" style="--wb-level:' + level + '" aria-hidden="true"></span>';
      }
      const branch = item.children.length > 0, isExpanded = expanded.has(item.path), safePath = escape(item.path);
      return '<tr role="row" aria-level="' + (depth + 1) + '"' + (branch ? ' aria-expanded="' + isExpanded + '"' : '') + ' class="' + (!depth ? 'wb-root-row' : '') + '" data-wb-row="' + safePath + '" data-category="' + escape(item.category) + '" data-wb-node-type="' + item.type + '">' +
        '<td class="wb-check-cell">' + (item === disk ? '' : '<label class="wb-check-hit cursor-interaction"><input class="wb-check cursor-interaction" type="checkbox" data-wb-check="' + safePath + '" aria-label="选择 ' + safePath + (branch ? ' 及其全部下级' : '') + '"></label>') + '</td>' +
        '<td class="wb-name-cell"><div class="wb-node" style="--wb-depth:' + depth + '">' + guides + (branch ? '<button type="button" class="wb-chevron cursor-interaction" data-wb-toggle="' + safePath + '" aria-label="' + (isExpanded ? '折叠 ' : '展开 ') + escape(item.name) + '" aria-expanded="' + isExpanded + '">' + icon(isExpanded ? 'chevron-down' : 'chevron-right') + '</button>' : '<span class="wb-chevron-space" aria-hidden="true"></span>') +
        '<button type="button" class="wb-folder-select cursor-interaction" data-wb-select="' + safePath + '" data-tooltip="' + safePath + '" aria-pressed="false"><span class="wb-folder-icon">' + icon(item === disk ? 'hard-drive' : item.type === 'file' ? 'file' : isExpanded && branch ? 'folder-open' : 'folder') + '</span><span class="wb-folder-text">' + escape(item === disk ? '工作盘 D:' : item.name) + '</span></button></div></td>' +
        '<td class="wb-align-right wb-number wb-folder-size">' + item.size + ' GB</td>' +
        '<td class="wb-share-cell"><div class="wb-share"><span class="wb-share-track"><span style="width:' + share.toFixed(3) + '%"></span></span><span class="wb-share-value wb-number">' + share.toFixed(1) + '%</span></div></td>' +
        '<td class="wb-type-cell" data-wb-type></td><td class="wb-open-cell">' + (item !== currentScope ? '<button type="button" class="wb-enter cursor-interaction" data-wb-enter="' + safePath + '" aria-label="' + (branch ? '聚焦目录 ' : '打开所在位置 ') + safePath + '">' + icon(branch ? 'corner-down-right' : 'external-link') + '</button>' : '') + '</td></tr>';
    }).join('');
    detail();
  }
  function selectSubtree(item, checked) {
    if (checked) {
      selectedPaths.add(item.path);
      selectedPaths = normalize(selectedPaths);
    } else {
      const ancestor = selectedNodes().find(candidate => within(item, candidate));
      if (ancestor) {
        selectedPaths.delete(ancestor.path);
        function preserveOtherBranches(branch) {
          branch.children.forEach(child => {
            if (child === item) return;
            if (within(item, child)) preserveOtherBranches(child);
            else selectedPaths.add(child.path);
          });
        }
        if (ancestor !== item) preserveOtherBranches(ancestor);
      }
      selectedPaths = normalize([...selectedPaths].filter(path => !within(find(path), item)));
    }
    detail();
  }
  function navigate(path) {
    const item = find(path);
    if (!item || !item.children.length) return;
    currentScope = item; expanded.add(item.path);
    selectedPaths = normalize([...selectedPaths].filter(selectedPath => within(find(selectedPath), item)));
    if (!selectedPaths.size) selectedPaths.add(item.path);
    selectedNodes().forEach(reveal); render();
  }
  function openAction(kind) {
    const context = actionContext();
    if (!context.count && !['export', 'scan', 'refresh'].includes(kind)) return;
    if (window.diskConceptActions && typeof window.diskConceptActions.open === 'function') window.diskConceptActions.open(root, { kind, ...context, exportScope: kind === 'export' ? 'selected' : undefined });
    else $('[data-wb-status]').textContent = '演示操作：' + ({export:'导出扫描数据',scan:'选择扫描位置',open:'打开所在位置',move:'移动目录',rename:'重命名',copy:'复制路径',trash:'移到回收站'}[kind] || kind) + ' · ' + context.name;
  }
  root.addEventListener('click', event => {
    const button = event.target.closest('button');
    if (!button || !root.contains(button)) return;
    if (button.hasAttribute('data-wb-clear')) { selectedPaths = new Set(); detail(); (root.dataset.sfInspectorOpen === 'true' ? $('[data-sf-inspector-close]') : $('[data-wb-select-all]')).focus(); return; }
    if (button.hasAttribute('data-wb-action')) return openAction(button.dataset.wbAction);
    if (button.hasAttribute('data-wb-focus')) { const items = selectedNodes(); return items.length === 1 && navigate(items[0].path); }
    if (button.hasAttribute('data-wb-inspect')) { const item = find(button.dataset.wbInspect); if (item) { selectedPaths = new Set([item.path]); reveal(item); render(); } return; }
    if (button.hasAttribute('data-wb-up')) return currentScope.parent && navigate(currentScope.parent.path);
    if (button.hasAttribute('data-wb-nav')) return navigate(button.dataset.wbNav);
    if (button.hasAttribute('data-wb-toggle')) { const path = button.dataset.wbToggle; expanded.has(path) ? expanded.delete(path) : expanded.add(path); render(); return; }
    if (button.hasAttribute('data-wb-select')) { selectedPaths = new Set([button.dataset.wbSelect]); detail(); return; }
    if (button.hasAttribute('data-wb-enter')) {
      const item = find(button.dataset.wbEnter);
      selectedPaths = new Set([item.path]);
      if (item.children.length) navigate(item.path); else { detail(); openAction('open'); }
      return;
    }
    if (button.hasAttribute('data-wb-collapse')) { expanded = new Set([currentScope.path]); render(); return; }
    if (button.hasAttribute('data-wb-expand')) {
      const candidates = visibleRows().filter(({item}) => item.children.length && !expanded.has(item.path));
      const depth = Math.min(...candidates.map(item => item.depth));
      candidates.filter(item => item.depth === depth).forEach(({item}) => expanded.add(item.path)); render(); return;
    }
    if (button.hasAttribute('data-wb-sort')) { const next = button.dataset.wbSort; direction = sortBy === next ? direction * -1 : next === 'size' ? -1 : 1; sortBy = next; render(); }
  });
  root.addEventListener('change', event => {
    const input = event.target;
    if (input.hasAttribute('data-wb-select-all')) {
      selectedPaths = input.checked ? normalize(currentScope.children.map(item => item.path)) : new Set(); detail();
    } else if (input.hasAttribute('data-wb-check')) selectSubtree(find(input.dataset.wbCheck), input.checked);
  });
  root.addEventListener('dblclick', event => {
    const button = event.target.closest('[data-wb-select]');
    if (!button) return;
    const item = find(button.dataset.wbSelect);
    if (item.children.length) { expanded.has(item.path) ? expanded.delete(item.path) : expanded.add(item.path); render(); }
    else openAction('open');
  });
  window.diskViewAdapters = window.diskViewAdapters || {};
  window.diskViewAdapters.tree = {
    getState: () => ({ scopePath: currentScope.path, selectedPaths: [...selectedPaths] }),
    setState: (state = {}) => {
      const requested = find(state.scopePath);
      if (requested) currentScope = requested.type === 'file' ? requested.parent : requested;
      if (Array.isArray(state.selectedPaths)) selectedPaths = normalize(state.selectedPaths);
      ensureScope(); expanded.add(currentScope.path); selectedNodes().forEach(reveal); render();
    },
    getActionContext: actionContext
  };
  render();
})();
</script>
<script>
(() => {
  const root = document.getElementById('disk-lens');
  if (!root) return;
  const BS = String.fromCharCode(92);
  const leaf = (name, size) => ({name, size, children: [], type: /\.(safetensors|gguf)$/.test(name) ? 'file' : 'folder'});
  const disk = {name: 'D: 工作盘', size: 742, path: 'D:' + BS, children: [
    {name: 'AI Models', size: 286, color: 'teal', children: [
      {name: 'Checkpoints', size: 168, children: [leaf('image-base.safetensors', 64), leaf('portrait-v2.safetensors', 48), leaf('archive', 32), leaf('reference.safetensors', 24)]},
      {name: 'LLM', size: 78, children: [leaf('vision-model.gguf', 36), leaf('language-model.gguf', 28), leaf('embeddings', 14)]},
      {name: 'LoRA', size: 24, children: [leaf('Characters', 15), leaf('Styles', 9)]},
      leaf('VAE', 16)
    ]},
    {name: 'Games', size: 218, color: 'amber', children: [leaf('SteamLibrary', 166), leaf('Epic Games', 42), leaf('Saves', 10)]},
    {name: 'Media', size: 132, color: 'blue', children: [leaf('Videos', 88), leaf('Photos', 30), leaf('Audio', 14)]},
    {name: 'Projects', size: 64, color: 'purple', children: [leaf('Workspace', 39), leaf('Build cache', 18), leaf('Design', 7)]},
    {name: 'Downloads', size: 28, color: 'clay', children: [leaf('Installers', 17), leaf('Archives', 8), leaf('Temporary', 3)]},
    {name: 'Others', size: 14, color: 'gray', children: [leaf('Backups', 10), leaf('Misc', 4)]}
  ]};
  let nextId = 0;
  const pathIndex = new Map(), nodesById = new Map();
  const pathKey = path => String(path || '').replace(/\//g, BS).replace(/\\+/g, BS).replace(/\\+$/, '').toLocaleLowerCase();
  function indexNode(node, parent) {
    node.id = 'lens-' + nextId++;
    nodesById.set(node.id, node);
    node.parent = parent || null;
    node.type = node.type || 'folder';
    if (parent) { node.path = parent.path + (parent === disk ? '' : BS) + node.name; node.color = node.color || parent.color; }
    pathIndex.set(pathKey(node.path), node);
    node.children.sort((a, b) => b.size - a.size).forEach(child => indexNode(child, node));
  }
  indexNode(disk);
  let scope = disk;
  let selected = disk.children[0];
  let hovered = null;
  const rowOf = node => ({name: node.name, path: node.path, size: node.size, type: node.type});
  function getState() { return {scopePath: scope.path, selectedPaths: [selected.path]}; }
  function getActionContext() { return {...rowOf(selected), scope: scope.path, scopeSize: scope.size, rows: scope.children.map(rowOf), selectedRows: [rowOf(selected)], total: 742, capacity: 1000, free: 258}; }
  function setState(state = {}) {
    let nextScope = pathIndex.get(pathKey(state.scopePath)) || disk;
    if (!nextScope.children.length) nextScope = nextScope.parent || disk;
    const nextSelected = pathIndex.get(pathKey(state.selectedPaths?.[0])) || nextScope.children[0] || nextScope;
    if (nextSelected !== nextScope && nextSelected.parent !== nextScope) nextScope = nextSelected.parent || disk;
    scope = nextScope; selected = nextSelected; render(); return getState();
  }
  const svg = root.querySelector('.dl-chart');
  const stage = root.querySelector('.dl-chart-stage');
  const list = root.querySelector('[data-lens-folders]');
  const breadcrumb = root.querySelector('[data-lens-breadcrumb]');
  const live = root.querySelector('[data-lens-live]');
  const ns = 'http://www.w3.org/2000/svg';
  function icon(name) { const el = document.createElement('i'); el.dataset.lucide = name; el.setAttribute('aria-hidden', 'true'); return el; }
  function makeButton(label, cls, fn) { const b = document.createElement('button'); b.type = 'button'; b.className = cls + ' cursor-interaction'; b.textContent = label; b.addEventListener('click', fn); return b; }
  function pct(node) { return (node.size / scope.size * 100).toFixed(1); }
  function viewChanged() { root.dispatchEvent(new CustomEvent('disk-view-change', {bubbles:true, detail:{view:'lens'}})); }
  function selectNode(node) { selected = node; updateSelection(); live.textContent = '已选中 ' + node.name + '，' + node.size + ' GB，占当前范围 ' + pct(node) + '%'; viewChanged(); }
  function enterNode(node) {
    if (!node.children.length) { selectNode(node); return; }
    scope = node;
    selected = node.children[0];
    render();
    live.textContent = '当前目录 ' + scope.path + '，' + scope.children.length + ' 个子目录';
  }
  function goParent() { if (scope.parent) { const previous = scope; scope = scope.parent; selected = previous; render(); } }
  function related(a, b) { return a === b || a?.parent === b || b?.parent === a; }
  function hoverNode(node) { if (hovered === node) return; hovered = node; updateHighlights(); }
  function updateHighlights() {
    svg.querySelectorAll('path[data-lens-node]').forEach(path => {
      const node = nodesById.get(path.dataset.lensNode), outer = path.dataset.lensOuter === 'true';
      const shade = Number(path.dataset.lensSibling) % 3;
      let opacity = outer ? .49 + shade * .07 : .84;
      if (related(node, selected)) opacity = outer ? .67 + shade * .06 : 1;
      if (hovered) opacity = related(node, hovered) ? (outer ? .86 : 1) : .23;
      path.style.opacity = String(opacity);path.style.setProperty('--dl-hover-opacity', String(opacity));
      path.dataset.lensSelected = String(node === selected);
    });
    svg.querySelectorAll('text[data-lens-node]').forEach(label => {
      const node = nodesById.get(label.dataset.lensNode);label.style.opacity = hovered && !related(node, hovered) ? '.26' : '1';
    });
    list.querySelectorAll('[data-lens-row]').forEach(row => {
      const node = nodesById.get(row.dataset.lensRow);
      row.classList.toggle('dl-hovered', !!hovered && (hovered === node || hovered.parent === node));
    });
  }
  function linkHover(button, node) {
    button.addEventListener('pointerenter', () => hoverNode(node));
    button.addEventListener('pointerleave', () => hoverNode(null));
    button.addEventListener('focus', () => hoverNode(node));
    button.addEventListener('blur', () => hoverNode(null));
  }
  function polar(cx, cy, radius, angle) { return [cx + Math.cos(angle) * radius, cy + Math.sin(angle) * radius]; }
  function arcPath(cx, cy, inner, outer, start, end) {
    const fullEnd = Math.min(end, start + Math.PI * 2 - 0.0001);
    const a = polar(cx, cy, outer, start), b = polar(cx, cy, outer, fullEnd), c = polar(cx, cy, inner, fullEnd), d = polar(cx, cy, inner, start);
    const big = fullEnd - start > Math.PI ? 1 : 0;
    return 'M' + a.join(',') + ' A' + outer + ',' + outer + ' 0 ' + big + ',1 ' + b.join(',') + ' L' + c.join(',') + ' A' + inner + ',' + inner + ' 0 ' + big + ',0 ' + d.join(',') + ' Z';
  }
  function drawChart() {
    const box = stage.getBoundingClientRect();
    if (box.width < 10 || box.height < 10) return;
    const width = Math.round(box.width), height = Math.round(box.height), cx = width / 2, cy = height / 2;
    const radius = Math.min(width, height) * .484;
    const inner = radius * .54, parentOuter = radius * .797, childInner = radius * .812;
    svg.setAttribute('viewBox', '0 0 ' + width + ' ' + height);
    svg.replaceChildren();
    const title = document.createElementNS(ns, 'title'); title.textContent = scope.path + '，总占用 ' + scope.size + ' GB'; svg.appendChild(title);
    const desc = document.createElementNS(ns, 'desc'); desc.textContent = scope.children.map(n => n.name + ' ' + n.size + ' GB').join('；') + '。点击色块可选择，右侧列表提供相同的键盘操作。'; svg.appendChild(desc);
    let start = -Math.PI / 2;
    function addArc(node, a, b, r1, r2, outer, siblingIndex) {
      const path = document.createElementNS(ns, 'path');
      const gap = Math.min(.009, (b - a) * .12);
      path.setAttribute('d', arcPath(cx, cy, r1, r2, a + gap, b - gap));
      path.style.fill = 'var(--dl-' + node.color + ')';
      path.dataset.lensSelected = String(node === selected);
      path.dataset.lensNode = node.id; path.dataset.lensOuter = String(outer); path.dataset.lensSibling = String(siblingIndex);
      path.dataset.tooltip = node.name + ' · ' + node.size + ' GB · 占当前范围 ' + pct(node) + '%';
      path.classList.add('cursor-interaction');
      path.setAttribute('aria-hidden', 'true');
      path.addEventListener('click', () => selectNode(node));
      path.addEventListener('dblclick', () => enterNode(node));
      path.addEventListener('pointerenter', () => hoverNode(node));
      path.addEventListener('pointerleave', () => hoverNode(null));
      svg.appendChild(path);
    }
    scope.children.forEach((node, index) => {
      const span = node.size / scope.size * Math.PI * 2;
      const end = start + span;
      addArc(node, start, end, inner, parentOuter, false, index);
      if (node.children.length) {
        let childStart = start;
        node.children.forEach((child, childIndex) => {
          const childEnd = childStart + child.size / scope.size * Math.PI * 2;
          addArc(child, childStart, childEnd, childInner, radius, true, childIndex);
          childStart = childEnd;
        });
      } else { addArc(node, start, end, childInner, radius, true, index); }
      if (span > .6 && radius > 123) {
        const [x,y] = polar(cx, cy, (inner + parentOuter) / 2, start + span / 2);
        const text = document.createElementNS(ns, 'text'); text.setAttribute('x', x); text.setAttribute('y', y); text.setAttribute('class', 'dl-chart-label'); text.dataset.lensNode = node.id; text.textContent = Math.round(node.size / scope.size * 100) + '%'; svg.appendChild(text);
      }
      start = end;
    });
    updateHighlights();
  }
  function render() {
    hovered = null;
    const trail = []; let ancestor = scope;
    while (ancestor) { trail.unshift(ancestor); ancestor = ancestor.parent; }
    breadcrumb.replaceChildren();
    const back = makeButton('', 'dl-back', goParent);
    back.setAttribute('aria-label', '返回上一级'); back.disabled = scope === disk; back.append(icon('arrow-left')); breadcrumb.append(back);
    trail.forEach((node, index) => {
      if (index) { breadcrumb.appendChild(icon('chevron-right')); }
      const crumb = makeButton(node === disk ? 'D: 工作盘' : node.name, 'dl-crumb', () => { if (node !== scope) { scope = node; selected = trail[index + 1] || node.children[0] || node; render(); } });
      crumb.dataset.tooltip = node.path;
      if (node === scope) crumb.setAttribute('aria-current', 'location');
      breadcrumb.appendChild(crumb);
    });
    const scopeSize = document.createElement('span'); scopeSize.className = 'dl-scope-size'; scopeSize.textContent = scope.size + ' GB'; breadcrumb.append(scopeSize);
    root.querySelector('[data-lens-list-title]').textContent = '当前目录 · ' + scope.children.length;
    list.replaceChildren();
    scope.children.forEach(node => {
      const row = document.createElement('li'); row.className = 'dl-folder-row'; row.dataset.lensRow = node.id;
      const main = document.createElement('div'); main.className = 'dl-row-main'; row.appendChild(main);
      const btn = makeButton('', 'dl-folder-select', () => selectNode(node));
      btn.dataset.lensSelect = node.id;
      btn.setAttribute('aria-pressed', selected === node ? 'true' : 'false');
      btn.setAttribute('aria-label', '选择 ' + node.name + '，' + node.size + ' GB，占 ' + pct(node) + '%');
      btn.addEventListener('dblclick', () => enterNode(node)); linkHover(btn, node);
      const swatch = document.createElement('span'); swatch.className = 'dl-swatch'; swatch.style.setProperty('--dl-item-color', 'var(--dl-' + node.color + ')'); swatch.setAttribute('aria-hidden', 'true');
      const info = document.createElement('span'); info.className = 'dl-folder-info';
      const name = document.createElement('span'); name.className = 'dl-folder-name'; name.textContent = node.name; btn.dataset.tooltip = node.name;
      const meta = document.createElement('span'); meta.className = 'dl-folder-meta'; meta.textContent = node.children.length ? node.children.length + ' 个项目' : node.type === 'file' ? '文件' : '目录';
      info.append(name, meta);
      const size = document.createElement('span'); size.className = 'dl-folder-size'; size.textContent = node.size + ' GB';
      const share = document.createElement('small'); share.textContent = pct(node) + '%'; size.appendChild(share);
      btn.append(swatch, info, size); main.appendChild(btn);
      if (node.children.length) {
        const enter = makeButton('', 'dl-folder-enter', () => enterNode(node)); enter.setAttribute('aria-label', '进入 ' + node.name); enter.dataset.tooltip = '进入目录'; enter.appendChild(icon('chevron-right')); linkHover(enter, node); main.appendChild(enter);
      } else { const pad = document.createElement('span'); pad.style.width = '26px'; pad.setAttribute('aria-hidden', 'true'); main.appendChild(pad); }
      list.appendChild(row);
    });
    root.querySelector('[data-lens-center-name]').textContent = scope.name;
    root.querySelector('[data-lens-center-name]').dataset.tooltip = scope.path;
    const size = root.querySelector('[data-lens-center-size]'); size.replaceChildren(document.createTextNode(scope.size)); const unit = document.createElement('span'); unit.textContent = 'GB'; size.appendChild(unit);
    root.querySelector('[data-lens-center-share]').textContent = '当前目录 · ' + scope.children.length + ' 个项目';
    const center = root.querySelector('[data-lens-center-enter]'); center.disabled = !scope.parent;
    center.setAttribute('aria-label', scope.parent ? '返回 ' + scope.parent.name : '当前扫描范围 ' + scope.name + '，' + scope.size + ' GB');
    root.querySelector('[data-lens-center-back]').hidden = !scope.parent;
    drawChart(); updateSelection(); viewChanged();
  }
  function updateSelection() {
    list.querySelectorAll('[data-lens-row]').forEach(row => {
      const node = nodesById.get(row.dataset.lensRow);
      row.classList.toggle('dl-selected', selected === node || (row.classList.contains('dl-folder-row') && selected.parent === node));
    });
    list.querySelectorAll('[data-lens-select]').forEach(button => button.setAttribute('aria-pressed', String(button.dataset.lensSelect === selected.id)));
    root.querySelector('[data-lens-selection-title]').textContent = selected.name + ' · ' + selected.size + ' GB';
    root.querySelector('.dl-selection-icon').replaceChildren(icon(selected.type === 'file' ? 'file' : 'folder'));
    const path = root.querySelector('[data-lens-selection-path]'); path.textContent = selected.path; path.dataset.tooltip = selected.path;
    root.querySelectorAll('[data-lens-action="move"],[data-lens-action="rename"],[data-lens-action="trash"]').forEach(button => button.disabled = selected === disk);
    updateHighlights();
    if (globalThis.lucide) globalThis.lucide.createIcons({attrs: {width:16, height:16}});
  }
  root.querySelector('[data-lens-center-enter]').addEventListener('click', goParent);
  root.querySelectorAll('[data-lens-action]').forEach(button => button.addEventListener('click', () => {
    const kind = button.dataset.lensAction;
    const target = kind === 'scan' ? disk : selected;
    if (window.diskConceptActions && window.diskConceptActions.open) window.diskConceptActions.open(root, {...getActionContext(), ...rowOf(target), kind, exportScope: kind === 'export' ? 'selected' : undefined});
  }));
  window.diskViewAdapters = window.diskViewAdapters || {};
  window.diskViewAdapters.lens = {getState, setState, getActionContext};
  render();
  new ResizeObserver(drawChart).observe(stage);
})();
</script>
<script>
(()=>{
  const root=document.getElementById('spacefold-mac');
  const keys=['map','tree','lens'];
  const rootIds={map:'disk-map',tree:'disk-workbench',lens:'disk-lens'};
  const labels={map:'空间地图',tree:'目录树',lens:'层级透镜'};
  const tabbar=root.querySelector('.sf-switch'),thumb=root.querySelector('.sf-switch-thumb');
  const menu=root.querySelector('.sf-action-menu'),menuButton=root.querySelector('[data-sf-menu]');
  const sourceMenu=root.querySelector('.sf-source-menu'),sourceButton=root.querySelector('[data-sf-source]');
  const settingsMenu=root.querySelector('.sf-appearance-menu'),settingsButton=root.querySelector('[data-sf-settings]');
  const inspectorParts=keys.map(key=>({key,view:document.getElementById(rootIds[key]),panel:root.querySelector('[data-sf-inspector="'+key+'"]'),toggle:root.querySelector('[data-sf-inspector-toggle="'+key+'"]')}));
  const accentOptions=[...root.querySelectorAll('[data-sf-accent-choice]')];
  const accentLabels={purple:'紫色',blue:'蓝色',teal:'青色',amber:'橙色',rose:'玫红'};
  const reduced=window.matchMedia('(prefers-reduced-motion: reduce)');
  let active='tree',saved={},tick=0,thumbX=0,thumbWidth=0,targetX=0,targetWidth=0,velocityX=0,velocityWidth=0,lastTime=0,initialized=false;
  const adapters=()=>window.diskViewAdapters||{};
  const copy=value=>JSON.parse(JSON.stringify(value));
  const canonical=path=>String(path||'').replace(/\//g,'\\').replace(/\\+/g,'\\').replace(/\\$/,'').toLowerCase();
  function paintThumb(){thumb.style.transform='translate3d('+thumbX+'px,0,0)';thumb.style.width=thumbWidth+'px';}
  function spring(time){
    const dt=Math.min((time-(lastTime||time-16))/1000,.032);lastTime=time;
    velocityX+=(480*(targetX-thumbX)-44*velocityX)*dt;thumbX+=velocityX*dt;
    velocityWidth+=(480*(targetWidth-thumbWidth)-44*velocityWidth)*dt;thumbWidth+=velocityWidth*dt;
    paintThumb();
    if(Math.abs(targetX-thumbX)>.08||Math.abs(velocityX)>.08||Math.abs(targetWidth-thumbWidth)>.08||Math.abs(velocityWidth)>.08){tick=requestAnimationFrame(spring);}
    else{thumbX=targetX;thumbWidth=targetWidth;velocityX=0;velocityWidth=0;tick=0;lastTime=0;paintThumb();}
  }
  function updateThumb(immediate=false){
    const button=root.querySelector('[data-sf-view="'+active+'"]');
    targetX=button.offsetLeft;targetWidth=button.offsetWidth;
    if(!initialized||immediate||reduced.matches){if(tick)cancelAnimationFrame(tick);tick=0;lastTime=0;thumbX=targetX;thumbWidth=targetWidth;velocityX=0;velocityWidth=0;initialized=true;paintThumb();}
    else if(!tick)tick=requestAnimationFrame(spring);
  }
  function closeMenu(restore=false){menu.hidden=true;menuButton.setAttribute('aria-expanded','false');if(restore)menuButton.focus();}
  function closeSource(restore=false){sourceMenu.hidden=true;sourceButton.setAttribute('aria-expanded','false');if(restore)sourceButton.focus();}
  function closeSettings(restore=false){settingsMenu.hidden=true;settingsButton.setAttribute('aria-expanded','false');if(restore)settingsButton.focus();}
  function closePopovers(){closeMenu();closeSource();closeSettings();}
  let openInspector=null;
  const compact=()=>root.clientWidth<640;
  function updateInspectorAccessibility(){
    const modal=compact()&&!!openInspector;
    root.querySelector('.sf-topbar').toggleAttribute('inert',modal);
    inspectorParts.forEach(part=>{
      const expanded=modal&&part.key===openInspector;
      part.view.dataset.sfInspectorOpen=String(expanded);
      part.toggle.setAttribute('aria-expanded',String(expanded));
      part.panel.toggleAttribute('inert',compact()&&!expanded);
      if(compact()&&!expanded)part.panel.setAttribute('aria-hidden','true');else part.panel.removeAttribute('aria-hidden');
      if(expanded){part.panel.setAttribute('role','dialog');part.panel.setAttribute('aria-modal','true');}
      else{part.panel.removeAttribute('role');part.panel.removeAttribute('aria-modal');}
      part.view.querySelector('[data-sf-inspector-scrim]').hidden=!expanded;
      part.view.querySelectorAll('.wb-pathbar,.wb-directory,.wb-statusbar,.dm-canvas-wrap,.dl-pathbar,.dl-radial-column').forEach(node=>node.toggleAttribute('inert',expanded));
    });
  }
  function closeInspector(restore=false){
    const previous=openInspector;openInspector=null;updateInspectorAccessibility();
    if(restore&&previous)inspectorParts.find(part=>part.key===previous)?.toggle.focus();
  }
  function showInspector(key){
    if(!compact())return;
    const part=inspectorParts.find(item=>item.key===key);if(!part)return;
    closePopovers();window.diskConceptActions?.close();openInspector=key;updateInspectorAccessibility();
    part.panel.querySelector('[data-sf-inspector-close]').focus();
  }
  inspectorParts.forEach(part=>{
    part.toggle.addEventListener('click',()=>openInspector===part.key?closeInspector(true):showInspector(part.key));
    part.panel.querySelector('[data-sf-inspector-close]').addEventListener('click',()=>closeInspector(true));
    part.view.querySelector('[data-sf-inspector-scrim]').addEventListener('click',()=>closeInspector(true));
  });
  root.addEventListener('click',event=>{
    const button=event.target.closest('[data-wb-action],[data-map-action],[data-lens-action]');
    if(openInspector&&button&&!button.disabled&&button.closest('[data-sf-inspector]')&&button.dataset.mapAction!=='enter')closeInspector(true);
  },true);
  root.addEventListener('keydown',event=>{
    if(!openInspector||!compact())return;
    if(event.key==='Escape'){event.preventDefault();closeInspector(true);return;}
    if(event.key!=='Tab')return;
    const panel=inspectorParts.find(part=>part.key===openInspector).panel;
    const items=[...panel.querySelectorAll('button:not(:disabled),input:not(:disabled),select:not(:disabled),textarea:not(:disabled),a[href]')].filter(node=>!node.closest('[hidden]'));
    const first=items[0],last=items[items.length-1];
    if(event.shiftKey&&document.activeElement===first){event.preventDefault();last?.focus();}
    else if(!event.shiftKey&&document.activeElement===last){event.preventDefault();first?.focus();}
  });
  function setAccent(value){
    if(!Object.hasOwn(accentLabels,value))return;
    root.dataset.sfAccent=value;
    accentOptions.forEach(button=>button.setAttribute('aria-pressed',String(button.dataset.sfAccentChoice===value)));
    root.querySelector('[data-sf-accent-name]').textContent=accentLabels[value];
    root.querySelector('[data-sf-live]').textContent='强调色已设为'+accentLabels[value];
  }
  function positionPopover(popover,trigger){
    const frame=root.getBoundingClientRect(),anchor=trigger.getBoundingClientRect();
    const width=popover.getBoundingClientRect().width||popover.offsetWidth;
    const preferred=popover===sourceMenu?anchor.left-frame.left:anchor.right-frame.left-width;
    popover.style.left=Math.max(9,Math.min(preferred,frame.width-width-9))+'px';
    popover.style.right='auto';popover.style.top=(anchor.bottom-frame.top+9)+'px';
  }
  function syncSidebar(){
    const scope=canonical(adapters()[active]?.getState()?.scopePath);
    const locations=[...root.querySelectorAll('[data-sf-path]')];
    const match=locations.filter(button=>scope===canonical(button.dataset.sfPath)||scope.startsWith(canonical(button.dataset.sfPath)+'\\')).sort((a,b)=>b.dataset.sfPath.length-a.dataset.sfPath.length)[0];
    locations.forEach(button=>{if(button===match)button.setAttribute('aria-current','location');else button.removeAttribute('aria-current');});
  }
  function switchView(key){
    if(key===active||!adapters()[key])return;
    const outgoing=adapters()[active]?.getState();if(outgoing)saved[active]=copy(outgoing);
    let incoming=outgoing;
    if(key==='tree'&&saved.tree&&outgoing&&canonical(saved.tree.selectedPaths?.[0])===canonical(outgoing.selectedPaths?.[0]))incoming=saved.tree;
    closePopovers();closeInspector();window.diskConceptActions?.close();
    active=key;
    keys.forEach(mode=>{root.querySelector('#sf-mac-view-'+mode).hidden=mode!==key;root.querySelector('#sf-mac-tab-'+mode).setAttribute('aria-selected',String(mode===key));});
    if(incoming)adapters()[key].setState(copy(incoming));
    updateThumb();syncSidebar();root.querySelector('[data-sf-live]').textContent='已切换到'+labels[key];
  }
  root.querySelectorAll('[data-sf-view]').forEach(button=>button.addEventListener('click',()=>switchView(button.dataset.sfView)));
  tabbar.addEventListener('keydown',event=>{
    if(!['ArrowLeft','ArrowRight','Home','End'].includes(event.key))return;
    event.preventDefault();const index=keys.indexOf(active),next=event.key==='Home'?0:event.key==='End'?2:(index+(event.key==='ArrowRight'?1:2))%3;
    switchView(keys[next]);root.querySelector('#sf-mac-tab-'+keys[next]).focus();
  });
  function openAction(kind,context){
    closePopovers();closeInspector();const data=context||adapters()[active]?.getActionContext();
    if(data)window.diskConceptActions?.open(document.getElementById(rootIds[active]),{...data,kind});
  }
  root.querySelectorAll('[data-sf-action]').forEach(button=>button.addEventListener('click',()=>{
    if(sourceMenu.contains(button))sourceButton.focus();
    openAction(button.dataset.sfAction);
  }));
  root.querySelectorAll('[data-sf-path]').forEach(button=>button.addEventListener('click',()=>{
    closePopovers();sourceButton.focus();window.diskConceptActions?.close();const path=button.dataset.sfPath;
    adapters()[active]?.setState({scopePath:path,selectedPaths:[]});syncSidebar();root.querySelector('[data-sf-live]').textContent='已打开 '+path;
  }));
  sourceButton.addEventListener('click',()=>{
    if(!sourceMenu.hidden){closeSource();return;}
    closeMenu();closeSettings();window.diskConceptActions?.close();syncSidebar();
    sourceMenu.hidden=false;sourceButton.setAttribute('aria-expanded','true');positionPopover(sourceMenu,sourceButton);
  });
  menuButton.addEventListener('click',()=>{
    if(!menu.hidden){closeMenu();return;}
    closeSource();closeSettings();window.diskConceptActions?.close();const context=adapters()[active]?.getActionContext();if(!context)return;
    const rows=context.selectedRows||[],count=rows.length,isRoot=count===1&&canonical(rows[0].path)==='d:';
    root.querySelector('[data-sf-menu-title]').textContent=count>1?'已选 '+count+' 项':rows[0]?.name||context.name||'所选项目';
    menu.querySelectorAll('[data-sf-file]').forEach(button=>{const kind=button.dataset.sfFile;button.disabled=count===0||(['open','rename'].includes(kind)&&count!==1)||(isRoot&&['rename','move','trash'].includes(kind));});
    menu.hidden=false;menuButton.setAttribute('aria-expanded','true');positionPopover(menu,menuButton);
  });
  settingsButton.addEventListener('click',()=>{
    if(!settingsMenu.hidden){closeSettings();return;}
    closeMenu();closeSource();window.diskConceptActions?.close();
    settingsMenu.hidden=false;settingsButton.setAttribute('aria-expanded','true');positionPopover(settingsMenu,settingsButton);
    settingsMenu.querySelector('[aria-pressed="true"]')?.focus();
  });
  root.querySelector('[data-sf-settings-close]').addEventListener('click',()=>closeSettings(true));
  accentOptions.forEach(button=>button.addEventListener('click',()=>setAccent(button.dataset.sfAccentChoice)));
  settingsMenu.addEventListener('keydown',event=>{
    if(event.key==='Escape'){event.preventDefault();closeSettings(true);return;}
    if(!event.target.matches('[data-sf-accent-choice]')||!['ArrowLeft','ArrowRight','Home','End'].includes(event.key))return;
    event.preventDefault();const index=accentOptions.indexOf(event.target);
    const next=event.key==='Home'?0:event.key==='End'?accentOptions.length-1:(index+(event.key==='ArrowRight'?1:accentOptions.length-1))%accentOptions.length;
    const button=accentOptions[next];setAccent(button.dataset.sfAccentChoice);button.focus();
  });
  menu.querySelectorAll('[data-sf-file]').forEach(button=>button.addEventListener('click',()=>{menuButton.focus();openAction(button.dataset.sfFile);}));
  menu.addEventListener('keydown',event=>{
    if(event.key==='Escape'){event.preventDefault();closeMenu(true);}
    if(['ArrowDown','ArrowUp'].includes(event.key)){event.preventDefault();const items=[...menu.querySelectorAll('button:not(:disabled)')],index=items.indexOf(document.activeElement);items[(index+(event.key==='ArrowDown'?1:items.length-1)+items.length)%items.length]?.focus();}
  });
  sourceMenu.addEventListener('keydown',event=>{
    if(event.key==='Escape'){event.preventDefault();closeSource(true);}
    if(['ArrowDown','ArrowUp'].includes(event.key)){event.preventDefault();const items=[...sourceMenu.querySelectorAll('button:not(:disabled)')],index=items.indexOf(document.activeElement);items[(index+(event.key==='ArrowDown'?1:items.length-1)+items.length)%items.length]?.focus();}
  });
  root.addEventListener('click',event=>{
    if(!menu.hidden&&!menu.contains(event.target)&&!menuButton.contains(event.target))closeMenu();
    if(!sourceMenu.hidden&&!sourceMenu.contains(event.target)&&!sourceButton.contains(event.target))closeSource();
    if(!settingsMenu.hidden&&!settingsMenu.contains(event.target)&&!settingsButton.contains(event.target))closeSettings();
    syncSidebar();
  });
  root.addEventListener('disk-view-change',()=>{syncSidebar();});
  document.addEventListener('click',event=>{if(!root.contains(event.target))closePopovers();});
  document.addEventListener('click',event=>{if(!root.contains(event.target)&&openInspector)closeInspector();});
  root.addEventListener('keydown',event=>{
    if(event.key!=='Escape')return;
    if(!menu.hidden){closeMenu(true);event.preventDefault();}
    if(!sourceMenu.hidden){closeSource(true);event.preventDefault();}
    if(!settingsMenu.hidden){closeSettings(true);event.preventDefault();}
  });
  root.addEventListener('focusin',event=>{
    if(!menu.hidden&&!menu.contains(event.target)&&!menuButton.contains(event.target))closeMenu();
    if(!sourceMenu.hidden&&!sourceMenu.contains(event.target)&&!sourceButton.contains(event.target))closeSource();
    if(!settingsMenu.hidden&&!settingsMenu.contains(event.target)&&!settingsButton.contains(event.target))closeSettings();
  });
  document.addEventListener('focusin',event=>{if(!root.contains(event.target))closePopovers();});
  root.addEventListener('disk-concept-scan',event=>{
    const path=event.detail?.path;if(!path||!adapters()[active])return;
    adapters()[active].setState({scopePath:path,selectedPaths:[]});syncSidebar();root.querySelector('[data-sf-live]').textContent='当前扫描位置：'+path;
  });
  if(window.ResizeObserver)new ResizeObserver(()=>{
    updateThumb(true);
    if(!sourceMenu.hidden)positionPopover(sourceMenu,sourceButton);
    if(!menu.hidden)positionPopover(menu,menuButton);
    if(!settingsMenu.hidden)positionPopover(settingsMenu,settingsButton);
  }).observe(tabbar);
  if(window.ResizeObserver)new ResizeObserver(()=>{if(!compact())openInspector=null;updateInspectorAccessibility();}).observe(root);
  updateInspectorAccessibility();
  requestAnimationFrame(()=>{updateThumb(true);syncSidebar();});
  window.spacefoldPreview={switchView,getActiveView:()=>active,getState:()=>adapters()[active]?.getState(),setAccent,getAccent:()=>root.dataset.sfAccent,showInspector,closeInspector,getOpenInspector:()=>openInspector};
})();
</script>

````
<!-- SPACEFOLD_REFERENCE_END -->

