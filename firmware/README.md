# 板子固件

- `ai-status-screen/`：AI 状态面板 Arduino sketch（ESP32-S3-Touch-LCD-3.49 V2，640×172 横屏）。fw=6 显示 Claude / ChatGPT / DeepSeek 三列状态、名称左侧标志和单行标题省略，右侧保留 128 像素录音区；兼容旧版两列状态消息。ChatGPT 列沿用 `codex` 状态来源与配色字段。
- `libraries/lvgl8/`：项目固定的 LVGL 8.4（`lv_conf.h` 已开启 `LV_FONT_FMT_TXT_LARGE`）。不要用 Arduino 全局库里的 LVGL 9。

另需全局安装 ArduinoJson 7.x 和 arduinoWebSockets 2.7.x（压缩包在 `../docs/archive/downloads/`），开发板包 esp32 3.3.12。

## 编译与烧录

板子参数：ESP32S3 Dev Module、USB CDC On Boot Enabled、QIO 80MHz、16MB Flash、16M Flash (3MB APP/9.9MB FATFS)、OPI PSRAM。

在项目根目录执行（arduino-cli 位于 Arduino IDE 安装目录 `resources/app/lib/backend/resources/`）：

```bash
arduino-cli compile --fqbn "esp32:esp32:esp32s3:CDCOnBoot=cdc,FlashSize=16M,PartitionScheme=app3M_fat9M_16MB,PSRAM=opi" --library firmware/libraries/lvgl8/lvgl firmware/ai-status-screen
```

```bash
arduino-cli upload -p COM3 --fqbn "esp32:esp32:esp32s3:CDCOnBoot=cdc,FlashSize=16M,PartitionScheme=app3M_fat9M_16MB,PSRAM=opi" firmware/ai-status-screen
```

用 Arduino IDE 时，需把 `libraries/lvgl8` 下的 `lvgl` 与 `lv_conf.h` 作为当前 LVGL 库，避免与全局 LVGL 9 冲突。

烧录前关闭串口监视器；配网与电脑端服务见 `../desktop/README.md`。

## 实际 UI 原生回归

`tests/lvgl-row/build.ps1` 使用 MSVC 编译项目实际 LVGL、三个固件字体和 `panel_ui.cpp`，只替代背光硬件函数，渲染到本地 PPM，不连接串口或打开窗口。默认合成数据覆盖长中文、英文、混排、`#` 和空行，也可用 `-SnapshotFile` 传入电脑端实际状态帧。验证 100 次刷新、三种宽度和位置交换下，所有会话行保持 19 像素、保留标题首字且不重叠。

旧实现让 `LV_LABEL_LONG_DOT` 标签自动计算高度，长标题可撑高到 38 像素，但行距仅 20 像素，造成重叠。fw=4 将圆点与纯文本标签分开，固定单行高度，超宽标题显示前缀和省略号。

fw=5 在三个名称左侧加入 20×20 的 Claude、Codex、DeepSeek 标志。标志使用 8 位透明度蒙版，三张共 1,200 字节，按各栏配色着色；Claude/Codex 交换时标志随名称移动。SVG 来源、固定版本和许可在 `assets/provider-logos/`，通过 `tools/generate-provider-logos.cjs` 生成 `provider_logos.h`。实际 UI 原生测试覆盖最窄/默认/最宽分栏下名称完整显示、标志对应关系和配色同步。

fw=6 将屏幕第二列的名称改为 ChatGPT，图标换为 GPT/OpenAI 结形标志；协议中的 `provider=codex`、`codexColor` 和任务来源保持兼容。交换前两列时，ChatGPT 名称和 GPT 标志一起移动。
