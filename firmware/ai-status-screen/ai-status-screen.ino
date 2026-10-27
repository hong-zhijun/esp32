#include <WiFi.h>
#include <ESPmDNS.h>
#include <WebSocketsClient.h>
#include <ArduinoJson.h>
#include "lvgl_port.h"
#include "i2c_bsp.h"
#include "panel_ui.h"
#include "voice_bsp.h"
#include "wifi_profiles.h"
#include "src/lcd_bl_bsp/lcd_bl_pwm_bsp.h"
#define BOOT_KEY 0
#define PWR_KEY 16
#define VOICE_CHUNK 320
#define VOICE_MAX_MS 60000
#define WIFI_TRY_MS 15000      // one join attempt before the next saved network
#define WIFI_RETRY_MS 15000    // pause after every saved network failed, then scan again
#define SOCKET_LOST_MS 20000   // WebSocket not connected this long: look for the computer again
WebSocketsClient socketClient;
String serialLine;
WifiProfile active;            // Wi-Fi + computer in use; a copy, so forget or rediscovery cannot pull it away
enum LinkPhase{LINK_IDLE,LINK_SCAN,LINK_JOIN,LINK_WAIT,LINK_UP};
LinkPhase linkPhase=LINK_IDLE;
String netList[WIFI_PROFILE_MAX];int netRssi[WIFI_PROFILE_MAX],netCount=0,netTry=0;
bool connected=false,mdnsTried=false;
unsigned long netAt=0,attemptAt=0,lastMessage=0,lastLog=0;
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
void sendEnter(){
 if(panel_ui_is_locked()||!connected)return;
 JsonDocument d;d["type"]="key";d["key"]="Enter";
 String msg;serializeJson(d,msg);socketClient.sendTXT(msg);
}
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
// Wi-Fi and computer discovery. wifiStep() and socketStep() run every loop pass and never block;
// the mDNS query inside socketStart() is the one exception (about 3 s, never while recording).
void socketStop(){socketClient.disconnect();connected=false;attemptAt=0;}
// Start over: drop the WebSocket, then scan and join the strongest saved network. With none saved, wait for setup.
void startWifi(){socketStop();if(!profileCount()){WiFi.disconnect();linkPhase=LINK_IDLE;return message("请在电脑上配网");}startScan();}
// Async scan: loop() keeps running meanwhile; wifiStep() collects the result.
void startScan(){WiFi.disconnect();WiFi.mode(WIFI_STA);WiFi.setSleep(false);WiFi.scanNetworks(true);linkPhase=LINK_SCAN;netAt=millis();message("正在连接 Wi-Fi");}
// Saved networks the scan can see, strongest first (a network can appear several times; its best signal counts).
// Saved networks the scan missed follow in saved order: hidden SSIDs never show up in a scan, and a scan can fail.
void pickCandidates(int found){
 netCount=0;
 for(int i=0;i<profileCount();i++){
  String s=profileAt(i).ssid;int best=-1000;
  for(int n=0;n<found;n++)if(WiFi.SSID(n)==s&&WiFi.RSSI(n)>best)best=WiFi.RSSI(n);
  int at=netCount++;for(;at>0&&netRssi[at-1]<best;at--){netList[at]=netList[at-1];netRssi[at]=netRssi[at-1];}
  netList[at]=s;netRssi[at]=best;
 }
}
// Joins the next candidate that is still saved. When none is left, pause and scan again.
void nextJoin(){
 for(;netTry<netCount;netTry++){
  int i=profileFind(netList[netTry]);if(i<0)continue;
  active=profileAt(i);WiFi.disconnect();WiFi.begin(active.ssid.c_str(),active.pass.c_str());
  linkPhase=LINK_JOIN;netAt=millis();return message(("正在连接 "+active.ssid).c_str());
 }
 linkPhase=LINK_WAIT;netAt=millis();message("正在连接 Wi-Fi");
}
// Non-blocking Wi-Fi state machine: scan, join each visible saved network for 15 s, pause 15 s, scan again.
void wifiStep(){
 if(WiFi.status()==WL_CONNECTED){if(linkPhase!=LINK_UP){linkPhase=LINK_UP;attemptAt=0;Serial.println("WIFI_CONNECTED");}return;}
 if(linkPhase==LINK_UP)return startWifi();                              // link dropped
 if(!profileCount()){if(linkPhase!=LINK_IDLE)startWifi();return;}      // every network was forgotten meanwhile
 if(linkPhase==LINK_SCAN){int found=WiFi.scanComplete();if(found==WIFI_SCAN_RUNNING)return;pickCandidates(found>0?found:0);WiFi.scanDelete();netTry=0;nextJoin();}
 else if(linkPhase==LINK_JOIN&&millis()-netAt>=WIFI_TRY_MS){netTry++;nextJoin();}
 else if(linkPhase==LINK_WAIT&&millis()-netAt>=WIFI_RETRY_MS)startScan();
}
// The computer advertises _aipanel._tcp with TXT id=<computerId>. Returns its IPv4 for this id, or "".
String findComputer(const String &id){
 if(!mdnsTried){uint8_t mac[6];char name[24];WiFi.macAddress(mac);snprintf(name,sizeof(name),"ai-panel-%02x%02x",mac[4],mac[5]);mdnsTried=true;MDNS.begin(name);}   // mdns_init() fails if repeated
 int n=MDNS.queryService("aipanel","tcp");
 for(int i=0;i<n;i++){IPAddress ip=MDNS.address(i);if(MDNS.txt(i,"id")==id&&ip!=IPAddress())return ip.toString();}
 return "";
}
// Opens the WebSocket to the computer found by id, else the saved host. Profiles saved without an id always use the saved host.
void socketStart(){
 socketStop();attemptAt=millis();
 String host=active.host;
 if(active.id.length()){
  message("正在寻找电脑");
  String found=findComputer(active.id);
  if(found.length()&&found!=host){host=found;active.host=found;profileSetHost(active.ssid,found);}
 }
 if(!host.length())return message("未找到电脑 · 重试中");
 String endpoint="/device?token="+active.token;
 socketClient.begin(host.c_str(),active.port,endpoint.c_str());
 message("Wi-Fi 已连接 · 正在连接电脑");
}
// One connection attempt at a time. No WebSocket for 20 s: start again, which looks the computer up again.
void socketStep(){
 if(listening||connected)return;
 if(attemptAt&&millis()-attemptAt<SOCKET_LOST_MS)return;
 socketStart();
}
void addSsids(JsonArray a){for(int i=0;i<profileCount();i++)a.add(profileAt(i).ssid);}
// First message on every connection: this panel's current Wi-Fi and the networks it remembers.
void sendHello(){
 JsonDocument d;d["type"]="hello";d["fw"]=13;d["ip"]=WiFi.localIP().toString();d["ssid"]=active.ssid;addSsids(d["ssids"].to<JsonArray>());
 String msg;serializeJson(d,msg);socketClient.sendTXT(msg);
}
// The computer removed a saved network. A live connection on it stays up until it drops by itself.
void forgetWifi(const char *json){JsonDocument d;if(deserializeJson(d,json))return;profileRemove(d["ssid"]|"");sendHello();}
// USB: configure saves one Wi-Fi + computer pair, then restarts the connection with the updated list.
void configureWifi(JsonDocument &doc){
 WifiProfile p;p.ssid=doc["ssid"]|"";p.pass=doc["password"]|"";p.host=doc["host"]|"";p.token=doc["token"]|"";p.id=doc["id"]|"";p.port=doc["port"]|8787;
 if(!profileValid(p)){Serial.println("CONFIG_INVALID");return;}
 profileUpsert(p);Serial.println("CONFIG_SAVED");startWifi();
}
// USB line protocol: one JSON command per line, each answered with one line.
void serialCommand(){
 JsonDocument doc;if(deserializeJson(doc,serialLine))return;
 if(doc["type"]=="configure")configureWifi(doc);
 else if(doc["type"]=="profiles"){JsonDocument d;addSsids(d.to<JsonArray>());String out;serializeJson(d,out);Serial.println("PROFILES "+out);}
 else if(doc["type"]=="forget"){profileRemove(doc["ssid"]|"");Serial.println("FORGOT");if(connected)sendHello();}
 else if(doc["type"]=="status"){   // diagnostics: link phase, Wi-Fi driver status, current network and computer
  JsonDocument d;d["phase"]=(int)linkPhase;d["wifi"]=(int)WiFi.status();d["ssid"]=active.ssid;d["ip"]=WiFi.localIP().toString();d["rssi"]=WiFi.RSSI();
  d["host"]=active.host;d["port"]=active.port;d["hasId"]=active.id.length()>0;d["ws"]=connected;d["candidates"]=netCount;d["try"]=netTry;
  String out;serializeJson(d,out);Serial.println("STATUS "+out);
 }
}
void wsEvent(WStype_t type,uint8_t *payload,size_t length){
 if(type==WStype_CONNECTED){connected=true;lastMessage=millis();Serial.println("WS_CONNECTED");sendHello();}
 // Only a connection that was up starts the 20 s clock; failed retries must not keep restarting it.
 if(type==WStype_DISCONNECTED){if(connected)attemptAt=millis();connected=false;message("电脑服务离线 · 正在重连");}
 if(type==WStype_TEXT){String data;data.reserve(length+1);for(size_t i=0;i<length;i++)data+=(char)payload[i];if(data.startsWith("{\"type\":\"voice\"")){voiceMessage(data.c_str());return;}if(data.startsWith("{\"type\":\"forget\"")){forgetWifi(data.c_str());return;}if(example_lvgl_lock(100)){panel_ui_json(data.c_str());example_lvgl_unlock();}lastMessage=millis();}
}
void setup(){
 Serial.begin(115200);Serial.setTxTimeoutMs(0);i2c_master_Init();lvgl_port_init();lcd_bl_pwm_bsp_init(95);
 pinMode(BOOT_KEY,INPUT_PULLUP);pinMode(PWR_KEY,INPUT_PULLUP);micReady=voice_init();Serial.printf("MIC_READY %d\n",micReady);
 profilesLoad();
 socketClient.onEvent(wsEvent);socketClient.setReconnectInterval(5000);socketClient.enableHeartbeat(30000,5000,2);startWifi();Serial.println("AI_PANEL_READY");
}
void loop(){
 unsigned long loopNow=millis();if(loopPrevious)maxLoopMs=max(maxLoopMs,loopNow-loopPrevious);loopPrevious=loopNow;
 pollKeys();
 bool touchRequested=panel_ui_take_voice_request();
 if(touchRequested&&!panel_ui_is_locked()){Serial.println("VOICE_TOUCH");logVoiceButton("touch");toggleListening();}
 if(panel_ui_take_enter_request())sendEnter();
 while(Serial.available()){
  char c=Serial.read();if(c=='\n'){serialCommand();serialLine="";}
  else if(serialLine.length()<2048)serialLine+=c;else serialLine="";
 }
 wifiStep();
 if(linkPhase==LINK_UP){socketStep();socketClient.loop();}
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
