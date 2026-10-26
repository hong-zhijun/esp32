#include "user_config.h"
#include "lvgl_port.h"
#include "esp_err.h"
#include "i2c_bsp.h"
#include "src/lcd_bl_bsp/lcd_bl_pwm_bsp.h"

void setup()
{
  i2c_master_Init();
  Serial.begin(115200);
  lvgl_port_init();
  lcd_bl_pwm_bsp_init(LCD_PWM_MODE_255);
  Serial.println("V2 display and touch demo initialized");
}

void loop()
{
  static unsigned long lastStatus = 0;
  if (millis() - lastStatus >= 5000) {
    lastStatus = millis();
    Serial.printf("V2 demo running: uptime=%lu ms, PSRAM=%u bytes, free heap=%u bytes\n",
                  millis(), ESP.getPsramSize(), ESP.getFreeHeap());
  }
  delay(10);
#if (Backlight_Testing == 1)
  setUpduty(LCD_PWM_MODE_255);
  delay(1500);
  setUpduty(LCD_PWM_MODE_175);
  delay(1500);
  setUpduty(LCD_PWM_MODE_125);
  delay(1500);
  setUpduty(LCD_PWM_MODE_0);
  delay(1500);
#endif
}
