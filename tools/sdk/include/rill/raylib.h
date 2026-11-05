#ifndef RILL_RAYLIB_H
#define RILL_RAYLIB_H
#include <stdbool.h>
#include <stdint.h>
#if defined(_WIN32)
# if defined(RILL_RAYLIB_BUILD)
#  define RILL_RAYLIB_API __declspec(dllexport)
# else
#  define RILL_RAYLIB_API __declspec(dllimport)
# endif
#else
# define RILL_RAYLIB_API __attribute__((visibility("default")))
#endif
#ifdef __cplusplus
extern "C" {
#endif
/* One window, all calls on the application main thread. Do not mix the bridge
   with direct Raylib lifecycle calls. No C aggregate layout crosses this ABI.
   InitWindow -> SetCamera -> BeginDrawing -> [BeginMode3D -> EndMode3D] ->
   EndDrawing -> CloseWindow. Balance begun scopes even when drawing fails.
   Status is the last error since InitWindow: 0 none, 1 invalid argument/state,
   2 native initialization, 3 image export failure. Native platform errors can
   also terminate inside Raylib. No interpreter or software fallback is used. */
RILL_RAYLIB_API bool RillRaylib_InitWindow(int32_t width, int32_t height, const char *title);
RILL_RAYLIB_API bool RillRaylib_CloseWindow(void);
RILL_RAYLIB_API int32_t RillRaylib_Status(void);
/* -1 invalid state, 0 keep running, 1 close requested. */
RILL_RAYLIB_API int32_t RillRaylib_WindowShouldClose(void);
RILL_RAYLIB_API bool RillRaylib_SetTargetFPS(int32_t fps);
RILL_RAYLIB_API bool RillRaylib_BeginDrawing(void);
RILL_RAYLIB_API bool RillRaylib_EndDrawing(void);
RILL_RAYLIB_API bool RillRaylib_ClearBackground(uint8_t red, uint8_t green, uint8_t blue, uint8_t alpha);
RILL_RAYLIB_API bool RillRaylib_DrawRectangle(int32_t x, int32_t y, int32_t width, int32_t height,
    uint8_t red, uint8_t green, uint8_t blue, uint8_t alpha);
RILL_RAYLIB_API bool RillRaylib_DrawText(const char *text, int32_t x, int32_t y, int32_t size,
    uint8_t red, uint8_t green, uint8_t blue, uint8_t alpha);
RILL_RAYLIB_API bool RillRaylib_SetCamera(float px, float py, float pz,
    float tx, float ty, float tz, float ux, float uy, float uz, float fovy, bool perspective);
RILL_RAYLIB_API bool RillRaylib_BeginMode3D(void);
RILL_RAYLIB_API bool RillRaylib_EndMode3D(void);
RILL_RAYLIB_API bool RillRaylib_DrawCube(float x, float y, float z, float width, float height, float length,
    uint8_t red, uint8_t green, uint8_t blue, uint8_t alpha);
RILL_RAYLIB_API bool RillRaylib_DrawPlane(float x, float y, float z, float width, float length,
    uint8_t red, uint8_t green, uint8_t blue, uint8_t alpha);
/* Capture during 2D drawing, after EndMode3D and before EndDrawing. The caller
   chooses the output path. All temporary Image storage is released in C. */
RILL_RAYLIB_API bool RillRaylib_Capture(const char *path);
#ifdef __cplusplus
}
#endif
#endif
