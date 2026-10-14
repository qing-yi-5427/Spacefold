# 间隙 Spacefold

离线 Windows 磁盘空间分析与文件管理工具，用**目录树、空间地图、层级透镜**查看同一份扫描结果。

[下载最新版本](https://github.com/qing-yi-5427/Spacefold/releases/latest)

## 下载运行

面向 Windows 11 x64，提供安装版与便携版：

| 版本 | 使用方式 |
|---|---|
| 安装版，约 5.2 MiB | 双击 setup 安装；系统缺少 WebView2 时需下载微软官方运行依赖。 |
| 便携版，约 311.4 MiB | 完整解压到本地磁盘，运行 `Spacefold.exe`；包含 WebView2 155.0.4283.45 x64，免安装。保留旁边的 `WebView2Runtime` 目录。 |

下载页附 SHA-256 校验清单和中文使用说明。设置与扫描记录保存在 `%APPDATA%\com.spacefold.desktop`。固定版运行组件不自动更新，随新版发行包更新；不支持从网络位置/UNC 启动便携程序。

扫描数据库升级后，旧版无法打开；更新前请退出旧版程序。

## 功能与限制

- 目录扫描、实时进度与取消、部分结果及重启恢复；搜索、筛选、排序和多选。
- 打开/定位、复制路径、属性、复制、移动、重命名和回收站；执行前确认计划并重新核验对象。
- CSV、XLSX、JSON、HTML 报告，五种强调色、明暗主题和 GB/GiB 单位。

默认按本机逻辑 CPU 数并行扫描。

默认统计逻辑大小，硬链接按路径累计；未知分配大小不会当成零。取消批量操作可能保留已完成项，回收站失败不会转为永久删除。HDD、实际云占位、断网/断卷及 Windows 10 尚未完成实机验证。历史比较、暂停续扫、硬链接去重和其他平台属于后续范围。

## 实际界面

Windows WebView2 应用扫描自建演示文件的截图，文件名与分布为测试夹具。

![目录树](screenshots/tree.png)
![空间地图](screenshots/map.png)
![层级透镜](screenshots/lens.png)

## 开发构建

需要 Node.js 24、Rust stable MSVC、Visual Studio 2022 Build Tools（C++ 桌面开发与 Windows SDK）。

```powershell
npm ci
npm run build
npm test
cargo test --locked --manifest-path src-tauri/Cargo.toml
npm run tauri build
```

`npm run tauri dev` 启动开发模式；生产界面内嵌，无需开发服务器。使用 [打包脚本](scripts/package.ps1) 生成双版本时，传入脚本指定版本的官方 Fixed Runtime CAB：`-WebView2RuntimeCab <CAB路径>`。CI 检查与构建，不自动发布。

## 许可

源码采用 [MIT](LICENSE)；依赖声明见 [第三方版权与许可](licenses/THIRD-PARTY-LICENSES.txt)，其中组件可按锁文件及声明中的版本从 Cargo/npm 获取原始源码。随包 WebView2 遵循 [微软固定版许可](licenses/WebView2-Fixed-License.html)。WebView2 包含 SmartScreen，可能按[微软隐私声明](https://aka.ms/privacy)和 [Edge 隐私说明](https://learn.microsoft.com/en-us/microsoft-edge/privacy-whitepaper#smartscreen)向微软发送相关信息；磁盘分析在本机完成。
