#pragma once
#include <Arduino.h>
// Saved Wi-Fi + computer pairs, kept in NVS as one JSON array (most recently configured first).
#define WIFI_PROFILE_MAX 4
struct WifiProfile{String ssid,pass,token,id,host;int port=8787;};
void profilesLoad();                       // NVS -> RAM; turns the old single-Wi-Fi keys into one profile, once
int profileCount();
const WifiProfile &profileAt(int i);
int profileFind(const String &ssid);       // -1 when not saved
bool profileValid(const WifiProfile &p);   // the limits the USB configure command has always enforced (+ id)
void profileUpsert(const WifiProfile &p);  // same ssid is replaced; the saved one moves to the front; a 5th drops the oldest
bool profileRemove(const String &ssid);
void profileSetHost(const String &ssid,const String &host);
