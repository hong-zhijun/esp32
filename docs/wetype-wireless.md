# ESP32 无线麦克风 → 微信输入法

本次目标：ESP32 通过 Wi-Fi 发送麦克风音频；电脑接入虚拟麦克风，由微信输入法实时输入。运行时不需要 USB 数据连接，不调用本项目 SenseVoice 模型。ESP32-S3 不支持传统蓝牙耳机所需的 Bluetooth Classic HFP，因此采用现有 Wi-Fi 音频链路。

## 实际进度

- 已验证 Mac 微信输入法 3.0.0 的语音设置支持指定麦克风、全局语音启动快捷键；页面说明按任意键结束语音。
- 已实现跨平台 PCM 音频转接：16 kHz 单声道转换到 48 kHz 双声道，80 ms 初始缓冲，最多 2 秒排队，超限明确报错，不积压到越来越慢。
- 开始录音：打开虚拟输出设备，触发平台快捷键（Mac：Fn＋空格；Windows：F8），缓冲开头音频，待输入法启动后送音频。
- 结束录音：送完尾音，再发送同一快捷键结束；不按 Return，不自动发送聊天消息。
- 断开设备、虚拟声卡错误、启动超时均停止音频；音频桥不读取电脑内置麦克风。
- 默认微信模式不加载本地模型。`VOICE_BACKEND=local npm start` 可临时回到旧版识别。
- 音频桥接和生命周期测试通过不代表微信端到端输入已经验证。

**重启后验证：** BlackHole 已安装，服务已恢复，ESP32 已通过 Wi-Fi 重连。微信输入法麦克风已选为 BlackHole 2ch。向音频桥输入 1 秒合成音并从 BlackHole 回读，确认收到信号，输出无欠载。

**Mac 端到端已验证：** 用户已设为 Fn＋空格，Mac 端已编译对应按键辅助程序，现有辅助功能权限检查通过。真实 ESP32 麦克风测试由 BOOT 开始、屏幕触摸结束；微信语音浮窗正常出现，网页测试框实际收到中文文字。日志记录 8.84 秒音频、0 次虚拟音频欠载，正常结束，无语音错误。

**验证范围：** 上述结果来自真实设备和用户确认，并读取网页输入框核实；不以合成测试或模拟快捷键成功代替。Windows 分支尚无实机验证。

## Mac 一次性设置

1. 安装 `desktop/installers/BlackHole2ch-0.7.1.pkg`。包由 Homebrew 官方 cask 下载，macOS Gatekeeper 检查为已公证的 Existential Audio Inc. (Q5C99V536K)。SHA-256：`57b540f27a3e29c37e310e01bee0fdfab76733087e47f997ef9dccf851400dcf`。在安装器中自行授权；若安装后设备未出现，按厂商说明重启。
2. 微信输入法设置 → 语音输入 → 麦克风选择 **BlackHole 2ch**。不必更改系统扬声器，不要把整台电脑声音都送进 BlackHole。
3. 同一页“启动语音输入”设为 **Fn＋空格**，保持“快捷键全局生效”（本机已设置）。Mac 安装脚本会编译 `bridge/wetype-key.c` 到 `build/wetype-key`，需要 Xcode Command Line Tools 和启动服务应用的辅助功能权限。
4. 音频依赖已安装到 `desktop/.venv`。其他 Mac 可运行 `bash bridge/setup-mac.sh`。
5. `cd desktop && npm start`。网页点击“重新检查虚拟麦克风”确认设备可用。
6. 光标放到网页语音测试框，按 ESP32 开始，讲话，再按结束。微信输入法应将文字直接输入测试框。若出现系统权限提示，仅由用户确认需要的麦克风/辅助功能权限。

微信输入法可能联网处理语音，具体行为遵循其设置及隐私政策。控制台只确认音频转发及快捷键已发送，不能把快捷键发送成功冒充为微信识别成功。

## Windows 一次性设置

1. 安装 Python 3、Node.js 和支持语音的微信输入法。
2. 从厂商页面 https://vb-audio.com/Cable/ 下载 VB-CABLE，使用厂商签名安装器完成管理员安装及必要重启。
3. 项目电脑端执行 `powershell -ExecutionPolicy Bypass -File bridge/setup-windows.ps1`。
4. 桥接输出设备是 **CABLE Input**；微信输入法麦克风选择对应的 **CABLE Output**。二者名称方向容易混淆。
5. 微信“启动语音输入”全局快捷键设为 **F8**。需要在具体 Windows 微信版本上验证开始/停止键的语义。
6. `npm ci`、`npm start`，用网页测试框做第一次测试。允许局域网 Node 服务接收 ESP32 连接。

## 诊断与配置

- `desktop/.venv/bin/python desktop/bridge/audio-output.py --list`：枚举输出设备（Mac）。
- `desktop/.venv/bin/python desktop/bridge/audio-output.py --check`：只检查虚拟设备，不发送任何快捷键。
- `VOICE_OUTPUT_DEVICE` 可指定虚拟输出设备名称；`VOICE_PYTHON` 可指定 Python 路径。
- 桥接永不自动回退到默认扬声器，缺设备会直接报错。
- 微信输入法直接操作当前焦点输入框。开始后不要切换窗口，结束后再进行其他操作。
- 虚拟麦克风测试完成前不应把“无报错”当作输入成功。必须在测试框里实际看到预期文字。

## 从电脑结束录音

Mac 端增加 `bridge/wetype-status.c`，通过 Core Audio 的微信输入法进程采音状态同步结束。只在本项目的录音会话期间观察，不读取按键或录音内容。先确认微信开始采音，再等待连续 250 ms 的停止状态（每 100 ms 检查一次），通知 ESP32 停止麦克风并恢复右侧按钮。电脑已结束时丢弃残留 PCM，不再发送 Fn＋空格，避免重新打开语音。

此检测需要系统支持 Core Audio Process Objects；读取失败只记录诊断，不将“检测失败”当作“已经关闭”。Windows 暂未接入反向停止检测，仍可使用屏幕结束。此次修改已通过 7 项生命周期测试，固件已编译烧录。2026-10-27 实机 Esc 测试通过：用户从屏幕开始录音，日志确认微信开始采音，约 5.06 秒音频后检测到电脑结束语音（wetype-external-stop）；随后 ESP32 连续上报 recording=false，音频读取/发送错误均为 0。点击关闭浮窗使用同一采音状态检测，尚未单独实测。
