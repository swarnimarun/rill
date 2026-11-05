#ifndef RILL_ENGINE_RENDERER_H
#define RILL_ENGINE_RENDERER_H
#include <stdbool.h>
#include <stdint.h>
typedef struct Engine Engine;
Engine *e3_open(int32_t width, int32_t height);
void e3_close(Engine *engine);
int32_t e3_status(void);
bool e3_headless(Engine *engine);
int32_t e3_mode(Engine *engine);
int32_t e3_limit(Engine *engine);
int32_t e3_width(Engine *engine);
int32_t e3_height(Engine *engine);
bool e3_poll(Engine *engine);
int32_t e3_key(Engine *engine, int32_t action);
int32_t e3_mouse(Engine *engine, int32_t axis);
int32_t e3_elapsed(Engine *engine);
float e3_sin(int32_t millidegrees);
float e3_cos(int32_t millidegrees);
int32_t e3_mesh_new(Engine *engine, int32_t capacity);
bool e3_vertex(Engine *engine, int32_t mesh, float x, float y, float z, float nx, float ny, float nz);
bool e3_mesh_upload(Engine *engine, int32_t mesh);
bool e3_begin(Engine *engine);
bool e3_matrix(Engine *engine, int32_t slot, int32_t row, float x, float y, float z, float w);
bool e3_material(Engine *engine, float r, float g, float b, float lx, float ly, float lz, float ambient);
bool e3_draw(Engine *engine, int32_t mesh);
bool e3_end(Engine *engine);
bool e3_capture(Engine *engine);
int32_t e3_pixel(Engine *engine, int32_t x, int32_t y, int32_t component);
#endif
