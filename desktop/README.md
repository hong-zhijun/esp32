# 本地 AI 状态屏

打开 http://localhost:8787 。重启电脑后运行 `powershell -ExecutionPolicy Bypass -File start.ps1`。

## 配网

1. 保留板子的 USB 连接，关闭 Arduino 串口监视器。
2. 网页“设备配网”选择 COM3 和电脑实际局域网地址（当前 192.168.0.64，不选 Tailscale 地址）。
3. 本地填写 2.4 GHz Wi-Fi 名称和密码，点击写入。密码不保存在电脑服务中，会保存在板子 NVS。
4. 等待屏幕连接。路由器不能开启阻止设备互访的访客隔离。
5. 如果防火墙阻止连接，以管理员方式运行 allow-lan.ps1。规则仅限私有网络、本地子网、Node TCP 8787。

## 状态来源与限制

- Claude：已安装的 `cc-bridge/hook.cjs` 写事件（Claude Code 全局 settings.json 中的 hooks 指向 `D:/workspace/esp32/desktop/cc-bridge/hook.cjs`，移动目录需同步修改），服务增量读取。不会采集提示词、工具参数或回复正文。
- Codex：只读 `$CODEX_HOME/sessions` 或 `~/.codex/sessions` 的最近三天 JSONL；识别本机观测到的 task_started/task_complete/turn_aborted 和工具名称，忽略 subagent。仅展示最近 24 小时的会话。
- Codex 日志格式不是稳定公共 API；官方 Hooks 文档：https://learn.chatgpt.com/docs/hooks 。非托管 Hooks 需要用户审核信任，当前版本不修改 Codex hooks 或 notify 配置。
- Claude Stop 与 Codex task_complete 都显示“本轮结束”，不保证业务成功。Codex 等待权限事件尚未验证，不声称能准确显示等待状态。
- 超过 5 分钟没有新事件的工作/等待状态显示“状态待确认”，不会自动认定成功。服务无法仅凭静默日志断言桌面程序是否在线。
- 多会话默认等待确认优先，其次执行中、未读结束。网页可选择固定会话，重启服务恢复自动选择。
- 屏幕不需要点击：“做完了”10 分钟后自动变灰为“空闲”；在该会话继续对话或允许工具后立即更新。等你确认时左侧色条缓慢呼吸，执行中底部细线流动。
- 无额度查询，无云端，无自动调暗，无开机自启动。

## 交互与安全

浏览器编辑/API 仅 localhost；设备使用 /device WebSocket 和本地生成的设备令牌。当前使用明文局域网传输，只适合可信局域网，不要端口转发或暴露公网。data/device.json 包含设备令牌，不要共享。

配置保存至 data/config.json；设备断线时显示离线提示。固件的项目名覆盖基本汉字区，扩展汉字和 emoji 不保证可显示。系统字体生成的本地字体产物仅用于本机设备开发，公开分发前应改为许可合适的字体。

## 构建

固件在 `../firmware`，编译与烧录见 `../firmware/README.md`。

测试：`npm test`；服务启动后 `node integration.cjs`。

数据流：Claude Hooks / Codex 日志 → Node 服务 → WebSocket → ESP32 LVGL；网页 POST 配置 → 服务保存 → 屏幕更新。
