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
    "codexColor": "#63d9ca",
    "deepseekColor": "#7c9fff",
    "showTool": true,
    "showElapsed": true,
    "swap": false
  },
  "cards": []
}
```

每张卡片包含 provider、会话 ID、状态、项目、标题、工具名、时间、完成提示和最多三条会话行。会话行的 `project` 字段为兼容旧协议保留该名称，实际内容优先使用对话标题。`codex` 的屏幕显示名称为 ChatGPT，字段与数据来源保持兼容。

状态区为 512×172，右侧 128×172 为录音区。会话行固定单行，标题超宽时显示省略号；圆点和标题独立绘制。新固件收到旧版两卡片帧时恢复两列。

## 配网与发现

USB 串口为 115200，每行一个 JSON 命令。`configure` 携带 SSID、密码、电脑地址、端口、令牌与电脑 ID；板子确认 `CONFIG_SAVED`。还支持 `profiles`、`forget` 和 `status`。

板子最多保存四组 Wi-Fi/电脑配置，电脑通过 `_aipanel._tcp.local.` 广播自身 ID。发现失败时使用保存的地址；断线后自动重试。更新沿用 NVS 分区和电脑的现有 `device.json`，不生成新的配对身份。

## 无线语音

板子发送 `{"type":"voice","action":"start"}`，随后以二进制帧发送 16 kHz、单声道、16 位小端 PCM；结束发送 `action=stop`。电脑回复 `type=voice`，状态为 partial、finished 或 error。

电脑把音频写入 VB-CABLE/BlackHole，由微信输入法识别。启动时缓存开头音频，结束前排空尾部；音频积压有界，断线会取消会话。Windows WASAPI 和 macOS Core Audio 监听输入法采音状态，确认已开始后持续停止 250 ms 时通知板子结束，避免再次发送快捷键重新打开录音。

## 验证

- `cargo test --workspace`：使用临时目录与注入依赖验证状态、压缩增量读取、标题、跨目录 Hooks、设备协议和语音生命周期，不读取本机真实会话。
- `desktop/app/tests/verify-ui.cjs`：独立无界面浏览器与模拟 Tauri，检查三张卡片、会话固定、配色保存和窄窗口布局。
- `firmware/tests/lvgl-row/build.ps1`：编译实际 LVGL、固件 UI 和字体，无窗口渲染并验证单行、图标、列宽及连续刷新。
