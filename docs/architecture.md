# 架构与协议

## 模块

| 路径 | 职责 |
|---|---|
| `desktop/app/crates/core` | 配置、设备身份、会话状态机、Claude/Codex/Harness 日志读取、标题、Claude Hooks |
| `desktop/app/crates/voice` | 微信语音生命周期、虚拟声卡输出、重采样、快捷键、Windows/macOS 采音检测 |
| `desktop/app/src-tauri` | 桌面窗口与托盘、开机自启、设备 WebSocket、mDNS、串口配网和 UI 命令 |
| `desktop/app/ui` | 无构建步骤的 HTML/CSS/JS 界面 |
| `firmware/ai-status-screen` | Wi-Fi、录音采集、按键与触摸、LVGL 状态显示 |

电脑端每秒增量读取日志、更新状态机，并向界面与板子推送快照。状态优先级为等待确认、执行中、未清除的完成提示、其他状态；工作或等待状态超过 5 分钟没有新事件时显示待确认。

`core/presence.rs` 每秒通过系统进程的可执行路径独立检测程序运行状态，不依赖日志时间；忽略 macOS 嵌套 Helper / 内置 CLI 和僵尸进程。Windows 的 `core/presence/windows.rs` 仅刷新已识别可执行文件及 Node 的参数，排除 Electron `--type` / crashpad 辅助进程、已知桌面内置 CLI 和同来源进程的后代；父链检查启动时间以防 PID 复用。Node 只匹配已知 npm 包的实际入口脚本。参数只在内存中用于识别，不记录、不发送；不读取进程环境。桌面卡片和 fw≥7 根据 `running` 显示栏目，历史会话仍保留；主程序最小化或后台运行时继续显示。

Claude 使用 Hooks 事件，并读取对话标题元数据；Windows 同时跟踪普通 AppData 和 Claude MSIX 隔离目录的 Hooks 文件。Codex 使用 rollout 日志与会话索引。DeepSeek 使用 Harness v4 的 JSONL 或独立追加 Zstandard 帧。标题变化不改变状态时间或完成提示。

## 屏幕状态帧

设备通过 `GET /device?token=<device token>` 建立 WebSocket。电脑端在升级连接前验证令牌，每 30 秒发送心跳；界面命令通过本地 Tauri IPC 调用。

状态 JSON 的 `type` 必须是第一个字段，卡片顺序固定为 Claude、Codex、DeepSeek：

```json
{
  "type": "state",
  "now": 0,
  "config": {
    "split": 50,
    "brightness": 160,
    "claudeColor": "#df9877",
    "codexColor": "#ffffff",
    "deepseekColor": "#7c9fff",
    "showTool": true,
    "showElapsed": true,
    "swap": false
  },
  "cards": [],
  "running": { "claude": false, "codex": true, "deepseek": false }
}
```

每张卡片包含 provider、会话 ID、状态、项目、标题、工具名、时间、完成提示和最多三条会话行。会话行的 `project` 字段为兼容旧协议保留该名称，实际内容优先使用对话标题。`codex` 的屏幕显示名称为 ChatGPT，字段与数据来源保持兼容。

fw=8 状态区为 548×172，右侧 92×172 为按钮区。会话行固定单行，标题超宽时显示省略号；圆点和标题独立绘制。新固件收到旧版两卡片帧时恢复两列。

三张卡片保持固定顺序，即使对应程序退出也保留其历史状态。fw=7 按 `running` 隐藏退出的来源，压紧剩余栏目并铺满状态区；全部退出时状态区留空。缺少 `running` 字段时兼容旧版显示方式。

## 配网与发现

USB 串口为 115200，每行一个 JSON 命令。`configure` 携带 SSID、密码、电脑地址、端口、令牌与电脑 ID；板子确认 `CONFIG_SAVED`。还支持 `profiles`、`forget` 和 `status`。

板子最多保存四组 Wi-Fi/电脑配置，电脑通过 `_aipanel._tcp.local.` 广播自身 ID。发现失败时使用保存的地址；断线后自动重试。更新沿用 NVS 分区和电脑的现有 `device.json`，不生成新的配对身份。

## 无线语音

板子发送 `{"type":"voice","action":"start"}`，随后以二进制帧发送 16 kHz、单声道、16 位小端 PCM；结束发送 `action=stop`。电脑回复 `type=voice`，状态为 partial、finished 或 error。

电脑把音频写入 VB-CABLE/BlackHole，由微信输入法识别。启动时缓存开头音频，结束前排空尾部；音频积压有界，断线会取消会话。Windows WASAPI 和 macOS Core Audio 监听输入法采音状态，确认已开始后持续停止 250 ms 时通知板子结束，避免再次发送快捷键重新打开录音。

## 验证

- `cargo test --workspace`：使用临时目录与注入依赖验证状态、压缩增量读取、标题、跨目录 Hooks、设备协议和语音生命周期；临时子进程验证启动/退出检测，不读取本机真实会话。
- `desktop/app/tests/verify-ui.cjs`：独立无界面浏览器与模拟 Tauri，检查三张卡片、全部 8 种程序运行组合、历史保留、会话固定、配色保存和窄窗口布局。
- `firmware/tests/lvgl-row/build.ps1`（Windows）或 `build.py`（macOS/Linux）：编译实际 LVGL、固件 UI 和字体，无窗口渲染并验证单行、图标、列宽、8 种运行组合、旧协议及连续刷新。

## 屏幕按钮

fw=8 右侧为上下两个 76×76 正方形按钮，仅显示录音波形和回车箭头，无文字标签。录音继续使用原语音消息；回车发送 `{"type":"key","key":"Enter"}`，仅经过配对认证的 `/device` WebSocket 可触发。电脑端仅允许普通 Enter，固定不带任何修饰键，使用现有 macOS CGEvent / Windows SendInput 实现；250 ms 内重复请求忽略，按键调用在阻塞线程池执行，失败记录 `keyboard-error`。屏幕锁定时忽略两个按钮，并清空待处理请求。

## 极简仪表（fw=9）

状态区保留 548×172 和动态分栏。每栏显示 18×18 品牌标志、柔白名称、24 像素状态、一行当前任务标题及底部工具 / 时长。标题优先取卡片 title，其次按卡片 id 匹配 rows，最后回退项目名称及旧协议首行；不会把其他紧急任务的标题错误显示在已固定的任务下。所有文本固定高度，超宽省略；时长单独右对齐，窄栏隐藏放不下的工具。状态圆点轻微呼吸，去掉粗色条和横向跑马线。主机测试使用实际 LVGL 与字体验证布局、状态字形、标题回退、旧协议及按钮事件。

fw=10 放大状态至 28px、任务标题至 20px、平台名及计时至 16px，标志为 20×20。状态到标题、标题到底部的间距固定 10px；主机渲染测试覆盖三种分栏宽度下的文本边界及垂直间隔。20px 中文字库启用 LVGL 压缩，底部 16px 精简字库通过 fallback 使用完整字库，减少重复字形占用。

fw=11 保持 20px 任务字号，替换为 4bpp 灰阶抗锯齿中文字库；字形测试验证边缘中间灰阶。固件自带 partitions.csv，在 16MB Flash 上提供两个 6MB APP 分区，NVS 与 app0 起始位置沿用旧版；编译最大程序长度设置为 6291456。
