# ESP32-S3-Touch-LCD-3.49

URL: https://docs.waveshare.net/ESP32-S3-Touch-LCD-3.49/

> **版本兼容性说明：背光引脚已更换，请根据版本下载对应示例，否则屏幕将无法点亮**
>
> **注意：** ESP32-S3-Touch-LCD-3.49 V1 版本已停产，自 2026 年 6 月 8 日起统一发货 V2 版本。
>
> **版本识别方式：**
>
> - PCB 丝印：V2 版本带有 "Rev1.1" 丝印
> - 外壳 QC 标签：V2 版本贴有 "V2" 标签
> **V1 与 V2 的主要区别：**
>
> - 新增 TP_INT 引脚引出
> - 优化锂电池充电电流
> - LCD_TE 与 LCD_RESET 引脚互换
> - LCD_BL 与 EXIO_INT 引脚互换

- ESP32-S3-Touch-LCD-3.49（A 款带锂电池版本）
- ESP32-S3-Touch-LCD-3.49-EN（A 款不带锂电池版本）
- ESP32-S3-Touch-LCD-3.49B（B 款带锂电池版本）
- ESP32-S3-Touch-LCD-3.49B-EN（B 款不带锂电池版本）
[![](https://docs.waveshare.net/assets/images/ESP32-S3-Touch-LCD-3.49-DocHeader-2-2669fb1ab64cd743be19858549427792.webp)](https://www.waveshare.net/shop/ESP32-S3-Touch-LCD-3.49-EN.htm)

本产品是一款微雪 (Waveshare) 设计的高性能、高集成的微控制器开发板。板载了 3.49 英寸电容高清 IPS 屏、高度集成的电源管理芯片、六轴传感器 （三轴加速度计与三轴陀螺仪）、RTC、低功耗音频编解码芯片和回声消除电路等外设，方便开发并嵌入到产品中。

**如果你在寻找：**

- **使用 AI 工具编程** ：点击 **发给 AI** 并选择 AI 工具，询问与本页面相关的问题；也可以选择 **复制为 Markdown** ，将页面内容复制并粘贴到 AI 工具中辅助编程。
- **原理图、规格书和示例代码** ：参见 [相关资料](https://docs.waveshare.net/ESP32-S3-Touch-LCD-3.49/Resources-And-Documents)
- **Arduino 开发** ：参见 [Arduino 开发](https://docs.waveshare.net/ESP32-S3-Touch-LCD-3.49/Arduino)
- **ESP-IDF 开发** ：参见 [ESP-IDF 开发](https://docs.waveshare.net/ESP32-S3-Touch-LCD-3.49/ESP-IDF)
- **问题排查** ：参见 [产品 FAQ](https://docs.waveshare.net/ESP32-S3-Touch-LCD-3.49/FAQ) 或联系 [技术支持](https://docs.waveshare.net/ESP32-S3-Touch-LCD-3.49/Technical-Support)

| SKU | 产品 |
| --- | --- |
| 32373 | ESP32-S3-Touch-LCD-3.49 |
| 32374 | ESP32-S3-Touch-LCD-3.49-EN |
| 32375 | ESP32-S3-Touch-LCD-3.49B |
| 32376 | ESP32-S3-Touch-LCD-3.49B-EN |

## 产品特性

- 搭载 ESP32-S3R8 高性能 Xtensa 32 位 LX7 双核处理器，主频高达 240MHz
- 支持 2.4GHz Wi-Fi(802.11 b/g/n) 和 Bluetooth 5(LE) ，板载天线
- 内置 512KB 的 SRAM 和 384KB ROM ，叠封 8MB PSRAM 和 16MB Flash
- 板载 3.49 英寸电容触摸屏和 IPS 屏，172 × 320 分辨率，1.67M 彩色，能清晰地显示彩色图片
- 配备双数字麦克风阵列，可实现降噪和回声消除等音频算法，适合精准的语音识别和近场/远场语音唤醒应用
- 内置 AXS15231B 显示驱动一体芯片，使用 QSPI 和 I2C 接口通信
- 板载 QMI8658 六轴惯性测量单元 (3 轴加速度，3 轴陀螺仪），可检测设备偏转状态、计步等功能
- 板载 PCF85063 RTC 芯片，可方便实现 RTC 功能需求
- 板载 PWR、BOOT 两个可自定义功能的侧边按钮，方便使用按钮进行自定义功能开发
- 板载 3.7V MX1.25 锂电池充电接口
- 板载 Micro SD 卡槽，可灵活扩展存储和快速进行数据传输，适用于数据记录和媒体播放，简化了电路设计
- 预留 22PIN 2.54mm 间距的通讯焊盘，便于用户连接外部模块或进行功能拓展，提升开发灵活性

## 资源简介

![ESP32-S3-Touch-LCD-3.49 资源简介](https://docs.waveshare.net/assets/images/ESP32-S3-Touch-LCD-3.49-HW-d45f2a0617c5f4e05d497ae3533752b7.webp)

1. **ESP32-S3R8** Wi-Fi 和蓝牙 SoC，240MHz 运行频率，叠封 8MB PSRAM
2. **贴片天线** 支持 2.4GHz Wi-Fi(802.11 b/g/n) 和 Bluetooth 5(LE)
3. **TCA9554PWR** 8 位 I2C GPIO 扩展芯片
4. **W25Q128JVSI** 16MB Flash 存储器
5. **ES8311 DAC 音频编码芯片** 高性能低功耗音频数模转换器
6. **ES7210 ADC 音频解码芯片** 高性能低功耗音频模数转换器，支持多路麦克风输入
7. **QMI8658** 三轴加速度+三轴陀螺仪，可用于姿态感知、动作识别等应用
8. **双麦克风阵列设计** 双数字麦克风阵列，支持更丰富的语音交互功能
9. **Micro SD 卡槽** 支持 FAT32 格式的 SD 卡，用于数据扩展
10. **RESET 按键（背面）** 按住 BOOT 按键，单击 RESET 按键可以进入下载模式
11. **PWR 电源键（背面）** 配合程序可实现锂电池供电情况下的电源控制
12. **BOOT 按键（背面）** 按住 BOOT 按键，单击 RESET 按键可以进入下载模式
13. **MX1.25 2PIN 扬声器接口（背面）** 音频输出信号，支持外接扬声器
14. **IPEX 1 代座子** 可通过拆掉电阻切换为使用外部天线
15. **22PIN 2.54mm 间距通孔焊盘** 连接外部模块或进行功能拓展
16. **Type-C 接口** 用于烧录程序和日志打印
17. **PCF85063** RTC 时钟芯片，支持时间保持功能

## LCD 参数

| 参数 | 值 | 参数 | 值 |
| --- | --- | --- | --- |
| 显示面板 | LCD | 显示尺寸 | 3.49 英寸 |
| 显示分辨率 | 172 × 640 | 显示颜色 | 16.7M |
| 显示亮度 | 350 cd/m² | 对比度 | 1200:1 |
| 显示接口 | QSPI | 驱动芯片 | AXS15231B |
| 触摸接口 | I2C | 触摸类型 | 电容式感应 |

## V1 版本接口介绍

![ESP32-S3-Touch-LCD-3.49 接口介绍](https://docs.waveshare.net/assets/images/ESP32-S3-Touch-LCD-3.49-IntfIntro-c98715985a15287c0b786f08eea5bae0.webp)

## V2 版本接口介绍

![ESP32-S3-Touch-LCD-3.49 接口介绍](https://docs.waveshare.net/assets/images/ESP32-S3-Touch-LCD-3.49-IntfIntro2-d2e5d4232ebf65a55f9e056263b4bab9.webp)

## 产品尺寸

### A 款尺寸

![ESP32-S3-Touch-LCD-3.49 产品尺寸](https://docs.waveshare.net/assets/images/ESP32-S3-Touch-LCD-3.49-ProductSize-1-b798e9adb46e2dde7847e5227739baf6.webp)

### B 款尺寸

![ESP32-S3-Touch-LCD-3.49 产品尺寸](https://docs.waveshare.net/assets/images/ESP32-S3-Touch-LCD-3.49-ProductSize-2-53f7565186c1f234cee2f71812a7628d.webp)

## 开发方式

ESP32-S3-Touch-LCD-3.49 支持 Arduino IDE 和 ESP-IDF 两种开发框架，为开发者提供灵活的选择，您可以根据项目需求和个人偏好选择合适的开发工具。

Arduino 通常更容易上手，对初学者和爱好者更加友好。ESP-IDF 提供更完整的开发工具链和更细粒度的系统控制，适合复杂项目或对性能有较高要求的应用。

- **Arduino IDE** 是一款便捷灵活、易于上手的开源电子原型平台。无需太多基础知识，简单学习后即可快速开发。Arduino 拥有庞大的全球用户社区，提供海量开源代码、项目示例和教程，以及丰富的库资源，封装了复杂功能，让开发者能够快速实现各种功能。您可以参考 **[Arduino IDE 开发环境搭建教程](https://docs.waveshare.net/ESP32-S3-Touch-LCD-3.49/Arduino)** 完成初始设置，教程中同时提供了相关示例程序供参考。
- **ESP-IDF** 全称 Espressif IoT Development Framework，是乐鑫科技为 ESP 系列芯片推出的专业开发框架。它基于 C 语言开发，包含编译器、调试器、烧录工具等，支持命令行或集成开发环境（如 Visual Studio Code 配合 Espressif IDF 插件）开发，插件提供代码导航、项目管理、调试等功能。我们推荐使用 VS Code 进行开发，具体配置过程可参考 **[ESP-IDF (VS Code) 开发环境搭建教程](https://docs.waveshare.net/ESP32-S3-Touch-LCD-3.49/ESP-IDF)** ，教程中同时提供了相关示例程序供参考。

[文档反馈](https://wj.qq.com/s2/26077836/3c81/?q-2-rR1G=https%3A%2F%2Fdocs.waveshare.net%2FESP32-S3-Touch-LCD-3.49%2F)