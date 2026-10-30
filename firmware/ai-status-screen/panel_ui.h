#pragma once
#include <stdbool.h>
#ifdef __cplusplus
extern "C" {
#endif
void panel_ui_init(void);
// Consume touch requests in the main loop, outside the LVGL task lock.
bool panel_ui_take_voice_request(void);
bool panel_ui_take_enter_request(void);
void panel_ui_set_locked(bool locked);
bool panel_ui_is_locked(void);
void panel_ui_json(const char *json);
void panel_ui_connection(const char *message);
enum { PANEL_WIFI_OFFLINE, PANEL_WIFI_CONNECTING, PANEL_WIFI_CONNECTED };
// Device telemetry is independent of desktop state frames; batteryPercent=-1 means unavailable.
void panel_ui_status(int wifiState, int rssi, bool computerConnected, int batteryPercent);
void panel_ui_device_info(const char *ssid, const char *ip, int rssi, int batteryMv);
enum { VOICE_HIDDEN, VOICE_LISTENING, VOICE_RECOGNIZING, VOICE_DONE, VOICE_ERROR };
void panel_ui_voice(int mode, const char *title, const char *text);
void panel_ui_voice_partial(const char *text);
#ifdef __cplusplus
}
#endif
