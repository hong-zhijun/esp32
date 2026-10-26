#include "panel_ui.h"
#include "lvgl.h"
#include <ArduinoJson.h>
#include "src/lcd_bl_bsp/lcd_bl_pwm_bsp.h"
#include <stdio.h>
#include <string.h>
#include <atomic>
LV_FONT_DECLARE(panel_font_16);
LV_FONT_DECLARE(panel_font_24);
LV_FONT_DECLARE(panel_font_30);
static lv_obj_t *panels[2],*strips[2],*brands[2],*states[2],*details[2],*lists[2][3],*runners[2],*banner;
static char lastState[2][16];static int lastWidth[2]={-1,-1};
static int lastBrightness=-1;
static std::atomic<bool> screenLocked{false};
static constexpr int CONTENT_WIDTH=512;
static lv_obj_t *voiceButton,*voiceButtonLabel,*voiceButtonHint,*voiceTimer,*voiceBars[5];
static int displayedVoiceMode=VOICE_HIDDEN;
static uint32_t recordingTick=0,lastRecordingSecond=UINT32_MAX;
static std::atomic<bool> voiceRequested{false};
static void voice_button_state(int mode);
bool panel_ui_take_voice_request(void){return voiceRequested.exchange(false);}
bool panel_ui_is_locked(void){return screenLocked.load();}
void panel_ui_set_locked(bool locked){
 screenLocked.store(locked);voiceRequested.store(false);
 setUpduty(locked?255:255-(lastBrightness<0?160:lastBrightness));
}
static void voice_click(lv_event_t *event){
 if(screenLocked.load())return;
 if(voiceRequested.exchange(true))return;
 // Close the recording animation on touch-down, before network/audio cleanup.
 if(displayedVoiceMode==VOICE_LISTENING){displayedVoiceMode=VOICE_RECOGNIZING;voice_button_state(VOICE_RECOGNIZING);}
}
static void voice_button_state(int mode){
 bool recording=mode==VOICE_LISTENING,busy=mode==VOICE_RECOGNIZING,error=mode==VOICE_ERROR;
 // Finish sending the tail in the background before accepting a new session.
 if(busy)lv_obj_add_state(voiceButton,LV_STATE_DISABLED);else lv_obj_clear_state(voiceButton,LV_STATE_DISABLED);
 lv_obj_set_style_bg_color(voiceButton,lv_color_hex(recording?0x302124:error?0x382c1b:0x142b28),0);
 lv_obj_set_style_border_color(voiceButton,lv_color_hex(recording?0x75444c:error?0x80613b:0x2b5047),0);
 lv_label_set_text(voiceButtonLabel,recording?"识别中···":error?"出错了":"开始录音");
 lv_label_set_text(voiceButtonHint,recording?"点击结束":error?"查看电脑":"点击说话");
 lv_obj_set_style_text_color(voiceButtonHint,lv_color_hex(recording?0xd6a4ab:0x8caf9f),0);
 if(recording){lv_obj_clear_flag(voiceTimer,LV_OBJ_FLAG_HIDDEN);lv_label_set_text(voiceTimer,"00:00");}
 else lv_obj_add_flag(voiceTimer,LV_OBJ_FLAG_HIDDEN);
 const int heights[5]={8,16,24,16,8};
 for(int i=0;i<5;i++){
  lv_obj_set_style_bg_color(voiceBars[i],lv_color_hex(recording?0xff9da8:error?0xffc36a:0x73c9ad),0);
  lv_obj_set_height(voiceBars[i],heights[i]);lv_obj_set_y(voiceBars[i],34-heights[i]/2);
 }
}
static void voice_clock(lv_timer_t *timer){
 if(displayedVoiceMode!=VOICE_LISTENING)return;
 uint32_t elapsed=lv_tick_elaps(recordingTick),sec=elapsed/1000;
 if(sec!=lastRecordingSecond){char t[16];snprintf(t,sizeof(t),"%02lu:%02lu",(unsigned long)(sec/60),(unsigned long)(sec%60));lv_label_set_text(voiceTimer,t);lastRecordingSecond=sec;}
 // Decorative wave, not a microphone level meter. Only five tiny bars redraw.
 for(int i=0;i<5;i++){
  int phase=(elapsed/4+i*58)%360;
  int h=8+(lv_trigo_sin(phase)+32767)*22/65534;
  lv_obj_set_height(voiceBars[i],h);lv_obj_set_y(voiceBars[i],34-h/2);
 }
}
static lv_obj_t *label(lv_obj_t *parent,int x,int y,int width,const lv_font_t *font,uint32_t color){
 auto *l=lv_label_create(parent);lv_obj_set_pos(l,x,y);lv_obj_set_width(l,width);lv_obj_set_style_text_font(l,font,0);lv_obj_set_style_text_color(l,lv_color_hex(color),0);lv_label_set_long_mode(l,LV_LABEL_LONG_DOT);lv_label_set_text(l,"");return l;
}
static lv_obj_t *block(lv_obj_t *parent,int x,int y,int w,int h,uint32_t color){
 auto *b=lv_obj_create(parent);lv_obj_remove_style_all(b);lv_obj_set_pos(b,x,y);lv_obj_set_size(b,w,h);lv_obj_set_style_bg_color(b,lv_color_hex(color),0);lv_obj_set_style_bg_opa(b,LV_OPA_COVER,0);lv_obj_clear_flag(b,LV_OBJ_FLAG_CLICKABLE);return b;
}
static void set_opa(void *obj,int32_t v){lv_obj_set_style_bg_opa((lv_obj_t*)obj,v,0);}
static void set_x(void *obj,int32_t v){lv_obj_set_x((lv_obj_t*)obj,v);}
// Waiting: the left strip breathes slowly. Working: a thin line runs along the bottom.
static void animate(int i,const char *st,int width){
 lv_anim_del(strips[i],set_opa);lv_obj_set_style_bg_opa(strips[i],LV_OPA_COVER,0);
 lv_anim_del(runners[i],set_x);lv_obj_add_flag(runners[i],LV_OBJ_FLAG_HIDDEN);
 lv_anim_t a;lv_anim_init(&a);
 if(!strcmp(st,"waiting")){lv_anim_set_var(&a,strips[i]);lv_anim_set_exec_cb(&a,set_opa);lv_anim_set_values(&a,255,115);lv_anim_set_time(&a,1300);lv_anim_set_playback_time(&a,1300);lv_anim_set_repeat_count(&a,LV_ANIM_REPEAT_INFINITE);lv_anim_set_path_cb(&a,lv_anim_path_ease_in_out);lv_anim_start(&a);}
 if(!strcmp(st,"working")){int w=width*3/10;lv_obj_set_width(runners[i],w);lv_obj_clear_flag(runners[i],LV_OBJ_FLAG_HIDDEN);lv_anim_set_var(&a,runners[i]);lv_anim_set_exec_cb(&a,set_x);lv_anim_set_values(&a,-w,width);lv_anim_set_time(&a,2400);lv_anim_set_repeat_count(&a,LV_ANIM_REPEAT_INFINITE);lv_anim_start(&a);}
}
void panel_ui_init(){
 lv_obj_set_style_bg_color(lv_scr_act(),lv_color_hex(0),0);lv_obj_clear_flag(lv_scr_act(),LV_OBJ_FLAG_SCROLLABLE);
 for(int i=0;i<2;i++){
  panels[i]=lv_obj_create(lv_scr_act());lv_obj_remove_style_all(panels[i]);lv_obj_set_pos(panels[i],i*(CONTENT_WIDTH/2),0);lv_obj_set_size(panels[i],CONTENT_WIDTH/2,172);lv_obj_clear_flag(panels[i],LV_OBJ_FLAG_SCROLLABLE);
  if(i){lv_obj_set_style_border_side(panels[i],LV_BORDER_SIDE_LEFT,0);lv_obj_set_style_border_width(panels[i],1,0);lv_obj_set_style_border_color(panels[i],lv_color_hex(0x1c1c1c),0);}
  strips[i]=block(panels[i],0,0,6,172,0x333333);
  brands[i]=label(panels[i],22,4,CONTENT_WIDTH/2-40,&panel_font_24,i?0x63d9ca:0xdf9877);lv_label_set_text(brands[i],i?"Codex":"Claude");
  states[i]=label(panels[i],22,34,CONTENT_WIDTH/2-40,&panel_font_30,0x8b9199);lv_label_set_text(states[i],"空闲");
  details[i]=label(panels[i],22,70,CONTENT_WIDTH/2-40,&panel_font_16,0x8b9199);
  for(int r=0;r<3;r++){lists[i][r]=label(panels[i],22,94+r*20,CONTENT_WIDTH/2-40,&panel_font_16,0x8b9199);lv_label_set_recolor(lists[i][r],true);}
  runners[i]=block(panels[i],0,170,96,2,0x6ab2ff);lv_obj_set_style_bg_opa(runners[i],LV_OPA_70,0);lv_obj_add_flag(runners[i],LV_OBJ_FLAG_HIDDEN);
 }
 banner=label(lv_scr_act(),22,152,CONTENT_WIDTH-40,&panel_font_16,0xffc36a);lv_obj_set_style_bg_color(banner,lv_color_hex(0),0);lv_obj_set_style_bg_opa(banner,LV_OPA_COVER,0);lv_label_set_text(banner,"请在电脑网页中配置 Wi-Fi");
 auto *column=block(lv_scr_act(),CONTENT_WIDTH,0,128,172,0x101817);
 voiceButton=lv_btn_create(column);lv_obj_remove_style_all(voiceButton);
 lv_obj_set_pos(voiceButton,8,10);lv_obj_set_size(voiceButton,112,152);
 lv_obj_set_style_border_width(voiceButton,1,0);lv_obj_set_style_radius(voiceButton,16,0);lv_obj_set_style_bg_opa(voiceButton,LV_OPA_COVER,0);
 lv_obj_set_style_bg_color(voiceButton,lv_color_hex(0x355b52),LV_STATE_PRESSED);
 lv_obj_clear_flag(voiceButton,LV_OBJ_FLAG_SCROLLABLE);
 lv_obj_add_event_cb(voiceButton,voice_click,LV_EVENT_PRESSED,NULL);
 for(int i=0;i<5;i++){voiceBars[i]=block(voiceButton,34+i*10,26,5,16,0x73c9ad);lv_obj_set_style_radius(voiceBars[i],3,0);}
 voiceButtonLabel=label(voiceButton,0,60,110,&panel_font_16,0xffffff);
 voiceTimer=label(voiceButton,0,85,110,&panel_font_24,0xffb8c0);
 lv_obj_set_style_text_align(voiceTimer,LV_TEXT_ALIGN_CENTER,0);
 voiceButtonHint=label(voiceButton,0,120,110,&panel_font_16,0x8caf9f);
 lv_obj_set_style_text_align(voiceButtonLabel,LV_TEXT_ALIGN_CENTER,0);
 lv_obj_set_style_text_align(voiceButtonHint,LV_TEXT_ALIGN_CENTER,0);
 voice_button_state(VOICE_HIDDEN);
 lv_timer_create(voice_clock,50,NULL);
}
// Recognition text belongs in the focused computer input, never on this screen.
void panel_ui_voice_partial(const char *text){}
void panel_ui_voice(int mode,const char *title,const char *text){
 if(mode==VOICE_LISTENING&&displayedVoiceMode!=VOICE_LISTENING){recordingTick=lv_tick_get();lastRecordingSecond=UINT32_MAX;}
 displayedVoiceMode=mode;
 voice_button_state(mode);
}
void panel_ui_connection(const char *message){lv_label_set_text(banner,message);if(*message)lv_obj_clear_flag(banner,LV_OBJ_FLAG_HIDDEN);else lv_obj_add_flag(banner,LV_OBJ_FLAG_HIDDEN);}
static uint32_t color(const char *v,uint32_t fallback){if(!v||strlen(v)!=7||v[0]!='#')return fallback;return strtoul(v+1,nullptr,16);}
static uint32_t state_color(const char *st){if(!strcmp(st,"working"))return 0x6ab2ff;if(!strcmp(st,"waiting"))return 0xffc36a;if(!strcmp(st,"done"))return 0x6ce0ac;if(!strcmp(st,"interrupted"))return 0xe29494;return 0x555555;}
static void clock_text(char *out,size_t n,uint64_t from,uint64_t to){unsigned long sec=from&&to>=from?(to-from)/1000:0;snprintf(out,n,"%02lu:%02lu",sec/60,sec%60);}
void panel_ui_json(const char *json){
 JsonDocument d;if(deserializeJson(d,json))return;if(d["type"]!="state")return;
 auto cfg=d["config"];int split=constrain(cfg["split"]|50,35,65)*CONTENT_WIDTH/100;bool swap=cfg["swap"]|false;int brightness=constrain(cfg["brightness"]|160,25,255);
 bool showTool=cfg["showTool"]|true,showElapsed=cfg["showElapsed"]|true;
 if(brightness!=lastBrightness){if(!screenLocked.load())setUpduty(255-brightness);lastBrightness=brightness;}
 panel_ui_connection("");
 for(int i=0;i<2;i++){
  auto card=d["cards"][swap?1-i:i];const char *provider=card["provider"]|"";const char *st=card["state"]|"idle";const char *tool=card["tool"]|"";
  int width=i?CONTENT_WIDTH-split:split;lv_obj_set_pos(panels[i],i?split:0,0);lv_obj_set_width(panels[i],width);for(auto *l:{brands[i],states[i],details[i],lists[i][0],lists[i][1],lists[i][2]})lv_obj_set_width(l,width-40);
  bool claude=!strcmp(provider,"claude");lv_label_set_text(brands[i],claude?"Claude":"Codex");lv_obj_set_style_text_color(brands[i],lv_color_hex(color(cfg[claude?"claudeColor":"codexColor"],claude?0xdf9877:0x63d9ca)),0);
  const char *text="空闲";uint32_t c=0x8b9199,strip=0x333333;
  if(!strcmp(st,"working")){text="执行中";c=strip=0x6ab2ff;}else if(!strcmp(st,"waiting")){text="等你确认";c=strip=0xffc36a;}else if(!strcmp(st,"done")){text="做完了";c=strip=0x6ce0ac;}else if(!strcmp(st,"interrupted")){text="已中断";c=strip=0xe29494;}else if(!strcmp(st,"unknown")){text="状态待确认";}
  lv_label_set_text(states[i],text);lv_obj_set_style_text_color(states[i],lv_color_hex(c),0);lv_obj_set_style_bg_color(strips[i],lv_color_hex(strip),0);
  uint64_t start=card["startedAt"]|0ULL,end=card["endedAt"]|0ULL,updated=card["updatedAt"]|0ULL,now=d["now"]|0ULL;char t[16],detail[120]="";
  if(!strcmp(st,"working")){clock_text(t,sizeof(t),start,now);const char *shown=showTool?tool:"";if(*shown&&showElapsed)snprintf(detail,sizeof(detail),"%s · %s",shown,t);else snprintf(detail,sizeof(detail),"%s",*shown?shown:showElapsed?t:"");}
  else if(!strcmp(st,"waiting")){clock_text(t,sizeof(t),updated,now);snprintf(detail,sizeof(detail),"已等 %s",t);}
  else if(!strcmp(st,"done")&&showElapsed&&start){clock_text(t,sizeof(t),start,end);snprintf(detail,sizeof(detail),"用时 %s",t);}
  else if(!strcmp(st,"unknown"))snprintf(detail,sizeof(detail),"5 分钟没有新动静");
  lv_label_set_text(details[i],detail);
  // Up to three sessions: dot colour shows state, name is bright while it is still running.
  for(int r=0;r<3;r++){auto row=card["rows"][r];const char *rs=row["state"]|"";char name[64],line[120]="";snprintf(name,sizeof(name),"%s",row["project"]|"");for(char *p=name;*p;p++)if(*p=='#')*p=' ';
   if(*rs){bool active=!strcmp(rs,"working")||!strcmp(rs,"waiting");snprintf(line,sizeof(line),"#%06lx ●# #%06lx %s#",(unsigned long)state_color(rs),(unsigned long)(active?0xe4e4e4:0x8b9199),*name?name:"-");}
   lv_label_set_text(lists[i][r],line);}
  if(strcmp(lastState[i],st)||lastWidth[i]!=width){animate(i,st,width);snprintf(lastState[i],sizeof(lastState[i]),"%s",st);lastWidth[i]=width;}
 }
}
