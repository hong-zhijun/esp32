#include "lvgl.h"
#include <cassert>
#include <string>
extern "C" {
LV_FONT_DECLARE(panel_font_16);
LV_FONT_DECLARE(panel_font_24);
LV_FONT_DECLARE(panel_font_30);
}
#define constrain(value, low, high) ((value)<(low)?(low):((value)>(high)?(high):(value)))
// Compile the production UI itself; only the hardware backlight is stubbed in main.c.
#include "../../ai-status-screen/panel_ui.cpp"
extern "C" void stress_panel_rows(const char *json) {
    JsonDocument document;assert(!deserializeJson(document,json));
    document["cards"][2]["rows"][2]["state"]="idle";
    document["cards"][2]["rows"][2]["project"]="#ff0000";
    for(int frame=0;frame<100;frame++) {
        document["config"]["split"]=frame%3==0?35:frame%3==1?50:65;
        document["config"]["swap"]=frame%2!=0;
        document["config"]["deepseekColor"]=frame%2?"#7c9fff":"#d987cd";
        document["cards"][0]["tool"]="mcp__very_long_tool_name_that_exceeds_the_width_of_the_column";
        std::string wire;serializeJson(document,wire);panel_ui_json(wire.c_str());lv_obj_update_layout(lv_scr_act());
        for(int i=0;i<3;i++) {
            int source=i<2&&frame%2?1-i:i;
            assert(lv_obj_get_height(details[i])==19);
            assert(lv_img_get_src(brandIcons[i])==&panel_provider_logos[source]);
            assert(lv_obj_get_width(brandIcons[i])==20&&lv_obj_get_height(brandIcons[i])==20);
            assert(lv_obj_get_x(brandIcons[i])+lv_obj_get_width(brandIcons[i])<=lv_obj_get_x(brands[i]));
            assert(lv_obj_get_x(brands[i])+lv_obj_get_width(brands[i])<=lv_obj_get_width(panels[i]));
            assert(!strcmp(lv_label_get_text(brands[i]),providerNames[source]));
            assert(lv_color_to32(lv_obj_get_style_img_recolor(brandIcons[i],0))==lv_color_to32(lv_obj_get_style_text_color(brands[i],0)));
        }
        for(int i=0;i<3;i++)for(int row=0;row<3;row++) {
            int source=i<2&&frame%2?1-i:i;
            const char *state=document["cards"][source]["rows"][row]["state"]|"";
            const char *expected=document["cards"][source]["rows"][row]["project"]|"";
            lv_obj_t *label=lists[i][row].title;const char *shown=lv_label_get_text(label);
            assert(lv_obj_get_height(label)==19);
            assert(!lv_label_get_recolor(label));
            assert(lv_obj_has_flag(lists[i][row].dot,LV_OBJ_FLAG_HIDDEN)==(!*state));
            if(*state) {
                assert(*shown);
                if(*expected)assert(!strncmp(shown,expected,_lv_txt_encoded_size(expected)));
                if(i==2&&row==2)assert(!strcmp(shown,"#ff0000"));
            } else assert(!*shown);
            for(const char *p=shown;*p;) {unsigned size=_lv_txt_encoded_size(p);assert(size);p+=size;}
            if(row<2)assert(lv_obj_get_y(label)+lv_obj_get_height(label)<=lv_obj_get_y(lists[i][row+1].title));
        }
    }
}
