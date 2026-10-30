// Run the actual vendored LVGL label code with the actual firmware font. No
// hardware, GUI windows, serial ports or substitute text renderer are involved.
#include "lvgl.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "panel_ui.h"
extern void stress_panel_rows(const char *json);
extern void stress_panel_visibility(const char *json);
extern void verify_panel_controls(void);
extern void verify_title_overflow(void);
extern void verify_device_status(void);
extern void verify_device_details(void);
extern void toggle_device_details_preview(void);
void setUpduty(uint16_t duty) { (void)duty; }

static lv_color_t pixels[640 * 172];
static lv_color_t draw_pixels[640 * 172];
static void flush(lv_disp_drv_t *driver, const lv_area_t *area, lv_color_t *colors) {
    for(int y=area->y1;y<=area->y2;y++)for(int x=area->x1;x<=area->x2;x++)pixels[y*640+x]=*colors++;
    lv_disp_flush_ready(driver);
}

static void save(const char *path) {
    lv_refr_now(NULL);FILE *file=fopen(path,"wb");assert(file);
    fprintf(file,"P6\n640 172\n255\n");
    for(int i=0;i<640*172;i++) {lv_color32_t c;c.full=lv_color_to32(pixels[i]);fputc(c.ch.red,file);fputc(c.ch.green,file);fputc(c.ch.blue,file);}
    fclose(file);
}

int main(int argc,char **argv) {
    lv_init();
    static lv_disp_draw_buf_t draw;lv_disp_draw_buf_init(&draw,draw_pixels,NULL,640*172);
    static lv_disp_drv_t driver;lv_disp_drv_init(&driver);driver.hor_res=640;driver.ver_res=172;driver.draw_buf=&draw;driver.flush_cb=flush;lv_disp_drv_register(&driver);
    lv_obj_set_style_bg_color(lv_scr_act(),lv_color_hex(0),0);
    panel_ui_init();
    verify_panel_controls();
    verify_device_status();
    verify_device_details();
    char json[16384];
    if(argc>1){FILE *input=fopen(argv[1],"rb");assert(input);size_t count=fread(json,1,sizeof(json)-1,input);json[count]=0;fclose(input);}
    else strcpy(json,"{\"type\":\"state\",\"config\":{\"split\":50},\"cards\":[{\"provider\":\"claude\",\"state\":\"working\",\"rows\":[{\"state\":\"working\",\"project\":\"这是一段超过屏幕宽度的中文对话标题\"},{\"state\":\"done\",\"project\":\"A very long conversation title\"},{\"state\":\"idle\",\"project\":\"短标题\"}]},{\"provider\":\"codex\",\"state\":\"done\",\"rows\":[{\"state\":\"done\",\"project\":\"阅读 ESP32 代码\"},{\"state\":\"idle\",\"project\":\"标题包含 #ff0000 字符\"}]},{\"provider\":\"deepseek\",\"state\":\"waiting\",\"rows\":[{\"state\":\"waiting\",\"project\":\"等待确认的 DeepSeek 长标题\"}]}]}");
    for(int frame=0;frame<3;frame++) {
        panel_ui_json(json);lv_obj_update_layout(lv_scr_act());lv_tick_inc(1000);lv_timer_handler();
    }
    save("actual-panel.ppm");
    // Render the production modal with synthetic, non-sensitive device data.
    toggle_device_details_preview();
    save("device-details.ppm");
    toggle_device_details_preview();
    panel_ui_status(PANEL_WIFI_OFFLINE,0,false,10);save("offline-low-battery.ppm");
    panel_ui_status(PANEL_WIFI_CONNECTED,-52,true,80);
    panel_ui_voice(VOICE_LISTENING,"","");
    lv_tick_inc(200);lv_timer_handler();save("recording-panel.ppm");
    panel_ui_voice(VOICE_HIDDEN,"","");
    stress_panel_rows(json);
    stress_panel_visibility(json);
    save("visible-panel.ppm");
    verify_title_overflow();
    puts("PASS: minimal production UI, 100 updates, 3 split widths, selected task title, UTF-8 truncation, status glyphs, 20px logos, footer bounds and visibility");
    puts("PASS: all 8 running-app combinations, swapping, split widths, full status width, voice area, legacy two-card frames and reopening");
    puts("PASS: 76px square icon buttons, touch dispatch/coalescing, lock, waveform bounds and recording cleanup state");
    puts("PASS: 48px status rail, signal levels, offline/connecting/server states, low/unknown/full battery and voltage curve");
    return 0;
}
