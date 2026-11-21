#ifndef RILL_VOXEL_GPU_H
#define RILL_VOXEL_GPU_H
#include <stdbool.h>
#include <stdint.h>
typedef struct GPUParams {
    float eye[4], forward[4], right[4], up[4], edit[4], dynamics[4];
    uint32_t world[4], frame[4], limits[4], config[4];
    float movement[4];
    uint32_t controller[4];
} GPUParams;
typedef struct GPUStructure {
    float rest[4],position[4],velocity[4],angle[4],spin[4],bounds_min[4],bounds_max[4];
    uint32_t info[4];
} GPUStructure;
typedef struct GPUPlayer {
    float feet_ground[4],velocity_height[4],eye_mode[4];
    uint32_t flags[4];
} GPUPlayer;
bool vg_open(void);
void vg_close(void);
bool vg_poll(void);
int32_t vg_key(int32_t key);
int32_t vg_mouse(int32_t axis);
int32_t vg_elapsed(void);
int32_t vg_world(void);
int32_t vg_sine(int32_t angle);
int32_t vg_cosine(int32_t angle);
bool vg_smoke(void);
bool vg_automatic_demo(void);
int32_t vg_mode(void);
bool vg_lean(void);
bool vg_begin(int32_t x, int32_t y, int32_t z, int32_t yaw, int32_t pitch, int32_t elapsed, int32_t frame, int32_t view);
bool vg_dispatch(int32_t pass);
bool vg_controls(int32_t mode, bool reset, bool paused);
bool vg_aim(int32_t radius);
bool vg_demo(int32_t radius);
bool vg_present(void);
int32_t vg_test(void);
int32_t vg_input_test(void);
#endif
