#pragma once
#include <stdbool.h>
#ifdef __cplusplus
extern "C" {
#endif
void panel_ui_init(void);
void panel_ui_json(const char *json);
void panel_ui_connection(const char *message);
enum { VOICE_HIDDEN, VOICE_LISTENING, VOICE_RECOGNIZING, VOICE_DONE, VOICE_ERROR };
void panel_ui_voice(int mode, const char *title, const char *text);
#ifdef __cplusplus
}
#endif
