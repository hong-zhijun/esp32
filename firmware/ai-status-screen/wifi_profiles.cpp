#include "wifi_profiles.h"
#include <Preferences.h>
#include <ArduinoJson.h>
static Preferences prefs;
static WifiProfile list[WIFI_PROFILE_MAX];
static int listCount=0;
// The whole list is rewritten on each change: it is a few hundred bytes and changes rarely (flash wear).
static void saveAll(){
 JsonDocument d;JsonArray a=d.to<JsonArray>();
 for(int i=0;i<listCount;i++){JsonObject o=a.add<JsonObject>();o["ssid"]=list[i].ssid;o["pass"]=list[i].pass;o["token"]=list[i].token;o["id"]=list[i].id;o["host"]=list[i].host;o["port"]=list[i].port;}
 String json;serializeJson(d,json);prefs.putString("profiles",json);
}
void profilesLoad(){
 prefs.begin("ai-panel",false);
 if(prefs.isKey("profiles")){
  JsonDocument d;if(deserializeJson(d,prefs.getString("profiles")))return;
  for(JsonObject o:d.as<JsonArray>()){
   if(listCount>=WIFI_PROFILE_MAX)break;
   WifiProfile &p=list[listCount++];p.ssid=o["ssid"]|"";p.pass=o["pass"]|"";p.token=o["token"]|"";p.id=o["id"]|"";p.host=o["host"]|"";p.port=o["port"]|8787;
  }
  return;
 }
 // Firmware before multi-Wi-Fi kept one network in flat keys. Migrate it once; with no id, its saved host is used as-is.
 WifiProfile &p=list[0];p.ssid=prefs.getString("ssid","");
 if(!p.ssid.length())return;
 p.pass=prefs.getString("password","");p.token=prefs.getString("token","");p.host=prefs.getString("host","");p.port=prefs.getInt("port",8787);
 listCount=1;saveAll();
}
int profileCount(){return listCount;}
const WifiProfile &profileAt(int i){return list[i];}
int profileFind(const String &ssid){for(int i=0;i<listCount;i++)if(list[i].ssid==ssid)return i;return -1;}
bool profileValid(const WifiProfile &p){return p.ssid.length()>0&&p.ssid.length()<=32&&p.pass.length()<=63&&p.host.length()>0&&p.host.length()<100&&p.token.length()>=32&&p.token.length()<100&&p.id.length()<=32&&p.port>0&&p.port<=65535;}
void profileUpsert(const WifiProfile &p){
 int i=profileFind(p.ssid);
 if(i<0)i=listCount<WIFI_PROFILE_MAX?listCount++:WIFI_PROFILE_MAX-1;   // list is full: the oldest (last) slot is reused
 for(;i>0;i--)list[i]=list[i-1];
 list[0]=p;saveAll();
}
bool profileRemove(const String &ssid){
 int i=profileFind(ssid);if(i<0)return false;
 for(;i<listCount-1;i++)list[i]=list[i+1];
 listCount--;saveAll();return true;
}
void profileSetHost(const String &ssid,const String &host){
 int i=profileFind(ssid);if(i<0||list[i].host==host)return;
 list[i].host=host;saveAll();
}
