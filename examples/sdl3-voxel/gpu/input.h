#ifndef RILL_VOXEL_INPUT_H
#define RILL_VOXEL_INPUT_H
#include <stdbool.h>
#include <stdint.h>

typedef struct VoxelInput {
    bool focused, captured, discard_motion, just_captured;
    bool fire_pressed, fire_down, jump_pressed, quit;
    int32_t motion[2];
} VoxelInput;
typedef enum CaptureAction { CAPTURE_NONE, CAPTURE_ENABLE, CAPTURE_DISABLE, CAPTURE_QUIT } CaptureAction;

static inline void vi_begin(VoxelInput *s) {
    s->motion[0]=s->motion[1]=0;
    s->fire_pressed=false; s->jump_pressed=false;
    s->discard_motion=false; s->just_captured=false;
}
static inline void vi_release(VoxelInput *s) {
    s->captured=false; s->discard_motion=true;
    s->fire_pressed=s->fire_down=s->jump_pressed=false;
    s->motion[0]=s->motion[1]=0;
}
static inline CaptureAction vi_focus(VoxelInput *s, bool focused) {
    bool captured=s->captured;
    vi_release(s); s->focused=focused;
    return captured ? CAPTURE_DISABLE:CAPTURE_NONE;
}
static inline CaptureAction vi_click(VoxelInput *s, bool left, bool down) {
    if(!s->focused) return CAPTURE_NONE;
    if(down && !s->captured) {
        s->captured=true; s->discard_motion=true; s->just_captured=true;
        s->motion[0]=s->motion[1]=0;
        s->fire_down=s->fire_pressed=false;
        return CAPTURE_ENABLE;
    }
    if(left) {
        if(!down) s->fire_down=false;
        else if(s->captured && !s->just_captured) s->fire_down=s->fire_pressed=true;
    }
    return CAPTURE_NONE;
}
static inline CaptureAction vi_escape(VoxelInput *s, bool quit_when_free) {
    if(s->captured) { vi_release(s); return CAPTURE_DISABLE; }
    if(quit_when_free) { s->quit=true; return CAPTURE_QUIT; }
    return CAPTURE_NONE;
}
static inline void vi_motion(VoxelInput *s, int32_t x, int32_t y) {
    if(!s->focused || !s->captured || s->discard_motion) return;
    int64_t next_x=(int64_t)s->motion[0]+x, next_y=(int64_t)s->motion[1]+y;
    s->motion[0]=(int32_t)(next_x>1024?1024:next_x< -1024?-1024:next_x);
    s->motion[1]=(int32_t)(next_y>1024?1024:next_y< -1024?-1024:next_y);
}
#endif
