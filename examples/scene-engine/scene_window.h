#ifndef RILL_SCENE_WINDOW_H
#define RILL_SCENE_WINDOW_H
#include <stdbool.h>
#include <stdint.h>

bool scene_window_open(void);
int32_t scene_window_event(void);
bool scene_window_begin(void);
bool scene_window_cell(int32_t index, int32_t glyph);
bool scene_window_present(int32_t score);
void scene_window_close(void);
#endif
