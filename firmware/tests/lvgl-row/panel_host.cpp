#include "lvgl.h"
#include <cassert>
#include <string>
extern "C" {
LV_FONT_DECLARE(panel_font_16);
LV_FONT_DECLARE(panel_font_24);
LV_FONT_DECLARE(panel_font_30);
LV_FONT_DECLARE(panel_font_20);
LV_FONT_DECLARE(panel_font_28);
LV_FONT_DECLARE(panel_font_meta_16);
}
#define constrain(value, low, high) ((value)<(low)?(low):((value)>(high)?(high):(value)))
// Compile the production UI itself; only the hardware backlight is stubbed in main.c.
#include "../../ai-status-screen/panel_ui.cpp"
extern "C" void verify_panel_controls(void) {
    auto press=[](lv_obj_t *button){lv_event_send(button,LV_EVENT_PRESSED,NULL);lv_event_send(button,LV_EVENT_RELEASED,NULL);};
    lv_obj_update_layout(lv_scr_act());
    assert(CONTENT_WIDTH+CONTROL_WIDTH==640);
    auto *column=lv_obj_get_parent(voiceButton);
    assert(column==lv_obj_get_parent(enterButton));
    assert(lv_obj_get_x(column)==CONTENT_WIDTH);
    assert(lv_obj_get_width(column)==CONTROL_WIDTH);
    for(auto *button:{voiceButton,enterButton}) {
        assert(lv_obj_get_width(button)==76&&lv_obj_get_height(button)==76);
        assert(lv_obj_get_x(button)==8);
        assert(lv_obj_get_x(button)+lv_obj_get_width(button)<=CONTROL_WIDTH);
        assert(lv_obj_get_y(button)+lv_obj_get_height(button)<=172);
        for(unsigned i=0;i<lv_obj_get_child_cnt(button);i++)
            assert(!lv_obj_check_type(lv_obj_get_child(button,i),&lv_label_class));
    }
    assert(lv_obj_get_y(voiceButton)+76+8==lv_obj_get_y(enterButton));
    assert(lv_obj_get_child_cnt(voiceButton)==5);
    assert(lv_obj_get_child_cnt(enterButton)==1);
    panel_ui_set_locked(false);
    assert(!panel_ui_take_enter_request()&&!panel_ui_take_voice_request());
    press(enterButton);
    press(enterButton);
    assert(panel_ui_take_enter_request());
    assert(!panel_ui_take_enter_request()&&!panel_ui_take_voice_request());
    press(voiceButton);
    assert(panel_ui_take_voice_request());
    assert(!panel_ui_take_voice_request()&&!panel_ui_take_enter_request());
    // Locking discards pending input, and locked touch events cannot create requests.
    press(enterButton);
    panel_ui_set_locked(true);
    assert(!panel_ui_take_enter_request());
    press(enterButton);
    press(voiceButton);
    assert(!panel_ui_take_enter_request()&&!panel_ui_take_voice_request());
    panel_ui_set_locked(false);
    panel_ui_voice(VOICE_LISTENING,"","");
    lv_tick_inc(200);lv_timer_handler();
    for(auto *bar:voiceBars) {
        assert(lv_obj_get_y(bar)>0);
        assert(lv_obj_get_y(bar)+lv_obj_get_height(bar)<BUTTON_SIZE);
    }
    panel_ui_voice(VOICE_RECOGNIZING,"","");
    assert(lv_obj_has_state(voiceButton,LV_STATE_DISABLED));
    assert(!lv_obj_has_state(enterButton,LV_STATE_DISABLED));
    panel_ui_voice(VOICE_HIDDEN,"","");
    assert(!lv_obj_has_state(voiceButton,LV_STATE_DISABLED));
}
extern "C" void stress_panel_rows(const char *json) {
    JsonDocument document;assert(!deserializeJson(document,json));
    // The selected card title takes precedence over the urgency-sorted session list.
    JsonDocument selected;
    selected["id"]="pinned";selected["project"]="fallback";
    selected["rows"][0]["id"]="other";selected["rows"][0]["project"]="wrong task";
    selected["rows"][1]["id"]="pinned";selected["rows"][1]["project"]="固定的会话";
    assert(!strcmp(task_title(selected.as<JsonVariant>()),"固定的会话"));
    selected["title"]="明确的标题";
    assert(!strcmp(task_title(selected.as<JsonVariant>()),"明确的标题"));
    selected.clear();assert(!strcmp(task_title(selected.as<JsonVariant>()),"暂无任务"));
    selected["rows"][0]["project"]="旧版会话";
    assert(!strcmp(task_title(selected.as<JsonVariant>()),"旧版会话"));
    // Title edges must retain intermediate coverage; monochrome glyphs look jagged on the LCD.
    for(uint32_t cp:{0x4efbU,0x52a1U,0x540dU,0x79f0U}) {
        lv_font_glyph_dsc_t glyph;
        assert(lv_font_get_glyph_dsc(&panel_font_20,&glyph,cp,0));
        assert(!glyph.is_placeholder&&glyph.bpp==4);
        const uint8_t *bitmap=lv_font_get_glyph_bitmap(&panel_font_20,cp);
        assert(bitmap);
        bool antialiased=false;
        for(unsigned pixel=0;pixel<unsigned(glyph.box_w)*glyph.box_h;pixel++) {
            unsigned coverage=(bitmap[pixel/2]>>(pixel%2?0:4))&15;
            if(coverage>0&&coverage<15)antialiased=true;
        }
        assert(antialiased);
    }
    // Every status character must exist in the embedded status font.
    const char *labels[]={"执行中","待确认","已完成","已中断","空闲"};
    for(const char *text:labels) {
        uint32_t at=0;
        while(text[at]) {
            uint32_t cp=_lv_txt_encoded_next(text,&at);lv_font_glyph_dsc_t glyph;
            assert(lv_font_get_glyph_dsc(&panel_font_28,&glyph,cp,0));
            assert(!glyph.is_placeholder);
        }
    }
    for(int frame=0;frame<100;frame++) {
        document["config"]["split"]=frame%3==0?35:frame%3==1?50:65;
        document["config"]["swap"]=frame%2!=0;
        document["config"]["deepseekColor"]=frame%2?"#7c9fff":"#d987cd";
        document["config"]["showElapsed"]=frame%3!=0;
        document["config"]["showTool"]=frame%4!=0;
        document["cards"][0]["tool"]="mcp__very_long_tool_name_that_exceeds_the_width_of_the_column";
        document["cards"][0]["title"]="这是一段超过屏幕宽度的中文对话标题";
        document["cards"][1]["title"]="English title that exceeds the width";
        document["cards"][2]["title"]="#ff0000";
        std::string wire;serializeJson(document,wire);panel_ui_json(wire.c_str());lv_obj_update_layout(lv_scr_act());
        for(int i=0;i<3;i++) {
            int source=i<2&&frame%2?1-i:i;
            int width=lv_obj_get_width(panels[i]);
            assert(lv_img_get_src(brandIcons[i])==&panel_provider_logos[source]);
            assert(lv_obj_get_width(brandIcons[i])==20&&lv_obj_get_height(brandIcons[i])==20);
            assert(lv_obj_get_x(brandIcons[i])+20+6==lv_obj_get_x(brands[i]));
            assert(!strcmp(lv_label_get_text(brands[i]),providerNames[source]));
            assert(lv_color_to32(lv_obj_get_style_text_color(brands[i],0))==lv_color_to32(lv_obj_get_style_img_recolor(brandIcons[i],0)));
            lv_point_t brandSize;lv_txt_get_size(&brandSize,providerNames[source],&lv_font_montserrat_16,0,0,LV_COORD_MAX,LV_TEXT_FLAG_NONE);
            assert(brandSize.x<=lv_obj_get_width(brands[i]));
            for(auto *item:{brands[i],states[i],titles[i],details[i],elapsed[i]}) {
                assert(lv_obj_get_x(item)>=0);
                assert(lv_obj_get_x(item)+lv_obj_get_width(item)<=width);
                assert(lv_obj_get_y(item)+lv_obj_get_height(item)<=172);
            }
            assert(lv_obj_get_y(states[i])+lv_obj_get_height(states[i])<=lv_obj_get_y(titles[i]));
            assert(lv_obj_get_y(titles[i])+lv_obj_get_height(titles[i])<lv_obj_get_y(details[i]));
            int stateGap=lv_obj_get_y(titles[i])-lv_obj_get_y(states[i])-lv_obj_get_height(states[i]);
            int footerGap=lv_obj_get_y(details[i])-lv_obj_get_y(titles[i])-lv_obj_get_height(titles[i]);
            assert(stateGap>=8&&stateGap<=10);
            assert(footerGap>=8&&footerGap<=10);
            assert(lv_obj_get_x(details[i])+lv_obj_get_width(details[i])<=lv_obj_get_x(elapsed[i]));
            assert(lv_obj_get_height(titles[i])==lv_font_get_line_height(&panel_font_20));
            assert(!lv_label_get_recolor(titles[i]));
            const char *expected=document["cards"][source]["title"];
            const char *shown=lv_label_get_text(titles[i]);
            assert(!strncmp(shown,expected,_lv_txt_encoded_size(expected)));
            for(const char *p=shown;*p;) {unsigned size=_lv_txt_encoded_size(p);assert(size);p+=size;}
            if(source==2)assert(!strcmp(shown,"#ff0000"));
            if(frame%3==0)assert(!*lv_label_get_text(elapsed[i]));
            if(source==0&&frame%4==0)assert(!*lv_label_get_text(details[i]));
            if(source==1)assert(lv_color_to32(lv_obj_get_style_img_recolor(brandIcons[i],0))==lv_color_to32(lv_color_hex(0xffffff)));
        }
    }
}
extern "C" void stress_panel_visibility(const char *json) {
    JsonDocument document;assert(!deserializeJson(document,json));
    const char *providers[]={"claude","codex","deepseek"};
    for(int mask=7;mask>=0;mask--)for(int swap=0;swap<2;swap++)for(int split:{35,50,65}) {
        document["config"]["swap"]=bool(swap);document["config"]["split"]=split;
        int expected[3],count=0;
        for(int p=0;p<3;p++)document["running"][providers[p]]=bool(mask&(1<<p));
        for(int slot=0;slot<3;slot++) {
            int p=slot<2&&swap?1-slot:slot;
            if(mask&(1<<p))expected[count++]=p;
        }
        std::string wire;serializeJson(document,wire);panel_ui_json(wire.c_str());lv_obj_update_layout(lv_scr_act());
        int right=0;
        for(int i=0;i<3;i++) {
            assert(lv_obj_has_flag(panels[i],LV_OBJ_FLAG_HIDDEN)==(i>=count));
            if(i>=count)continue;
            assert(lv_img_get_src(brandIcons[i])==&panel_provider_logos[expected[i]]);
            assert(!strcmp(lv_label_get_text(brands[i]),providerNames[expected[i]]));
            assert(lv_obj_get_x(panels[i])==right);
            right+=lv_obj_get_width(panels[i]);
            assert(right<=CONTENT_WIDTH);
            if(count==1)assert(lv_obj_get_width(panels[i])==CONTENT_WIDTH);
            if(count==2&&mask!=3)assert(lv_obj_get_width(panels[i])==CONTENT_WIDTH/2);
        }
        assert(right==(count?CONTENT_WIDTH:0));
        assert(!lv_obj_has_flag(lv_obj_get_parent(voiceButton),LV_OBJ_FLAG_HIDDEN));
    }
    // The board still understands old servers with no presence map and two cards.
    document.remove("running");document["cards"].as<JsonArray>().remove(2);
    std::string wire;serializeJson(document,wire);panel_ui_json(wire.c_str());lv_obj_update_layout(lv_scr_act());
    assert(!lv_obj_has_flag(panels[0],LV_OBJ_FLAG_HIDDEN));
    assert(!lv_obj_has_flag(panels[1],LV_OBJ_FLAG_HIDDEN));
    assert(lv_obj_has_flag(panels[2],LV_OBJ_FLAG_HIDDEN));
    assert(lv_obj_get_width(panels[0])+lv_obj_get_width(panels[1])==CONTENT_WIDTH);
    // Reopen just DeepSeek after all slots have been hidden/reused.
    assert(!deserializeJson(document,json));
    for(int p=0;p<3;p++)document["running"][providers[p]]=p==2;
    wire.clear();serializeJson(document,wire);panel_ui_json(wire.c_str());lv_obj_update_layout(lv_scr_act());
    assert(!strcmp(lv_label_get_text(brands[0]),"DeepSeek"));
    assert(lv_obj_get_width(panels[0])==CONTENT_WIDTH);
}

extern "C" void verify_title_overflow(void) {
    const char *cases[]={
        "这是一个非常非常长的中文任务名称用于检查边界是否正确省略并保持单行",
        "An English task title with many words that cannot fit in one column",
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ",
        "修复 ESP32-S3 的 WebSocket reconnect 流程并增加 long_name_test 测试",
        "#ff0000 只是普通文字，不应触发颜色命令，也不应该影响后面的任务名",
        "第一行任务名\n第二行不应撑高标题\r\n第三行",
        "任务😀测试🚀以及非常长的标题来检查多字节字符省略",
        "短标题"
    };
    const char *providers[]={"claude","codex","deepseek"};
    JsonDocument d;d["type"]="state";
    unsigned checked=0;
    for(int mask=1;mask<8;mask++)for(int split:{35,50,65})for(bool swap:{false,true}) {
        d["config"]["split"]=split;d["config"]["swap"]=swap;
        for(int p=0;p<3;p++) {
            d["running"][providers[p]]=bool(mask&(1<<p));
            d["cards"][p]["provider"]=providers[p];
            d["cards"][p]["state"]="working";
        }
        for(const char *title:cases) {
            for(int p=0;p<3;p++)d["cards"][p]["title"]=title;
            std::string wire;serializeJson(d,wire);panel_ui_json(wire.c_str());lv_obj_update_layout(lv_scr_act());
            for(int slot=0;slot<3;slot++) {
                if(lv_obj_has_flag(panels[slot],LV_OBJ_FLAG_HIDDEN))continue;
                auto *label=titles[slot];
                assert(lv_label_get_long_mode(label)==LV_LABEL_LONG_DOT);
                assert(lv_obj_get_height(label)==lv_font_get_line_height(&panel_font_20));
                assert(lv_obj_get_y(label)+lv_obj_get_height(label)<lv_obj_get_y(details[slot]));
                assert(lv_obj_get_x(label)+lv_obj_get_width(label)<=lv_obj_get_width(panels[slot])-PAD);
                const char *shown=lv_label_get_text(label);
                // Decode the complete displayed prefix: ellipsis must not split a UTF-8 character.
                for(const unsigned char *p=(const unsigned char*)shown;*p;) {
                    unsigned n=*p<0x80?1:*p<0xe0?2:*p<0xf0?3:4;
                    for(unsigned j=1;j<n;j++)assert(p[j]&&(p[j]&0xc0)==0x80);
                    p+=n;
                }
                if(!strchr(title,'\n')) {
                    lv_point_t textSize;lv_txt_get_size(&textSize,shown,&panel_font_20,0,0,LV_COORD_MAX,LV_TEXT_FLAG_NONE);
                    assert(textSize.x<=lv_obj_get_width(label));
                    lv_txt_get_size(&textSize,title,&panel_font_20,0,0,LV_COORD_MAX,LV_TEXT_FLAG_NONE);
                    if(textSize.x>lv_obj_get_width(label)) {
                        size_t n=strlen(shown);assert(n>=3&&!strcmp(shown+n-3,"..."));
                    } else assert(!strcmp(shown,title));
                }
                checked++;
            }
        }
    }
    printf("PASS: %u title overflow cases, Chinese/English/mixed/emoji/newlines, all column widths, ellipsis, UTF-8 and long-to-short updates\n",checked);
}
