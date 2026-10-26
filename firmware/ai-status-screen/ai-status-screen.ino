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
#define VOICE_CHUNK 1600
#define VOICE_MAX_MS 60000
Preferences prefs;WebSocketsClient socketClient;
String serialLine,serverHost,deviceToken,ssid,password;
int serverPort=8787;bool socketStarted=false,connected=false;unsigned long lastRetry=0,lastMessage=0,lastLog=0;
bool micReady=false,listening=false,pwrArmed=false,bootDown=false;int voiceMode=VOICE_HIDDEN;
unsigned long voiceAt=0,voiceHideAt=0,bootChangedAt=0,pwrDownAt=0;int16_t voiceBuf[VOICE_CHUNK];
void message(const char *text){if(example_lvgl_lock(100)){panel_ui_connection(text);example_lvgl_unlock();}}
void showVoice(int mode,const char *title,const char *text,unsigned long hideAfter){voiceMode=mode;voiceHideAt=hideAfter?millis()+hideAfter:0;if(example_lvgl_lock(100)){panel_ui_voice(mode,title,text);example_lvgl_unlock();}}
void sendVoice(const char *action){JsonDocument d;d["type"]="voice";d["action"]=action;String msg;serializeJson(d,msg);socketClient.sendTXT(msg);}
void startListening(){
 if(!micReady)return showVoice(VOICE_ERROR,"出错了","麦克风未就绪",3000);
 if(!connected)return showVoice(VOICE_ERROR,"电脑未连接","请确认电脑上的服务已打开",3000);
 if(!voice_start())return showVoice(VOICE_ERROR,"出错了","麦克风打开失败",3000);
 sendVoice("start");listening=true;voiceAt=millis();showVoice(VOICE_LISTENING,"正在听","再按一下 BOOT 结束",0);
}
void stopListening(){listening=false;voice_stop();sendVoice("stop");voiceAt=millis();showVoice(VOICE_RECOGNIZING,"识别中","",0);}
// BOOT toggles listening; PWR held for 2 s turns the board off when it runs on battery.
void pollKeys(){
 bool boot=digitalRead(BOOT_KEY)==LOW;if(boot!=bootDown&&millis()-bootChangedAt>40){bootDown=boot;bootChangedAt=millis();if(boot){if(listening)stopListening();else if(voiceMode!=VOICE_RECOGNIZING)startListening();}}
 bool pwr=digitalRead(PWR_KEY)==LOW;if(!pwr){pwrArmed=true;pwrDownAt=0;}else if(pwrArmed){if(!pwrDownAt)pwrDownAt=millis();else if(millis()-pwrDownAt>2000){Serial.println("POWER_OFF");board_power_off();pwrArmed=false;}}
}
void voiceMessage(const char *data){
 JsonDocument d;if(deserializeJson(d,data))return;const char *st=d["state"]|"";const char *text=d["text"]|"";
 if(!strcmp(st,"recognizing"))showVoice(VOICE_RECOGNIZING,"识别中","",0);
 else if(!strcmp(st,"done"))showVoice(VOICE_DONE,"已输入",text,4000);
 else if(!strcmp(st,"error"))showVoice(VOICE_ERROR,"没听清",text,3000);
}
void startWifi(){socketClient.disconnect();socketStarted=false;connected=false;WiFi.disconnect();if(ssid.length()){WiFi.mode(WIFI_STA);WiFi.begin(ssid.c_str(),password.c_str());message("正在连接 Wi-Fi");}else message("请在电脑网页中配置 Wi-Fi");lastRetry=millis();}
void wsEvent(WStype_t type,uint8_t *payload,size_t length){
 if(type==WStype_CONNECTED){connected=true;lastMessage=millis();Serial.println("WS_CONNECTED");}
 if(type==WStype_DISCONNECTED){connected=false;message("电脑服务离线 · 正在重连");}
 if(type==WStype_TEXT){String data;data.reserve(length+1);for(size_t i=0;i<length;i++)data+=(char)payload[i];if(data.startsWith("{\"type\":\"voice\"")){voiceMessage(data.c_str());return;}if(example_lvgl_lock(100)){panel_ui_json(data.c_str());example_lvgl_unlock();}lastMessage=millis();}
}
void setup(){
 Serial.begin(115200);i2c_master_Init();lvgl_port_init();lcd_bl_pwm_bsp_init(95);
 pinMode(BOOT_KEY,INPUT_PULLUP);pinMode(PWR_KEY,INPUT_PULLUP);micReady=voice_init();Serial.printf("MIC_READY %d\n",micReady);
 prefs.begin("ai-panel",false);ssid=prefs.getString("ssid","");password=prefs.getString("password","");serverHost=prefs.getString("host","");deviceToken=prefs.getString("token","");serverPort=prefs.getInt("port",8787);
 socketClient.onEvent(wsEvent);socketClient.setReconnectInterval(5000);socketClient.enableHeartbeat(30000,5000,2);startWifi();Serial.println("AI_PANEL_READY");
}
void loop(){
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
 pollKeys();
 if(listening){
  if(!connected){listening=false;voice_stop();showVoice(VOICE_ERROR,"电脑未连接","录音已取消",3000);}
  else{int n=voice_read(voiceBuf,VOICE_CHUNK);if(n>0)socketClient.sendBIN((uint8_t*)voiceBuf,n*sizeof(int16_t));
   unsigned long sec=(millis()-voiceAt)/1000;static unsigned long shown=~0UL;if(sec!=shown){shown=sec;char t[64];snprintf(t,sizeof(t),"再按一下 BOOT 结束 · %02lu:%02lu",sec/60,sec%60);showVoice(VOICE_LISTENING,"正在听",t,0);}
   if(millis()-voiceAt>VOICE_MAX_MS)stopListening();}
 }
 if(voiceMode==VOICE_RECOGNIZING&&millis()-voiceAt>20000)showVoice(VOICE_ERROR,"出错了","电脑没有响应",3000);
 if(voiceHideAt&&millis()>voiceHideAt)showVoice(VOICE_HIDDEN,"","",0);
 if(connected&&millis()-lastMessage>10000)message("数据已过期 · 等待电脑更新");
 if(millis()-lastLog>10000){Serial.printf("PANEL status: wifi=%d ws=%d psram=%u heap=%u\n",WiFi.status(),connected,ESP.getPsramSize(),ESP.getFreeHeap());Serial.printf("NETWORK ip=%s gateway=%s mask=%s server=%s:%d rssi=%d\n",WiFi.localIP().toString().c_str(),WiFi.gatewayIP().toString().c_str(),WiFi.subnetMask().toString().c_str(),serverHost.c_str(),serverPort,WiFi.RSSI());lastLog=millis();}delay(5);
}
