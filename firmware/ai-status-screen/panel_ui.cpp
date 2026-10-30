#include "panel_ui.h"
#include "provider_logos.h"
#include "lvgl.h"
#include <ArduinoJson.h>
#include "src/lcd_bl_bsp/lcd_bl_pwm_bsp.h"
#include <stdio.h>
#include <string.h>
#include <atomic>
LV_FONT_DECLARE(panel_font_20);
LV_FONT_DECLARE(panel_font_28);
LV_FONT_DECLARE(panel_font_meta_16);

static constexpr int PANEL_COUNT=3,CONTROL_WIDTH=92,STATUS_WIDTH=48,BUTTON_SIZE=76,CONTENT_WIDTH=640-CONTROL_WIDTH-STATUS_WIDTH;
static constexpr int PAD=12,BRAND_X=38,BRAND_Y=24,STATE_Y=54,TITLE_Y=91,FOOTER_Y=124;
static constexpr uint32_t TEXT_COLOR=0xeceef3,MUTED_COLOR=0x929ba9,DIVIDER_COLOR=0x242930;
static lv_obj_t *panels[PANEL_COUNT],*brands[PANEL_COUNT],*states[PANEL_COUNT],*titles[PANEL_COUNT];
static lv_obj_t *details[PANEL_COUNT],*elapsed[PANEL_COUNT],*markers[PANEL_COUNT],*brandIcons[PANEL_COUNT],*banner;
static char lastState[PANEL_COUNT][16];
// Display names and logo colors are independent of the stable wire provider IDs.
static const char *providerNames[PANEL_COUNT]={"Claude","ChatGPT","DeepSeek"};
static const uint32_t providerColors[PANEL_COUNT]={0xdf9877,0xffffff,0x7c9fff};
static int lastBrightness=-1;
static std::atomic<bool> screenLocked{false};
static lv_obj_t *voiceButton,*enterButton,*voiceBars[5];
static lv_obj_t *statusRail,*wifiLabel,*batteryLabel;
static int statusWifi=PANEL_WIFI_OFFLINE,statusBars=0,statusBattery=-1;
static bool statusComputer=false;
static lv_obj_t *deviceOverlay,*deviceCard,*deviceClose,*deviceValues[6];
static char deviceSsid[33]="",deviceIp[16]="";
static int deviceRssi=0,deviceBatteryMv=0;
static int displayedVoiceMode=VOICE_HIDDEN;
static uint32_t recordingTick=0;
static std::atomic<bool> voiceRequested{false},enterRequested{false};
static void voice_button_state(int mode);
static void refresh_device_details(void);
static bool device_details_open(){return deviceOverlay&&!lv_obj_has_flag(deviceOverlay,LV_OBJ_FLAG_HIDDEN);}
static void close_device_details(){if(deviceOverlay)lv_obj_add_flag(deviceOverlay,LV_OBJ_FLAG_HIDDEN);}

bool panel_ui_take_voice_request(void){return voiceRequested.exchange(false);}
bool panel_ui_take_enter_request(void){return enterRequested.exchange(false);}
bool panel_ui_is_locked(void){return screenLocked.load();}
void panel_ui_set_locked(bool locked){
 screenLocked.store(locked);voiceRequested.store(false);enterRequested.store(false);
 if(locked)close_device_details();
 setUpduty(locked?255:255-(lastBrightness<0?160:lastBrightness));
}
static void enter_click(lv_event_t *event){
 if(!screenLocked.load()&&!device_details_open())enterRequested.store(true);
}
static void voice_click(lv_event_t *event){
 if(screenLocked.load()||device_details_open())return;
 if(voiceRequested.exchange(true))return;
 if(displayedVoiceMode==VOICE_LISTENING){displayedVoiceMode=VOICE_RECOGNIZING;voice_button_state(VOICE_RECOGNIZING);}
}
static void voice_button_state(int mode){
 bool recording=mode==VOICE_LISTENING,busy=mode==VOICE_RECOGNIZING,error=mode==VOICE_ERROR;
 if(busy)lv_obj_add_state(voiceButton,LV_STATE_DISABLED);else lv_obj_clear_state(voiceButton,LV_STATE_DISABLED);
 lv_obj_set_style_bg_color(voiceButton,lv_color_hex(recording?0x442830:error?0x282218:0x20262e),0);
 const int heights[5]={8,16,26,16,8};
 for(int i=0;i<5;i++){
  lv_obj_set_style_bg_color(voiceBars[i],lv_color_hex(recording?0xf4a3ae:error?0xe9bd76:TEXT_COLOR),0);
  lv_obj_set_style_bg_opa(voiceBars[i],busy?LV_OPA_40:LV_OPA_COVER,0);
  lv_obj_set_height(voiceBars[i],heights[i]);lv_obj_set_y(voiceBars[i],BUTTON_SIZE/2-heights[i]/2);
 }
}
static void voice_clock(lv_timer_t *timer){
 if(displayedVoiceMode!=VOICE_LISTENING)return;
 uint32_t elapsedMs=lv_tick_elaps(recordingTick);
 // Decorative recording indicator, not a measured microphone level.
 for(int i=0;i<5;i++){
  int phase=(elapsedMs/4+i*58)%360;
  int h=8+(lv_trigo_sin(phase)+32767)*22/65534;
  lv_obj_set_height(voiceBars[i],h);lv_obj_set_y(voiceBars[i],BUTTON_SIZE/2-h/2);
 }
}
static lv_obj_t *label(lv_obj_t *parent,int x,int y,int width,const lv_font_t *font,uint32_t color){
 auto *l=lv_label_create(parent);lv_obj_set_pos(l,x,y);lv_obj_set_size(l,width,lv_font_get_line_height(font));
 lv_obj_set_style_text_font(l,font,0);lv_obj_set_style_text_color(l,lv_color_hex(color),0);
 lv_label_set_recolor(l,false);lv_label_set_long_mode(l,LV_LABEL_LONG_DOT);lv_label_set_text(l,"");return l;
}
static lv_obj_t *block(lv_obj_t *parent,int x,int y,int w,int h,uint32_t color){
 auto *b=lv_obj_create(parent);lv_obj_remove_style_all(b);lv_obj_set_pos(b,x,y);lv_obj_set_size(b,w,h);
 lv_obj_set_style_bg_color(b,lv_color_hex(color),0);lv_obj_set_style_bg_opa(b,LV_OPA_COVER,0);
 lv_obj_clear_flag(b,LV_OBJ_FLAG_CLICKABLE);return b;
}
static void set_opa(void *obj,int32_t v){lv_obj_set_style_bg_opa((lv_obj_t*)obj,v,0);}
static void animate(int i,const char *st){
 lv_anim_del(markers[i],set_opa);lv_obj_set_style_bg_opa(markers[i],LV_OPA_COVER,0);
 if(strcmp(st,"waiting")&&strcmp(st,"working"))return;
 // Only a six-pixel status dot breathes; the text and separators stay still.
 lv_anim_t a;lv_anim_init(&a);lv_anim_set_var(&a,markers[i]);lv_anim_set_exec_cb(&a,set_opa);
 lv_anim_set_values(&a,255,100);lv_anim_set_time(&a,1300);lv_anim_set_playback_time(&a,1300);
 lv_anim_set_repeat_count(&a,LV_ANIM_REPEAT_INFINITE);lv_anim_set_path_cb(&a,lv_anim_path_ease_in_out);lv_anim_start(&a);
}
static lv_obj_t *control_button(lv_obj_t *column,int y,lv_event_cb_t callback){
 auto *button=lv_btn_create(column);lv_obj_remove_style_all(button);
 lv_obj_set_pos(button,8,y);lv_obj_set_size(button,BUTTON_SIZE,BUTTON_SIZE);
 lv_obj_set_style_radius(button,16,0);lv_obj_set_style_bg_opa(button,LV_OPA_COVER,0);
 lv_obj_set_style_bg_color(button,lv_color_hex(0x20262e),0);
 lv_obj_set_style_bg_color(button,lv_color_hex(0x3b4552),LV_STATE_PRESSED);
 lv_obj_clear_flag(button,LV_OBJ_FLAG_SCROLLABLE);lv_obj_add_event_cb(button,callback,LV_EVENT_PRESSED,NULL);return button;
}
static uint32_t wifi_color(){return statusWifi==PANEL_WIFI_CONNECTED?(statusComputer?0xd9e0e9:0xe9bd76):statusWifi==PANEL_WIFI_CONNECTING?0xe9bd76:MUTED_COLOR;}
static uint32_t battery_color(){return statusBattery<0?MUTED_COLOR:statusBattery<=10?0xf4a3ae:statusBattery<=20?0xe9bd76:0xd9e0e9;}
static void refresh_device_details(){
 if(!device_details_open())return;
 lv_label_set_text(deviceValues[0],*deviceSsid?deviceSsid:"未连接");
 char value[48];
 if(statusWifi==PANEL_WIFI_CONNECTED)snprintf(value,sizeof(value),"%d dBm",deviceRssi);
 else snprintf(value,sizeof(value),"%s",statusWifi==PANEL_WIFI_CONNECTING?"连接中":"未连接");
 lv_label_set_text(deviceValues[1],value);
 lv_label_set_text(deviceValues[2],statusWifi==PANEL_WIFI_CONNECTED&&*deviceIp?deviceIp:"--");
 lv_label_set_text(deviceValues[3],statusComputer?"已连接":"未连接");
 if(statusBattery>=0&&deviceBatteryMv>0)snprintf(value,sizeof(value),"%d.%02d V",deviceBatteryMv/1000,(deviceBatteryMv%1000)/10);
 else snprintf(value,sizeof(value),"--");
 lv_label_set_text(deviceValues[4],value);
 if(statusBattery>=0)snprintf(value,sizeof(value),"%d%%  估算",statusBattery);
 else snprintf(value,sizeof(value),"--");
 lv_label_set_text(deviceValues[5],value);
 lv_obj_set_style_text_color(deviceValues[1],lv_color_hex(wifi_color()),0);
 lv_obj_set_style_text_color(deviceValues[3],lv_color_hex(statusComputer?0x91c7ac:0xe9bd76),0);
 lv_obj_set_style_text_color(deviceValues[4],lv_color_hex(battery_color()),0);
 lv_obj_set_style_text_color(deviceValues[5],lv_color_hex(battery_color()),0);
}
static void device_details_click(lv_event_t *event){
 if(screenLocked.load())return;
 if(device_details_open()){close_device_details();return;}
 voiceRequested.store(false);enterRequested.store(false);
 lv_obj_clear_flag(deviceOverlay,LV_OBJ_FLAG_HIDDEN);refresh_device_details();
}
static void device_details_close(lv_event_t *event){if(!screenLocked.load())close_device_details();}
static void create_device_details(){
 // Cover task cards and controls to consume touches; leave the status rail available to close.
 deviceOverlay=block(lv_scr_act(),0,0,CONTENT_WIDTH+CONTROL_WIDTH,172,0);
 lv_obj_add_flag(deviceOverlay,LV_OBJ_FLAG_CLICKABLE);lv_obj_clear_flag(deviceOverlay,LV_OBJ_FLAG_SCROLLABLE);
 lv_obj_add_event_cb(deviceOverlay,device_details_close,LV_EVENT_CLICKED,NULL);
 deviceCard=block(deviceOverlay,8,6,576,160,0x14191f);lv_obj_set_style_radius(deviceCard,14,0);
 lv_obj_add_flag(deviceCard,LV_OBJ_FLAG_CLICKABLE);lv_obj_clear_flag(deviceCard,LV_OBJ_FLAG_SCROLLABLE);
 auto *heading=label(deviceCard,16,11,300,&panel_font_20,TEXT_COLOR);lv_label_set_text(heading,"设备详情");
 block(deviceCard,16,44,544,1,DIVIDER_COLOR);
 deviceClose=lv_btn_create(deviceCard);lv_obj_remove_style_all(deviceClose);lv_obj_set_pos(deviceClose,516,4);lv_obj_set_size(deviceClose,48,40);
 lv_obj_set_style_radius(deviceClose,10,0);lv_obj_set_style_bg_opa(deviceClose,LV_OPA_COVER,0);lv_obj_set_style_bg_color(deviceClose,lv_color_hex(0x20262e),0);
 lv_obj_set_style_bg_color(deviceClose,lv_color_hex(0x3b4552),LV_STATE_PRESSED);lv_obj_clear_flag(deviceClose,LV_OBJ_FLAG_SCROLLABLE);
 lv_obj_add_event_cb(deviceClose,device_details_close,LV_EVENT_CLICKED,NULL);
 auto *closeText=label(deviceClose,4,8,40,&panel_font_20,TEXT_COLOR);lv_label_set_text(closeText,"关闭");
 const char *names[]={"Wi-Fi","信号","IP","电脑","电压","电量"};
 for(int i=0;i<6;i++){
  int x=i<3?16:308,y=54+(i%3)*34;
  auto *name=label(deviceCard,x,y,60,i==0||i==2?&lv_font_montserrat_16:&panel_font_20,MUTED_COLOR);lv_label_set_text(name,names[i]);
  deviceValues[i]=label(deviceCard,x+64,y,i<3?208:188,&panel_font_20,TEXT_COLOR);
 }
 lv_obj_add_flag(deviceOverlay,LV_OBJ_FLAG_HIDDEN);
}
void panel_ui_device_info(const char *ssid,const char *ip,int rssi,int batteryMv){
 if(!ssid)ssid="";if(!ip)ip="";
 if(!strcmp(deviceSsid,ssid)&&!strcmp(deviceIp,ip)&&deviceRssi==rssi&&deviceBatteryMv==batteryMv)return;
 snprintf(deviceSsid,sizeof(deviceSsid),"%s",ssid);snprintf(deviceIp,sizeof(deviceIp),"%s",ip);deviceRssi=rssi;deviceBatteryMv=batteryMv;
 refresh_device_details();
}
static void status_draw(lv_event_t *event){
 auto *ctx=lv_event_get_draw_ctx(event);lv_area_t bounds;lv_obj_get_coords(statusRail,&bounds);
 const int x=bounds.x1,y=bounds.y1;
 lv_draw_arc_dsc_t arc;lv_draw_arc_dsc_init(&arc);arc.width=3;arc.rounded=1;
 lv_point_t center={(lv_coord_t)(x+20),(lv_coord_t)(y+39)};
 const int radii[3]={4,9,14};
 for(int i=0;i<3;i++){
  arc.color=lv_color_hex(i<statusBars?wifi_color():0x3a434e);
  lv_draw_arc(ctx,&arc,&center,radii[i],225,315);
 }
 lv_draw_rect_dsc_t rect;lv_draw_rect_dsc_init(&rect);rect.bg_color=lv_color_hex(wifi_color());rect.radius=LV_RADIUS_CIRCLE;
 lv_area_t dot={(lv_coord_t)(x+18),(lv_coord_t)(y+38),(lv_coord_t)(x+22),(lv_coord_t)(y+42)};lv_draw_rect(ctx,&rect,&dot);
 if(statusWifi==PANEL_WIFI_OFFLINE){
  lv_draw_line_dsc_t slash;lv_draw_line_dsc_init(&slash);slash.width=3;slash.round_start=slash.round_end=1;slash.color=lv_color_hex(MUTED_COLOR);
  lv_point_t a={(lv_coord_t)(x+8),(lv_coord_t)(y+24)},b={(lv_coord_t)(x+31),(lv_coord_t)(y+44)};lv_draw_line(ctx,&slash,&a,&b);
 }
 uint32_t batteryColor=battery_color();lv_draw_rect_dsc_init(&rect);rect.bg_color=lv_color_hex(batteryColor);rect.radius=2;
 lv_area_t cap={(lv_coord_t)(x+16),(lv_coord_t)(y+91),(lv_coord_t)(x+24),(lv_coord_t)(y+94)};lv_draw_rect(ctx,&rect,&cap);
 rect.bg_opa=LV_OPA_TRANSP;rect.border_color=lv_color_hex(batteryColor);rect.border_width=3;rect.radius=5;
 lv_area_t body={(lv_coord_t)(x+9),(lv_coord_t)(y+95),(lv_coord_t)(x+30),(lv_coord_t)(y+128)};lv_draw_rect(ctx,&rect,&body);
 lv_draw_rect_dsc_init(&rect);rect.radius=1;
 int filled=statusBattery<0?0:(statusBattery+19)/20;
 for(int i=0;i<5;i++){
  rect.bg_color=lv_color_hex(i<filled?batteryColor:0x303841);
  lv_area_t segment={(lv_coord_t)(x+14),(lv_coord_t)(y+120-i*5),(lv_coord_t)(x+25),(lv_coord_t)(y+122-i*5)};lv_draw_rect(ctx,&rect,&segment);
 }
}
void panel_ui_status(int wifiState,int rssi,bool computerConnected,int batteryPercent){
 int bars=wifiState==PANEL_WIFI_CONNECTED?(rssi>=-60?3:rssi>=-70?2:1):0;
 int battery=batteryPercent<0?-1:batteryPercent>100?100:batteryPercent;
 if(wifiState==statusWifi&&bars==statusBars&&computerConnected==statusComputer&&battery==statusBattery)return;
 statusWifi=wifiState;statusBars=bars;statusComputer=computerConnected;statusBattery=battery;
 if(!statusRail)return;
 lv_obj_set_style_text_color(wifiLabel,lv_color_hex(wifi_color()),0);
 lv_obj_set_style_text_color(batteryLabel,lv_color_hex(battery_color()),0);
 char value[8];if(battery<0)snprintf(value,sizeof(value),"--");else snprintf(value,sizeof(value),"%d%%",battery);
 lv_label_set_text(batteryLabel,value);lv_obj_invalidate(statusRail);refresh_device_details();
}
void panel_ui_init(){
 lv_obj_set_style_bg_color(lv_scr_act(),lv_color_hex(0),0);lv_obj_clear_flag(lv_scr_act(),LV_OBJ_FLAG_SCROLLABLE);
 for(int i=0;i<PANEL_COUNT;i++){
  int x=i*CONTENT_WIDTH/PANEL_COUNT,width=(i+1)*CONTENT_WIDTH/PANEL_COUNT-x;
  panels[i]=lv_obj_create(lv_scr_act());lv_obj_remove_style_all(panels[i]);lv_obj_set_pos(panels[i],x,0);lv_obj_set_size(panels[i],width,172);
  lv_obj_clear_flag(panels[i],LV_OBJ_FLAG_SCROLLABLE);lv_obj_add_flag(panels[i],LV_OBJ_FLAG_HIDDEN);
  if(i)block(panels[i],0,18,1,136,DIVIDER_COLOR);
  brandIcons[i]=lv_img_create(panels[i]);lv_img_set_src(brandIcons[i],&panel_provider_logos[i]);lv_obj_set_pos(brandIcons[i],PAD,BRAND_Y);
  lv_obj_set_style_img_recolor(brandIcons[i],lv_color_hex(providerColors[i]),0);lv_obj_set_style_img_recolor_opa(brandIcons[i],LV_OPA_COVER,0);
  brands[i]=label(panels[i],BRAND_X,BRAND_Y,width-BRAND_X-8,&lv_font_montserrat_16,TEXT_COLOR);lv_label_set_text(brands[i],providerNames[i]);
  states[i]=label(panels[i],PAD,STATE_Y,width-2*PAD,&panel_font_28,MUTED_COLOR);
  markers[i]=block(panels[i],106,STATE_Y+15,6,6,MUTED_COLOR);lv_obj_set_style_radius(markers[i],LV_RADIUS_CIRCLE,0);
  titles[i]=label(panels[i],PAD,TITLE_Y,width-2*PAD,&panel_font_20,TEXT_COLOR);
  details[i]=label(panels[i],PAD,FOOTER_Y,width-94,&panel_font_meta_16,MUTED_COLOR);
  elapsed[i]=label(panels[i],width-70,FOOTER_Y,58,&lv_font_montserrat_16,MUTED_COLOR);lv_obj_set_style_text_align(elapsed[i],LV_TEXT_ALIGN_RIGHT,0);
 }
 banner=label(lv_scr_act(),PAD,FOOTER_Y,CONTENT_WIDTH-2*PAD,&panel_font_20,0xe9bd76);
 lv_obj_set_style_bg_color(banner,lv_color_hex(0),0);lv_obj_set_style_bg_opa(banner,LV_OPA_COVER,0);
 lv_label_set_text(banner,"请在电脑客户端配置 Wi-Fi");
 auto *column=block(lv_scr_act(),CONTENT_WIDTH,0,CONTROL_WIDTH,172,0);
 block(column,0,18,1,136,DIVIDER_COLOR);
 voiceButton=control_button(column,6,voice_click);enterButton=control_button(column,90,enter_click);
 for(int i=0;i<5;i++){voiceBars[i]=block(voiceButton,22+i*7,26,3,16,TEXT_COLOR);lv_obj_set_style_radius(voiceBars[i],2,0);}
 static const lv_point_t returnPoints[]={{53,25},{53,42},{23,42},{33,32},{23,42},{33,52}};
 auto *returnIcon=lv_line_create(enterButton);lv_obj_remove_style_all(returnIcon);lv_line_set_points(returnIcon,returnPoints,6);
 lv_obj_set_style_line_width(returnIcon,2,0);lv_obj_set_style_line_rounded(returnIcon,true,0);
 lv_obj_set_style_line_color(returnIcon,lv_color_hex(TEXT_COLOR),0);lv_obj_clear_flag(returnIcon,LV_OBJ_FLAG_CLICKABLE);
 statusRail=block(lv_scr_act(),CONTENT_WIDTH+CONTROL_WIDTH+2,6,40,160,0x14191f);lv_obj_set_style_radius(statusRail,14,0);
 lv_obj_clear_flag(statusRail,LV_OBJ_FLAG_SCROLLABLE);lv_obj_add_event_cb(statusRail,status_draw,LV_EVENT_DRAW_MAIN,NULL);
 lv_obj_add_flag(statusRail,LV_OBJ_FLAG_CLICKABLE);lv_obj_set_ext_click_area(statusRail,6);
 lv_obj_add_event_cb(statusRail,device_details_click,LV_EVENT_CLICKED,NULL);
 wifiLabel=label(statusRail,0,49,40,&lv_font_montserrat_14,MUTED_COLOR);lv_label_set_text(wifiLabel,"Wi-Fi");
 batteryLabel=label(statusRail,0,136,40,&lv_font_montserrat_14,MUTED_COLOR);lv_label_set_text(batteryLabel,"--");
 lv_obj_set_style_text_align(wifiLabel,LV_TEXT_ALIGN_CENTER,0);lv_obj_set_style_text_align(batteryLabel,LV_TEXT_ALIGN_CENTER,0);
 create_device_details();
 voice_button_state(VOICE_HIDDEN);lv_timer_create(voice_clock,50,NULL);
}
void panel_ui_voice_partial(const char *text){}
void panel_ui_voice(int mode,const char *title,const char *text){
 if(mode==VOICE_LISTENING&&displayedVoiceMode!=VOICE_LISTENING)recordingTick=lv_tick_get();
 displayedVoiceMode=mode;voice_button_state(mode);
}
void panel_ui_connection(const char *message){lv_label_set_text(banner,message);if(*message)lv_obj_clear_flag(banner,LV_OBJ_FLAG_HIDDEN);else lv_obj_add_flag(banner,LV_OBJ_FLAG_HIDDEN);}
static uint32_t color(const char *v,uint32_t fallback){if(!v||strlen(v)!=7||v[0]!='#')return fallback;return strtoul(v+1,nullptr,16);}
static uint32_t state_color(const char *st){
 if(!strcmp(st,"working"))return 0xa2c8fc;if(!strcmp(st,"waiting"))return 0xe9bd76;
 if(!strcmp(st,"done"))return 0x91c7ac;if(!strcmp(st,"interrupted"))return 0xf4a3ae;return MUTED_COLOR;
}
static void clock_text(char *out,size_t n,uint64_t from,uint64_t to){unsigned long sec=from&&to>=from?(to-from)/1000:0;snprintf(out,n,"%02lu:%02lu",sec/60,sec%60);}
static const char *task_title(JsonVariant card){
 const char *title=card["title"]|"";if(*title)return title;
 const char *id=card["id"]|"";
 // Respect a pinned task instead of substituting the first, urgency-sorted row.
 for(JsonVariant row:card["rows"].as<JsonArray>()){
  const char *rowId=row["id"]|"",*name=row["project"]|"";
  if(*id&&!strcmp(id,rowId)&&*name)return name;
 }
 const char *project=card["project"]|"";if(*project)return project;
 const char *first=card["rows"][0]["project"]|"";return *first?first:"暂无任务";
}
void panel_ui_json(const char *json){
 JsonDocument d;if(deserializeJson(d,json))return;if(d["type"]!="state")return;
 auto cfg=d["config"];bool swap=cfg["swap"]|false;int brightness=constrain(cfg["brightness"]|160,25,255);
 int indices[PANEL_COUNT],visible=0;bool hasClaude=false,hasCodex=false;
 int cardCount=d["cards"].size();if(cardCount>PANEL_COUNT)cardCount=PANEL_COUNT;
 for(int slot=0;slot<PANEL_COUNT;slot++){
  int index=slot<2&&swap?1-slot:slot;if(index>=cardCount)continue;
  const char *provider=d["cards"][index]["provider"]|"";if(!*provider)continue;
  if(d["running"][provider].is<bool>()&&!d["running"][provider].as<bool>())continue;
  indices[visible++]=index;if(!strcmp(provider,"claude"))hasClaude=true;if(!strcmp(provider,"codex"))hasCodex=true;
 }
 int pairedWidth=visible==3?CONTENT_WIDTH*2/3:CONTENT_WIDTH;
 int split=constrain(cfg["split"]|50,35,65)*pairedWidth/100;
 // Keep both brand names readable after reserving the device status rail.
 if(visible==3)split=constrain(split,128,pairedWidth-128);
 bool showTool=cfg["showTool"]|true,showElapsed=cfg["showElapsed"]|true;
 if(brightness!=lastBrightness){if(!screenLocked.load())setUpduty(255-brightness);lastBrightness=brightness;}
 panel_ui_connection("");int x=0;
 for(int i=0;i<PANEL_COUNT;i++){
  if(i>=visible){lv_obj_add_flag(panels[i],LV_OBJ_FLAG_HIDDEN);lv_anim_del(markers[i],set_opa);lastState[i][0]=0;continue;}
  lv_obj_clear_flag(panels[i],LV_OBJ_FLAG_HIDDEN);
  auto card=d["cards"][indices[i]];const char *provider=card["provider"]|"",*st=card["state"]|"idle",*tool=card["tool"]|"";
  int width=CONTENT_WIDTH/visible;
  if(visible==3||(hasClaude&&hasCodex))width=i==0?split:i==1?pairedWidth-split:CONTENT_WIDTH-pairedWidth;
  lv_obj_set_pos(panels[i],x,0);x+=width;lv_obj_set_width(panels[i],width);
  lv_obj_set_width(brands[i],width-BRAND_X-8);lv_obj_set_width(states[i],width-2*PAD);lv_obj_set_width(titles[i],width-2*PAD);
  int brand=!strcmp(provider,"claude")?0:!strcmp(provider,"deepseek")?2:1;
  const char *colorKey=brand==0?"claudeColor":brand==2?"deepseekColor":"codexColor";
  lv_color_t brandColor=lv_color_hex(color(cfg[colorKey],providerColors[brand]));
  lv_img_set_src(brandIcons[i],&panel_provider_logos[brand]);
  lv_obj_set_style_img_recolor(brandIcons[i],brandColor,0);
  lv_obj_set_style_text_color(brands[i],brandColor,0);
  lv_label_set_text(brands[i],providerNames[brand]);
  const char *text="空闲";
  if(!strcmp(st,"working"))text="执行中";else if(!strcmp(st,"waiting"))text="待确认";else if(!strcmp(st,"done"))text="已完成";
  else if(!strcmp(st,"interrupted"))text="已中断";else if(!strcmp(st,"unknown"))text="待确认";
  lv_label_set_text(states[i],text);lv_obj_set_style_text_color(states[i],lv_color_hex(state_color(st)),0);
  lv_point_t size;lv_txt_get_size(&size,text,&panel_font_28,0,0,LV_COORD_MAX,LV_TEXT_FLAG_NONE);
  lv_obj_set_x(markers[i],PAD+size.x+8);lv_obj_set_style_bg_color(markers[i],lv_color_hex(state_color(st)),0);
  if((!strcmp(st,"working")||!strcmp(st,"waiting"))&&PAD+size.x+14<=width-PAD)lv_obj_clear_flag(markers[i],LV_OBJ_FLAG_HIDDEN);
  else lv_obj_add_flag(markers[i],LV_OBJ_FLAG_HIDDEN);
  lv_label_set_text(titles[i],task_title(card));
  uint64_t start=card["startedAt"]|0ULL,end=card["endedAt"]|0ULL,updated=card["updatedAt"]|0ULL,now=d["now"]|0ULL;
  char time[24]="";const char *detail="";
  if(!strcmp(st,"working")){detail=showTool?(*tool?tool:"处理中"):"";if(showElapsed&&start)clock_text(time,sizeof(time),start,now);}
  else if(!strcmp(st,"waiting")){detail="等待授权";if(showElapsed&&updated)clock_text(time,sizeof(time),updated,now);}
  else if(!strcmp(st,"done")){detail="任务结束";if(showElapsed&&start)clock_text(time,sizeof(time),start,end);}
  else if(!strcmp(st,"unknown"))detail="状态待确认";
  else if(!strcmp(st,"interrupted"))detail="任务中断";
  lv_txt_get_size(&size,time,&lv_font_montserrat_16,0,0,LV_COORD_MAX,LV_TEXT_FLAG_NONE);
  int timeWidth=size.x<width-2*PAD?size.x:width-2*PAD;
  lv_obj_set_pos(elapsed[i],width-PAD-timeWidth,FOOTER_Y);lv_obj_set_width(elapsed[i],timeWidth>0?timeWidth:1);lv_label_set_text(elapsed[i],time);
  int detailWidth=width-2*PAD-(*time?timeWidth+8:0);lv_obj_set_width(details[i],detailWidth>0?detailWidth:1);
  // Do not turn a narrow footer into a row of ellipses. Keep the elapsed time.
  lv_label_set_text(details[i],detailWidth>=48?detail:"");
  if(strcmp(lastState[i],st)){animate(i,st);snprintf(lastState[i],sizeof(lastState[i]),"%s",st);}
 }
}
