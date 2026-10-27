# docs

- `deepseek-status.md`：DeepSeek Harness 状态接入、三列屏幕布局与本机更新步骤。
- `architecture.md`：当前 Rust 桌面端、状态来源、设备协议和语音桥架构。
- `board-overview.md`：微雪 ESP32-S3-Touch-LCD-3.49 V2 板子介绍（V1/V2 差异、外设、屏幕参数）。
- `reference/`：Arduino 环境搭建教程、开发板设置截图、Claude 桌面端文档等离线资料。
- `research/`：状态屏方案调研（Claude/Codex 用量与状态监控相关开源项目）。
- `waveshare-v2/`：微雪官方 V2 资料包（Arduino/ESP-IDF 示例、出厂固件、原理图）。项目用的 LVGL 8.4 已移到 `../firmware/libraries/lvgl8`。
- `archive/`：前期测试归档，不参与当前构建。
  - `screen-v2-test/`：官方 LVGL 演示改的点屏测试。
  - `environment-check/`：开发环境自检 sketch。
  - `builds/`：历次编译产物，`ai-status-build/` 是当前烧录的固件。
  - `downloads/`：ESP32 3.3.12 离线安装清单、ArduinoJson / arduinoWebSockets 库压缩包。
  - `logs/`：编译、烧录、运行日志。
