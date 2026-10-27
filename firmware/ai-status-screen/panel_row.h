#pragma once
#include "lvgl.h"

// Keep the status marker outside the title label. Inline recolor commands and
// automatic-height text must not participate in the fixed 20-pixel row layout.
typedef struct {
    lv_obj_t *dot;
    lv_obj_t *title;
} panel_row_t;

static inline void panel_row_init(panel_row_t *row,lv_obj_t *parent,int x,int y,int width,const lv_font_t *font){
    row->dot=lv_obj_create(parent);lv_obj_remove_style_all(row->dot);
    lv_obj_set_pos(row->dot,x,y+(lv_font_get_line_height(font)-6)/2);
    lv_obj_set_size(row->dot,6,6);lv_obj_set_style_radius(row->dot,LV_RADIUS_CIRCLE,0);
    lv_obj_set_style_bg_opa(row->dot,LV_OPA_COVER,0);lv_obj_clear_flag(row->dot,LV_OBJ_FLAG_CLICKABLE);
    row->title=lv_label_create(parent);lv_obj_set_pos(row->title,x+12,y);
    lv_obj_set_style_text_font(row->title,font,0);lv_label_set_recolor(row->title,false);
    lv_label_set_long_mode(row->title,LV_LABEL_LONG_DOT);
    lv_obj_set_size(row->title,width-12,lv_font_get_line_height(font));
    lv_label_set_text(row->title,"");lv_obj_add_flag(row->dot,LV_OBJ_FLAG_HIDDEN);
}

static inline void panel_row_width(panel_row_t *row,int width){
    lv_obj_set_width(row->title,width-12);
}

static inline void panel_row_set(panel_row_t *row,const char *title,uint32_t dot_color,uint32_t text_color,bool visible){
    lv_obj_set_style_bg_color(row->dot,lv_color_hex(dot_color),0);
    lv_obj_set_style_text_color(row->title,lv_color_hex(text_color),0);
    if(visible){lv_obj_clear_flag(row->dot,LV_OBJ_FLAG_HIDDEN);lv_label_set_text(row->title,title&&*title?title:"-");}
    else{lv_obj_add_flag(row->dot,LV_OBJ_FLAG_HIDDEN);lv_label_set_text(row->title,"");}
}
