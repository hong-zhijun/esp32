#include "voice_bsp.h"
#include <stdlib.h>
#include "driver/i2s_tdm.h"
#include "driver/i2c_master.h"
#include "esp_heap_caps.h"
#include "src/esp_codec_dev/include/esp_codec_dev.h"
#include "src/esp_codec_dev/include/esp_codec_dev_defaults.h"

// ES7210 microphone ADC on the shared I2C bus 0; pins from Waveshare's S3_LCD_3_49 board config.
#define VOICE_MCLK 7
#define VOICE_BCLK 15
#define VOICE_WS 46
#define VOICE_DIN 6
#define VOICE_RATE 16000
#define VOICE_GAIN 30.0
static i2s_chan_handle_t rx;
static esp_codec_dev_handle_t mic;
static int16_t *stereo;static int stereoSamples;
bool voice_init(void){
 i2s_chan_config_t chan_cfg=I2S_CHANNEL_DEFAULT_CONFIG(I2S_NUM_AUTO,I2S_ROLE_MASTER);chan_cfg.auto_clear=true;
 if(i2s_new_channel(&chan_cfg,NULL,&rx)!=ESP_OK)return false;
 i2s_tdm_config_t tdm={
  .clk_cfg=I2S_TDM_CLK_DEFAULT_CONFIG(VOICE_RATE),
  .slot_cfg=I2S_TDM_PHILIPS_SLOT_DEFAULT_CONFIG(32,I2S_SLOT_MODE_STEREO,I2S_TDM_SLOT0|I2S_TDM_SLOT1|I2S_TDM_SLOT2|I2S_TDM_SLOT3),
  .gpio_cfg={.mclk=VOICE_MCLK,.bclk=VOICE_BCLK,.ws=VOICE_WS,.dout=I2S_GPIO_UNUSED,.din=VOICE_DIN},
 };
 tdm.slot_cfg.total_slot=4;
 if(i2s_channel_init_tdm_mode(rx,&tdm)!=ESP_OK)return false;
 i2c_master_bus_handle_t bus=NULL;if(i2c_master_get_bus_handle(0,&bus)!=ESP_OK)return false;
 audio_codec_i2s_cfg_t i2s_cfg={.port=0,.rx_handle=rx};
 const audio_codec_data_if_t *data_if=audio_codec_new_i2s_data(&i2s_cfg);
 audio_codec_i2c_cfg_t i2c_cfg={.port=0,.addr=ES7210_CODEC_DEFAULT_ADDR,.bus_handle=bus};
 const audio_codec_ctrl_if_t *ctrl_if=audio_codec_new_i2c_ctrl(&i2c_cfg);
 es7210_codec_cfg_t es7210_cfg={.ctrl_if=ctrl_if,.mic_selected=ES7120_SEL_MIC1|ES7120_SEL_MIC2|ES7120_SEL_MIC3|ES7120_SEL_MIC4};
 const audio_codec_if_t *codec_if=es7210_codec_new(&es7210_cfg);
 if(!data_if||!ctrl_if||!codec_if)return false;
 esp_codec_dev_cfg_t dev_cfg={.dev_type=ESP_CODEC_DEV_TYPE_IN,.codec_if=codec_if,.data_if=data_if};
 mic=esp_codec_dev_new(&dev_cfg);
 return mic!=NULL;
}
bool voice_start(void){
 if(!mic)return false;
 esp_codec_dev_sample_info_t fs={.bits_per_sample=16,.channel=2,.sample_rate=VOICE_RATE};
 if(esp_codec_dev_open(mic,&fs)!=ESP_CODEC_DEV_OK)return false;
 esp_codec_dev_set_in_gain(mic,VOICE_GAIN);return true;
}
// Reads stereo frames and averages both microphones into 16 kHz mono.
int voice_read(int16_t *mono,int samples){
 if(stereoSamples<samples){free(stereo);stereo=(int16_t*)heap_caps_malloc(samples*2*sizeof(int16_t),MALLOC_CAP_SPIRAM);stereoSamples=stereo?samples:0;}
 if(!stereo||esp_codec_dev_read(mic,stereo,samples*2*sizeof(int16_t))!=ESP_CODEC_DEV_OK)return 0;
 for(int i=0;i<samples;i++)mono[i]=(int16_t)(((int32_t)stereo[i*2]+stereo[i*2+1])/2);
 return samples;
}
void voice_stop(void){if(mic)esp_codec_dev_close(mic);}
