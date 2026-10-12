# 间隙 Spacefold

离线 Windows 磁盘空间分析工具。扫描目录后，用目录树、空间地图与层级透镜查看同一份结果，右侧完成定位、复制、移动、重命名、回收站和报告导出。

默认按逻辑大小统计，硬链接按目录项累计。已分配大小单独展示，无法测量的值保留为未知；扫描合计与磁盘可用空间分别显示。未读取的内容、链接跳转及云目录覆盖边界会显示在记录中。

## 运行与构建

首发目标 Windows 11 x64；需要 Microsoft Edge WebView2 Runtime。安装包会在缺失时下载官方引导程序，因此首次安装可能需要联网。

开发环境：Node.js 24、Rust stable MSVC、Visual Studio 2022 Build Tools（Desktop development with C++、Windows SDK）。

```powershell
npm ci
npm run tauri dev
```

```powershell
npm test
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build
```

运行 `powershell -ExecutionPolicy Bypass -File scripts/package.ps1` 可检查源码并生成 `dist-release/Spacefold.exe`、中文 NSIS 安装包、SHA256 校验清单与使用说明。依赖锁文件随源码提交；生产构建内嵌界面，可关闭开发服务器独立运行。构建不要求管理员权限，扫描按当前用户权限执行。

## 验证与设计

- [验收矩阵](docs/acceptance.md)：F01–F11、U01–U15、规模与发布检查。
- [同机性能实测](docs/performance.md)：与 WinDirStat、gdu、dua 的计数、耗时、内存和持久化方式比较。
- [功能和技术需求](spacefold-features-tech.md)、[UI 复刻规范](spacefold-ui-spec.md)：保留原始设计与验收要求。
- `npm run dev` 启动前端；`?fixture=1` 是固定设计夹具，仅用于视觉对照。
- `node scripts/prepare-reference.mjs` 核验并生成独立参考页面，原型模拟操作不代表系统功能。

文件管理执行前重新核验来源与目标；取消批量操作可能留下已经完成的项，结果逐项显示。回收站失败不会改用永久删除。

本机已验证跨 G→C 卷目录复制/移动、ACL 拒绝、链接跳转防护和四格式导出；云占位、断网/断卷、HDD 与缺失 WebView2 的全新系统尚未建立实测条件。完整记录与边界见验收矩阵。P1（历史比较、暂停续扫、硬链接去重、计划扫描和其他平台）不在首版范围。

MIT License。
