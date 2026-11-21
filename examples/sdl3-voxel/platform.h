#ifndef RILL_VOXEL_PLATFORM_H
#define RILL_VOXEL_PLATFORM_H
#include <stdbool.h>
#include <stdint.h>
bool vx_open(void);
bool vx_poll(void);
int32_t vx_key(int32_t action);
int32_t vx_buttons(void);
int32_t vx_mouse_x(void);
int32_t vx_mouse_y(void);
int32_t vx_elapsed(void);
bool vx_smoke(void);
bool vx_begin(void);
bool vx_color(int32_t r, int32_t g, int32_t b);
bool vx_line(int32_t x1, int32_t y1, int32_t x2, int32_t y2);
bool vx_text(int32_t x, int32_t y, const char *text);
bool vx_hud(int32_t removed, int32_t moved, int32_t view, bool paused);
bool vx_present(void);
void vx_close(void);
#endif
