#include <WiFi.h>
#include <Preferences.h>
#include <WebSocketsClient.h>
#include <ArduinoJson.h>
#include "lvgl_port.h"
#include "i2c_bsp.h"
#include "panel_ui.h"
#include "voice_bsp.h"
#include "src/lcd_bl_bsp/lcd_bl_pwm_bsp.h"
#define BOOT_KEY 0
#define PWR_KEY 16
#define VOICE_CHUNK 320
#define VOICE_MAX_MS 60000
Preferences prefs;WebSocketsClient socketClient;
String serialLine,serverHost,deviceToken,ssid,password;
int serverPort=8787;bool socketStarted=false,connected=false;unsigned long lastRetry=0,lastMessage=0,lastLog=0;
bool micReady=false,listening=false,pwrArmed=false,bootDown=false;int voiceMode=VOICE_HIDDEN;
unsigned long loopPrevious=0,maxLoopMs=0,maxReadMs=0,maxSendMs=0,performanceAt=0;uint32_t readErrors=0,sendErrors=0;
unsigned long voiceAt=0,voiceHideAt=0,bootChangedAt=0,pwrDownAt=0;int16_t voiceBuf[VOICE_CHUNK];
void message(const char *text){if(example_lvgl_lock(100)){panel_ui_connection(text);example_lvgl_unlock();}}
void showVoice(int mode,const char *title,const char *text,unsigned long hideAfter){voiceMode=mode;voiceHideAt=hideAfter?millis()+hideAfter:0;if(example_lvgl_lock(100)){panel_ui_voice(mode,title,text);example_lvgl_unlock();}}
void sendVoice(const char *action){JsonDocument d;d["type"]="voice";d["action"]=action;String msg;serializeJson(d,msg);socketClient.sendTXT(msg);}
void startListening(){
 if(panel_ui_is_locked())return;
 if(!micReady)return showVoice(VOICE_ERROR,"出错了","麦克风未就绪",3000);
 if(!connected)return showVoice(VOICE_ERROR,"电脑未连接","请确认电脑上的服务已打开",3000);
 if(!voice_start())return showVoice(VOICE_ERROR,"出错了","麦克风打开失败",3000);
 sendVoice("start");listening=true;voiceAt=millis();showVoice(VOICE_LISTENING,"","",0);
}
void stopListening(){listening=false;voiceAt=millis();showVoice(VOICE_RECOGNIZING,"","",0);voice_stop();sendVoice("stop");}
void logVoiceButton(const char *source){JsonDocument d;d["type"]="voice-debug";d["source"]=source;d["deviceMs"]=millis();String msg;serializeJson(d,msg);if(connected)socketClient.sendTXT(msg);}
void toggleListening(){if(listening)stopListening();else if(voiceMode!=VOICE_RECOGNIZING)startListening();}
void toggleScreenLock(){
 bool locked=!panel_ui_is_locked();
 if(locked&&listening)stopListening();
 if(example_lvgl_lock(100)){panel_ui_set_locked(locked);example_lvgl_unlock();}
}
// BOOT locks/wakes this screen; PWR held for 2 s turns it off on battery.
void pollKeys(){
 bool boot=digitalRead(BOOT_KEY)==LOW;if(boot!=bootDown&&millis()-bootChangedAt>40){bootDown=boot;bootChangedAt=millis();if(boot)toggleScreenLock();}
 bool pwr=digitalRead(PWR_KEY)==LOW;if(!pwr){pwrArmed=true;pwrDownAt=0;}else if(pwrArmed){if(!pwrDownAt)pwrDownAt=millis();else if(millis()-pwrDownAt>2000){Serial.println("POWER_OFF");board_power_off();pwrArmed=false;}}
}
void voiceMessage(const char *data){
 JsonDocument d;if(deserializeJson(d,data))return;const char *st=d["state"]|"";const char *text=d["text"]|"";
 if(!strcmp(st,"partial")){if(listening&&example_lvgl_lock(100)){panel_ui_voice_partial(text);example_lvgl_unlock();}}
 else if(!strcmp(st,"recognizing"))showVoice(VOICE_RECOGNIZING,"","",0);
 else if(!strcmp(st,"finished")){if(listening){listening=false;voice_stop();}showVoice(VOICE_HIDDEN,"","",0);}
 else if(!strcmp(st,"done"))showVoice(VOICE_HIDDEN,"","",0);
 else if(!strcmp(st,"error")){if(listening){listening=false;voice_stop();}showVoice(VOICE_ERROR,"出错了",text,8000);}
}
void startWifi(){socketClient.disconnect();socketStarted=false;connected=false;WiFi.disconnect();if(ssid.length()){WiFi.mode(WIFI_STA);WiFi.setSleep(false);WiFi.begin(ssid.c_str(),password.c_str());message("正在连接 Wi-Fi");}else message("请在电脑网页中配置 Wi-Fi");lastRetry=millis();}
void wsEvent(WStype_t type,uint8_t *payload,size_t length){
 if(type==WStype_CONNECTED){connected=true;lastMessage=millis();Serial.println("WS_CONNECTED");}
 if(type==WStype_DISCONNECTED){connected=false;message("电脑服务离线 · 正在重连");}
 if(type==WStype_TEXT){String data;data.reserve(length+1);for(size_t i=0;i<length;i++)data+=(char)payload[i];if(data.startsWith("{\"type\":\"voice\"")){voiceMessage(data.c_str());return;}if(example_lvgl_lock(100)){panel_ui_json(data.c_str());example_lvgl_unlock();}lastMessage=millis();}
}
void setup(){
 Serial.begin(115200);Serial.setTxTimeoutMs(0);i2c_master_Init();lvgl_port_init();lcd_bl_pwm_bsp_init(95);
 pinMode(BOOT_KEY,INPUT_PULLUP);pinMode(PWR_KEY,INPUT_PULLUP);micReady=voice_init();Serial.printf("MIC_READY %d\n",micReady);
 prefs.begin("ai-panel",false);ssid=prefs.getString("ssid","");password=prefs.getString("password","");serverHost=prefs.getString("host","");deviceToken=prefs.getString("token","");serverPort=prefs.getInt("port",8787);
 socketClient.onEvent(wsEvent);socketClient.setReconnectInterval(5000);socketClient.enableHeartbeat(30000,5000,2);startWifi();Serial.println("AI_PANEL_READY");
}
void loop(){
 unsigned long loopNow=millis();if(loopPrevious)maxLoopMs=max(maxLoopMs,loopNow-loopPrevious);loopPrevious=loopNow;
 pollKeys();
 bool touchRequested=panel_ui_take_voice_request();
 if(touchRequested&&!panel_ui_is_locked()){Serial.println("VOICE_TOUCH");logVoiceButton("touch");toggleListening();}
 while(Serial.available()){
  char c=Serial.read();if(c=='\n'){
   JsonDocument doc;if(!deserializeJson(doc,serialLine)&&doc["type"]=="configure"){
    String newSsid=doc["ssid"]|"",newHost=doc["host"]|"",newToken=doc["token"]|"",newPassword=doc["password"]|"";int newPort=doc["port"]|8787;
    if(newSsid.length()>0&&newSsid.length()<=32&&newPassword.length()<=63&&newHost.length()>0&&newHost.length()<100&&newToken.length()>=32&&newToken.length()<100&&newPort>0&&newPort<=65535){
     ssid=newSsid;password=newPassword;serverHost=newHost;deviceToken=newToken;serverPort=newPort;
     prefs.putString("ssid",ssid);prefs.putString("password",password);prefs.putString("host",serverHost);prefs.putString("token",deviceToken);prefs.putInt("port",serverPort);Serial.println("CONFIG_SAVED");startWifi();
    }else Serial.println("CONFIG_INVALID");
   }serialLine="";
  }else if(serialLine.length()<2048)serialLine+=c;else serialLine="";
 }
 if(WiFi.status()==WL_CONNECTED){
  if(!socketStarted){String endpoint="/device?token="+deviceToken;socketClient.begin(serverHost.c_str(),serverPort,endpoint.c_str());socketStarted=true;message("Wi-Fi 已连接 · 正在连接电脑");Serial.println("WIFI_CONNECTED");}
  socketClient.loop();
 }else if(ssid.length()&&millis()-lastRetry>15000){startWifi();}
 if(listening){
  if(!connected){listening=false;voice_stop();showVoice(VOICE_ERROR,"电脑未连接","录音已取消",3000);}
  else{unsigned long readAt=millis();int n=voice_read(voiceBuf,VOICE_CHUNK);maxReadMs=max(maxReadMs,millis()-readAt);
   if(n>0){unsigned long sendAt=millis();if(!socketClient.sendBIN((uint8_t*)voiceBuf,n*sizeof(int16_t)))sendErrors++;maxSendMs=max(maxSendMs,millis()-sendAt);}else readErrors++;
   if(millis()-voiceAt>VOICE_MAX_MS)stopListening();}
 }
 if(voiceMode==VOICE_RECOGNIZING&&millis()-voiceAt>20000)showVoice(VOICE_ERROR,"出错了","电脑没有响应",3000);
 if(voiceHideAt&&millis()>voiceHideAt)showVoice(VOICE_HIDDEN,"","",0);
 if(connected&&millis()-lastMessage>10000)message("数据已过期 · 等待电脑更新");
 if(connected&&millis()-performanceAt>=2000){
  JsonDocument d;d["type"]="voice-performance";d["loopMs"]=maxLoopMs;d["readMs"]=maxReadMs;d["sendMs"]=maxSendMs;d["readErrors"]=readErrors;d["sendErrors"]=sendErrors;d["rssi"]=WiFi.RSSI();d["recording"]=listening;d["screenLocked"]=panel_ui_is_locked();
  String msg;serializeJson(d,msg);socketClient.sendTXT(msg);performanceAt=millis();maxLoopMs=maxReadMs=maxSendMs=0;readErrors=sendErrors=0;
 }
 delay(1);

}
