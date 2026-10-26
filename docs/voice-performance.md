# 语音性能排查（macOS / ESP32-S3）

## 证据与已实施修复

首次网页日志：32.309 秒实际录音只收到 23.22 秒 PCM，最大音频包间隔 4063 ms。识别 54–115 ms，排队 0–1 ms，粘贴通常 565–616 ms。

检查本机 ESP32 Arduino 3.3.12 的 `cores/esp32/HWCDC.cpp`：默认发送超时 100 ms，背压时连续重试上限 20。USB 已连接而主机不消费日志时，一次 write 可等待约 2 秒。固件原先每 10 秒连续调用两次 Serial.printf，与观察到的约 4 秒停顿吻合。该结论仍需真实录音前后对照确认。

- `Serial.setTxTimeoutMs(0)`，日志不能阻塞录音；配网仍保留 CONFIG_SAVED 回复。
- 移除周期性串口打印，改为每 2 秒发送受限字段的 WebSocket 性能遥测。
- 界面计时由 LVGL timer 驱动；音频采集块 20 ms；I2S 读取超时上限 100 ms；关闭 Wi-Fi 省电。
- 识别使用 sherpa-onnx `OfflineRecognizer.decodeAsync` 并串行调度，避免阻塞 Node 事件循环。
- 自动粘贴恢复剪贴板的等待由 400 ms 降为 100 ms。目标应用极忙时仍需人工验证是否完整输入。

## 文档和实现依据

- 仓库 `firmware/ai-status-screen/src/esp_codec_dev/README.md`：codec open/read/close 接口及录音示例，read 为同步接口。
- 已安装 Arduino core `HWCDC.cpp`：USB TX 背压、超时和重试上限注释，`HWCDC.h` / `USBCDC.h`：setTxTimeoutMs API。
- 已安装 arduinoWebSockets 2.7.2 `WebSockets.cpp`：同步 TCP write；`WebSocketsClient.cpp` 已开启 TCP_NODELAY。
- `desktop/node_modules/sherpa-onnx-node/non-streaming-asr.js`：decodeAsync 为异步离线推理接口，并非流式模型。

## 验证方法

不打开串口监视器，在网页查看控制台。连续录音至少 30 秒，跨过多个原来的 10 秒日志周期。比较实际时间和音频时长、最大包间隔、设备循环/采集/发送耗时，以及识别与粘贴耗时。仍有网络长阻塞时，再用独立采集任务和有界缓冲隔离网络；不以改变计时显示掩盖音频丢失。

当前仍为短句离线识别，不是逐字流式听写。强制两秒切段可能切断词语。若追求逐字稳定输出，应另选流式模型并用真实中文样本对照准确率与延迟，不能只缩短切段间隔。
