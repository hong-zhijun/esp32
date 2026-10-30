#pragma once

// Approximate resting single-cell Li-ion curve, not a hardware fuel-gauge reading.
// Out-of-range readings (including USB-only boards without a cell) stay unknown.
static inline int battery_percent_from_mv(int mv){
 if(mv<2800||mv>4350)return -1;
 static const int millivolts[]={3300,3500,3600,3680,3740,3790,3850,3920,4000,4100,4200};
 static const int percentages[]={0,5,10,20,30,40,50,60,75,90,100};
 if(mv<=millivolts[0])return 0;
 for(int i=1;i<11;i++)if(mv<millivolts[i])return percentages[i-1]+(mv-millivolts[i-1])*(percentages[i]-percentages[i-1])/(millivolts[i]-millivolts[i-1]);
 return 100;
}
