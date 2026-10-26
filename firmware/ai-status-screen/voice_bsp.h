#pragma once
#include <stdbool.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
bool voice_init(void);
bool voice_start(void);
int voice_read(int16_t *mono, int samples);
void voice_stop(void);
#ifdef __cplusplus
}
#endif
