# DeepSeek Harness 三列状态屏

屏幕顺序：Claude | ChatGPT | DeepSeek | 录音。ChatGPT 是第二列的新显示名称，仍由 Codex 任务提供状态，协议与配色字段继续使用 `codex` / `codexColor`。fw=8 状态区 548×172，右侧 92×172 为录音 / 回车图标按钮区。默认三列约 183 像素，窄列字体自动缩小，任务名按可用宽度省略。DeepSeek 第三列固定，现有比例和交换设置只作用于前两列。

fw=7 起每秒按电脑端 `running` 进程检测结果隐藏未运行的来源。只打开一个程序时该栏占满状态区，其他两栏组合等宽；Claude 和 ChatGPT 同时显示时沿用分栏比例及交换设置。全部退出时只保留录音区。历史会话不删除，最小化或后台运行不隐藏；旧服务缺少 `running` 字段时维持旧布局。

三者的会话行都优先显示对话标题，没有标题才回退到项目名称。Claude 接入本地标题文件和标题元数据，DeepSeek 使用 `session/title`；改名不影响计时或完成提示。

Windows 的 Claude 状态同时跟踪普通 AppData 与 Claude MSIX 隔离目录的实时 Hooks 日志，避免迁移后只显示旧副本。无需为了此修复重新安装 Hooks、配网或烧录固件。

## 状态来源

电脑端只读 `$DSH_HOME/sessions`，未指定时使用 `~/.dsh/sessions`。支持当前 Harness v4 的独立追加 Zstandard 帧和未压缩 JSONL；每 5 秒扫描、每秒增量读取，每轮共享 2 MiB 读取预算。完整帧逐个解码，尚未完成的尾帧保留等待；限制压缩帧 16 MiB、解压后 32 MiB。损坏数据会报告来源异常，不会反复重放旧完成通知。

新发现的文件只纳入最近 24 小时更新的会话；`tracked-deepseek.json` 保持重启后的跟踪。排除 `origin=subagent` 和 `delegationDepth>0`。只把标题、项目、工具名、会话 ID、时间和状态传到屏幕；提示词、工具参数和回复正文不保存到面板日志或配置。

| Harness 事件 | 状态屏行为 |
|---|---|
| `turn/start` | 执行中，重置本轮计时 |
| `tool/call`、步骤进展 | 更新工具与活跃时间 |
| `approval/asked` | 等你确认 |
| `approval/decided` | 所有权限请求已处理后恢复执行中 |
| `session/title` | 更新标题，保持活动时间与完成提示 |
| `turn/end`，`reason.kind=completed` | 本轮结束，不保证业务成功 |
| `turn/end`，`aborted` / `interrupted` | 已中断 |
| 错误、未知结束原因、5 分钟无进展 | 待确认 |

## 本机更新

可执行更新包在 `build/deepseek-update/`。代码与构建期间不会操作现有窗口、串口或替换运行中的服务。

1. 从系统托盘菜单退出状态屏程序，关闭 Arduino 串口监视器，保持板子 USB 连接。
2. 运行更新包里的 `update-screen-and-desktop.cmd`（默认 COM3；其他串口可传参数）。
3. 脚本先验证文件 SHA-256、现有配对身份指纹、目标程序路径和串口；备份原程序与配置，使用原分区布局烧录，然后在原可执行路径替换并启动桌面端。它不生成新令牌，不修改防火墙，不执行全片擦除。
4. 等待屏幕自动重连，设备信息中的固件版本应为 6。分别在三个来源开始/结束任务，检查 DeepSeek 的权限等待、多任务排序、固定与清除完成提示。

`powershell -NoProfile -ExecutionPolicy Bypass -File build/deepseek-update/apply-local-update.ps1 -ValidateOnly` 只做更新前校验，不写文件或打开串口。更新包记录的是准备时确认的本机路径，换电脑或改配对后应重新生成，不能直接复制套用。

已经安装三列固件后，只更新标题功能可运行 `update-desktop-only.cmd`。它沿用相同的程序路径与配对校验，只更新桌面程序，不打开串口或重新烧录。只读校验时可同时传 `-DesktopOnly -ValidateOnly`。

修复“长标题挤到下一行、显示不全”需安装 fw=4：运行 `update-screen-only.cmd`，保持电脑服务运行、板子 USB 连接并关闭串口监视器。脚本只烧录屏幕，不替换或重启桌面服务。对应只读校验参数为 `-FirmwareOnly -ValidateOnly`。此前电脑端标题优先和 Claude 实时状态修复仍保留。

fw=5 包含上述修复，并在三个名称左侧增加对应标志：Claude 星芒、Codex 标志、DeepSeek 鲸鱼。使用同一个 `update-screen-only.cmd` 更新；标志随对应名称和栏位配色变化。

fw=6 把第二列的名称和图标改为 ChatGPT / GPT 标志，沿用 Codex 状态数据；同样运行 `update-screen-only.cmd` 更新屏幕即可。

旧固件兼容新电脑端状态帧，只显示前两列；新固件兼容旧电脑端两卡片协议并自动恢复两列。旧固件回退需使用之前的编译产物，电脑端旧程序保存在原路径旁的 `.backup-*` 文件。

## 验证范围

自动测试覆盖压缩多帧、半帧追加、日志截断/损坏、主子会话过滤、权限请求并发、标题更新、状态映射、重启跟踪、三卡片协议与旧配置迁移。独立无界面浏览器检查桌面三列和 420 像素窄窗口。只读本机会话测试确认可解析真实 Harness 日志；实际屏幕效果和实时权限流仍需更新后实机验收。
