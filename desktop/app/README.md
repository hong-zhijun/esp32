# AI 状态屏 · 桌面程序（Rust + Tauri）

当前电脑端使用 Rust + Tauri。功能：读取 Claude / Codex / DeepSeek Harness 状态推送到屏幕、USB 配网、无线微信语音桥、托盘常驻、开机自启。模块与设备协议见 [架构说明](../../docs/architecture.md)。

旧 Node 服务保留本地 SenseVoice 识别回退模式；两版都监听 8787，**不能同时运行**。

## 目录

| 路径 | 内容 |
|---|---|
| `crates/core` | 配置、会话状态、Claude/Codex/Harness 来源、标题、Hooks |
| `crates/voice` | 微信语音桥：虚拟声卡输出、快捷键、Windows/macOS 采音状态 |
| `src-tauri` | 桌面外壳：板子 WebSocket、mDNS、串口配网、托盘、命令 |
| `ui` | 界面（原生 HTML/CSS/JS，无构建步骤） |

## 首次使用

1. 关闭旧版 Node 服务（以及 Arduino 串口监视器）。
2. 安装并打开本程序。Windows 首次运行若弹出防火墙提示，允许"专用网络"（板子要连 8787 端口，自动发现用 UDP 5353）。
   如果屏幕一直提示“正在连接电脑”，在管理员 PowerShell 中运行 `powershell -ExecutionPolicy Bypass -File desktop/app/allow-lan.ps1`（路径按实际项目位置调整）。脚本为当前运行的新版程序放行专用网络、本地子网的 TCP 8787；旧版 `desktop/allow-lan.ps1` 只放行 Node，不适用于新版。
3. **设置 → Claude Hooks → 安装 / 更新 Hooks**：把 `~/.claude/settings.json` 里 8 个事件指向本程序（自动备份原文件，其他配置不动）。
4. 烧录新固件后，**设备 → USB 配网**：插 USB，选串口、电脑地址，填 2.4 GHz Wi‑Fi。公司、家里各配一次，板子最多记住 4 组，之后自动连接并自动找电脑。
5. **语音**：
   - Windows：安装 [VB-CABLE](https://vb-audio.com/Cable/)；微信输入法麦克风选 **CABLE Output**；"启动语音输入"快捷键设为与本程序一致（默认 F8）。
   - macOS：安装 BlackHole 2ch；微信输入法麦克风选 **BlackHole 2ch**；快捷键默认 Fn＋空格；首次使用在"系统设置 → 隐私与安全性 → 辅助功能"允许本程序。
   - 本程序"语音"页可改快捷键（录制或预设）；"测试"会真实发送一次快捷键。
   - 从屏幕开始录音后，在电脑上结束微信语音，屏幕也会同步结束：Windows 读取 WASAPI 中微信输入法的采音会话，macOS 读取 Core Audio 进程状态。先检测到开始采音，再连续确认停止 250 ms 才结束；检测失败只记录日志，不会误判停止，也不会再次发送快捷键重新打开语音。

## 开发

```bash
cd desktop/app
cargo test --workspace
cargo run -p ai-status-panel
```

开发用环境变量：`AI_PANEL_PORT`（改端口）、`AI_PANEL_DATA_DIR`（改数据目录）、`AI_PANEL_NO_VOICE=1`（关闭语音桥）、`DSH_HOME`（Harness 数据根目录，默认 `~/.dsh`，支持 `~/`）。

## DeepSeek Harness

支持 Harness v4 的 `sessions/**/session.v4.jsonl.zstd` 和未压缩的 `session.v4.jsonl`。只读增量解析追加的完整 Zstandard 帧，每 5 秒发现新会话，每秒读取已跟踪会话；未写完的帧等待下次读取，首次启动分批追赶历史。只保留会话 ID、工作目录、标题、工具名、时间和状态；不保存提示词、工具参数或回复正文。子代理不单列，最近 24 小时更新的会话自动纳入，`tracked-deepseek.json` 保持重启后的跟踪。

`turn/start` 显示执行中，`approval/asked` 显示等你确认，全部待处理权限请求结束后恢复执行中；`turn/end` 的 `completed` 显示本轮结束，`aborted`/`interrupted` 显示中断，未知原因或错误显示待确认，不声称业务成功。超过 5 分钟静默同样显示待确认。

屏幕顺序为 Claude、ChatGPT、DeepSeek、录音区。ChatGPT 列沿用 Codex 任务来源和配色字段。前三列共用 512 像素，录音区保留 128 像素；默认等宽，分栏滑块分配前两列的宽度，交换位置只影响前两列。DeepSeek 有独立配色、固定会话和清除完成提示。旧固件仍只显示前两列，需要烧录三列固件（当前 hello 报告 fw=6，包含固定单行标题和名称左侧的对应标志）。新固件收到旧版两卡片协议时自动恢复两列。

验证：`cargo test --workspace`。测试使用合成日志、临时目录与注入依赖；`tests/verify-ui.cjs` 使用独立无界面浏览器及模拟 Tauri 检查桌面三列、DeepSeek 固定、配色保存和窄屏布局。

## 对话标题

Windows 下会持续增量读取普通数据目录和 Claude MSIX 隔离目录中的 Hooks 日志，不依赖升级时复制的历史 `events.jsonl`。两个目录同一会话按事件时间合并，旧副本不会覆盖新状态，之后新建的日志也会自动读取。这样从普通 Windows 环境启动状态屏、同时从 Claude Desktop 运行任务时，Claude 状态仍会实时更新。

三个来源统一优先显示对话标题，没有标题才回退项目名和会话 ID。Claude 对已由 Hooks 跟踪的会话每 5 秒读取一次标题元数据：`~/.claude/projects/<project>/<session>/custom-title.json`、会话 JSONL 的 `custom-title`、`~/.claude/sessions/*.json` 的 `name`、会话索引的摘要标题。支持 `CLAUDE_CONFIG_DIR` 指定根目录；不会用用户提示词、`firstPrompt` 或工具正文拼接标题，也不会读取 `.key` 凭据文件。

DeepSeek 直接使用 `session/title`；标题在本轮开始前出现也会缓存，后续改名同步到电脑卡片和屏幕会话行。标题修改不会改变活动时间、状态或已清除的完成提示。标题超出屏幕列宽时显示省略号。

标题功能只需更新电脑端。若已烧录三列固件，可退出托盘程序后运行 `build/deepseek-update/update-desktop-only.cmd`；还未安装三列固件时使用同目录的 `update-screen-and-desktop.cmd` 完整更新。

打包（Windows 生成 NSIS 安装包，macOS 需在 Mac 上执行，生成 .app / .dmg）：

```bash
cargo install tauri-cli --version "^2" --locked
cd desktop/app/src-tauri && cargo tauri build
```

## 数据

`%APPDATA%\ai-status-panel`（Windows）或 `~/Library/Application Support/ai-status-panel`（macOS）：`config.json`、`device.json`（板子令牌，勿分享）、`acknowledged.json`、`tracked-codex.json`、`tracked-deepseek.json`、`events.jsonl`。

Windows 下，由 MSIX 打包的 Claude 启动时，数据可能重定向到 `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\ai-status-panel`。更新时以实际设备正在使用的身份为准，先校验令牌指纹并备份，不能仅凭目录或文件时间覆盖另一份身份。`AI_PANEL_DATA_DIR` 可显式指定数据目录；Hooks 事件会持续跟踪两个目录。

## 与 Node 版的差异

- 不再包含本地 SenseVoice 识别模式（`VOICE_BACKEND=local`）和自动粘贴；默认的微信语音路径不受影响。
- 首次迁移应核对配对数据，正常版本更新沿用现有令牌和语音配置。
- 屏幕设置改动自动保存，无需点保存。
- 微信快捷键通过录制或预设设置，与微信输入法保持一致。
