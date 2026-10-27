# 本地 AI 状态屏

ESP32-S3-Touch-LCD-3.49 V2 状态屏，搭配 Rust + Tauri 桌面程序，显示 Claude、Codex、DeepSeek Harness 的任务状态，并通过无线麦克风桥接微信输入法。

当前桌面版本为 **0.2.0**，固件版本为 **6**。屏幕从左到右显示 Claude、ChatGPT、DeepSeek 和录音区；ChatGPT 列使用 Codex 的任务状态。

- [桌面程序与首次使用](desktop/app/README.md)
- [固件编译、烧录和排版回归](firmware/README.md)
- [状态来源、三列显示和本机更新](docs/deepseek-status.md)
- [架构与设备协议](docs/architecture.md)
- [旧版 Node 服务](desktop/README.md)：保留本地 SenseVoice 识别回退模式，不能与 Rust 程序同时监听 8787。

开发验证：

```powershell
cd desktop/app
cargo test --workspace --offline
```

运行时数据、设备令牌、下载的驱动和编译产物不提交到 Git。已生成的本机更新包位于 `build/deepseek-update/`，更新脚本会校验配对身份、备份配置并保留当前可执行路径。
