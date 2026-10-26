# 板子固件

- `ai-status-screen/`：AI 状态面板 Arduino sketch（ESP32-S3-Touch-LCD-3.49 V2，640×172 横屏）。
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
