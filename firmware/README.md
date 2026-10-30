# 板子固件

- `ai-status-screen/`：AI 状态面板 Arduino sketch（ESP32-S3-Touch-LCD-3.49 V2，640×172 横屏）。fw=15 按电脑端的实时运行检测显示 Claude / ChatGPT / DeepSeek 栏目，退出后隐藏、剩余栏目自动铺满 500 像素状态区；采用黑底、16 像素名称与 20×20 标志、28 像素状态、20 像素单行当前任务标题和底部工具 / 时长，右侧 92 像素按钮区（上下两个 76×76 录音 / 回车图标按钮），最右侧 48 像素 Wi-Fi / 竖电池状态栏，点击可查看设备详情。兼容旧版两列/三列消息。ChatGPT 列沿用 `codex` 状态来源与配色字段。
- `libraries/lvgl8/`：项目固定的 LVGL 8.4（`lv_conf.h` 已开启 `LV_FONT_FMT_TXT_LARGE`）。不要用 Arduino 全局库里的 LVGL 9。

另需全局安装 ArduinoJson 7.x 和 arduinoWebSockets 2.7.x（压缩包在 `../docs/archive/downloads/`），开发板包 esp32 3.3.12。

## 编译与烧录

板子参数：ESP32S3 Dev Module、USB CDC On Boot Enabled、QIO 80MHz、16MB Flash、16M Flash（项目 partitions.csv：双 6MB APP / 约 3.9MB FATFS）、OPI PSRAM。

在项目根目录执行（arduino-cli 位于 Arduino IDE 安装目录 `resources/app/lib/backend/resources/`）：

```bash
arduino-cli compile --build-property upload.maximum_size=6291456 --fqbn "esp32:esp32:esp32s3:CDCOnBoot=cdc,FlashSize=16M,PartitionScheme=app3M_fat9M_16MB,PSRAM=opi" --library firmware/libraries/lvgl8/lvgl firmware/ai-status-screen
```

```bash
arduino-cli upload -p COM3 --fqbn "esp32:esp32:esp32s3:CDCOnBoot=cdc,FlashSize=16M,PartitionScheme=app3M_fat9M_16MB,PSRAM=opi" firmware/ai-status-screen
```

用 Arduino IDE 时，需把 `libraries/lvgl8` 下的 `lvgl` 与 `lv_conf.h` 作为当前 LVGL 库，避免与全局 LVGL 9 冲突。

烧录前关闭串口监视器；配网与电脑端服务见 `../desktop/README.md`。

## 实际 UI 原生回归

macOS / Linux 可运行 `python3 firmware/tests/lvgl-row/build.py`；ArduinoJson 路径不同时传入 `--arduino-json /path/to/ArduinoJson/src`。测试使用实际 LVGL、字体和生产 `panel_ui.cpp`，覆盖全部 8 种程序运行组合、交换位置、分栏比例、退出/重开、录音区保留及旧协议兼容；渲染产物保存在 `build/lvgl-row-host/`。

`tests/lvgl-row/build.ps1` 使用 MSVC 编译项目实际 LVGL、三个固件字体和 `panel_ui.cpp`，只替代背光硬件函数，渲染到本地 PPM，不连接串口或打开窗口。默认合成数据覆盖长中文、英文、混排、`#` 和空行，也可用 `-SnapshotFile` 传入电脑端实际状态帧。验证 100 次刷新、三种宽度和位置交换下，所有会话行保持 19 像素、保留标题首字且不重叠。

旧实现让 `LV_LABEL_LONG_DOT` 标签自动计算高度，长标题可撑高到 38 像素，但行距仅 20 像素，造成重叠。fw=4 将圆点与纯文本标签分开，固定单行高度，超宽标题显示前缀和省略号。

fw=5 在三个名称左侧加入 20×20 的 Claude、Codex、DeepSeek 标志。标志使用 8 位透明度蒙版，三张共 1,200 字节，按各栏配色着色；Claude/Codex 交换时标志随名称移动。SVG 来源、固定版本和许可在 `assets/provider-logos/`，通过 `tools/generate-provider-logos.cjs` 生成 `provider_logos.h`。实际 UI 原生测试覆盖最窄/默认/最宽分栏下名称完整显示、标志对应关系和配色同步。

fw=6 将屏幕第二列的名称改为 ChatGPT，图标换为 GPT/OpenAI 结形标志；协议中的 `provider=codex`、`codexColor` 和任务来源保持兼容。交换前两列时，ChatGPT 名称和 GPT 标志一起移动。

fw=8 将右侧录音区缩为 92 像素，上下排列两个 76×76 正方形图标按钮（上录音、下回车），无文字或计时标签。状态区扩大为 548×172；回车需配合支持 `type=key, key=Enter` 的新版电脑端。录音波形和颜色继续表示录音 / 等待 / 错误状态。

## 极简仪表（fw=9）

纯黑背景融入屏幕黑边框，去掉粗色条与底部跑马线，改用细分隔线和轻微呼吸的状态圆点。三栏品牌名统一柔白，18×18 标志保留各平台配色，ChatGPT 默认纯白。状态为 24 像素，显示当前选中 / 固定任务的一行标题，长标题省略；工具和时长分列底部，窄栏优先保留时长。更多会话在客户端查看。右侧保留 76×76 深灰录音 / 回车按钮，录音中以红色波形提示。状态字体包含全部状态用字，启动、退出、两栏交换和自定义配色继续生效。

## 字号与间距（fw=10）

状态字放大到 28px，任务标题为 20px，平台名和时长为 16px，logo 为 20×20。状态、标题与底部信息的垂直间距为 10px，左右边距为 12px。标题使用压缩的 20px 单色中文字库；底部使用精简 16px 字库，缺少的字回退到完整中文字库。两个方形按钮维持 76×76。

## 字体抗锯齿（fw=11）

任务标题维持 20px，改用 PingFang SC Regular 的 4bpp（16 级灰阶）压缩字形，覆盖基本汉字及中英文标点，替代 fw=10 的单色字形。字库来源为本机 macOS PingFang；重新生成应使用有相应授权的字体。主机测试读取实际字形位图，断言存在中间灰阶，防止再次误用单色字库。

项目自带 partitions.csv，将两个程序分区扩为各 6MB；16MB Flash 大小不变。NVS 仍为 0x9000–0xDFFF，app0 仍从 0x10000 开始，已有 Wi-Fi 与配对数据保留。当前固件不使用 FAT 文件区；新 FAT 起点为 0xC10000。使用上述编译命令设置最大程序长度；Arduino IDE 也会选用 sketch 内的分区表。

fw=13 顶部平台名称跟随对应 logo 的颜色，包括客户端自定义配色及栏目交换；中间任务名称为柔白色。

## 设备状态栏（fw=14）

屏幕分为 500px 任务区、92px 按钮区和 48px 设备状态栏。最右侧使用 40×160 深灰圆角底，上方为 3px 粗线 Wi-Fi 图标，下方为 22×34 竖电池与 14px 百分比。三栏不等分时为前两栏各保留至少 128px，避免名称被挤掉；两个按钮仍为 76×76。

Wi-Fi 弧线按 RSSI 分三级（≥−60 / ≥−70 / 更弱），重连时显示暗弧线和琥珀色圆点，断开时加斜杠。已连 Wi-Fi 而电脑服务未连接时图标为琥珀色。设备每秒自行更新，不依赖电脑推送。

电池使用 GPIO4 的校准 ADC 毫伏值乘以 3，依据[官方 V2 ADC 示例](https://gitee.com/waveshare/esp32-s3-touch-lcd-3.49-v2/blob/master/Arduino/examples/01_ADC_Test/adc_bsp.cpp)。每次平均 8 个读数，再做 1/8 指数平滑；按单节锂电池电压曲线估算百分比，≤20% 琥珀色、≤10% 红色。读数低于 2.8V 或高于 4.35V 时显示 `--`。电压估算受负载与充电影响，不是电量计读数，也不能仅凭 ADC 保证识别未接电池或充电状态。USB `{"type":"status"}` 返回 `fw`、`batteryMv`、`batteryPercent` 以便实机核对。

## 点击查看设备详情（fw=15）

点击最右侧 Wi-Fi / 电池状态栏，打开深灰圆角详情面板，分两列显示 Wi-Fi 名称、RSSI 信号、设备 IP、电脑连接状态、电池电压和估算电量，数据每秒由设备更新。长 Wi-Fi 名称单行省略，断网时 IP 显示 `--`，信号显示连接中或未连接，无有效电池数据时电压和电量显示 `--`。

再次点击状态栏、右上角“关闭”或面板外的边缘空白可收起。打开期间面板遮住任务和录音 / 回车区并拦截触摸，任务和录音状态继续更新；手动锁屏会关闭详情，锁屏时不能打开。实际 LVGL 原生回归使用指针点击覆盖上述行为，并生成 `build/lvgl-row/device-details.ppm` 供排版检查。
