# 本机验证环境

2026-10-12，Windows 11 专业版 10.0.26300 x64；AMD Ryzen 7 9800X3D。三块2TB SSD：TOPMORE Aries、SOLIDIGM SSDPFKKW020X7、ADATA SX8200PNP。硬件名称只表示本机条件，不据此声称 HDD 或网络盘表现。

已验证工具：Node.js 24.14.0、npm 11.9.0、Rust stable 1.99.0（x86_64-pc-windows-msvc）、Visual Studio 2022 Build Tools 17.14.41（VC工具及Windows SDK）。Rust位于 `%USERPROFILE%/.cargo/bin`，安装后的既有终端需重新加载用户PATH。

现有 Microsoft Edge 155.0.4283.45、WebView2 Runtime 154.0.4258.62。原型基准用 Edge 浏览器、DPR=1、100%缩放与系统字体栈；原生验收另记录实际WebView2版本和DPR，不把浏览器截图当成应用截图。

Rust和Build Tools是本轮为通用Rust开发配置的工具，Visual Studio安装完成且`vswhere`确认完整、可用、无需重启。
