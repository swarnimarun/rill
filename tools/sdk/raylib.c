#include "rill/raylib.h"
#include <raylib.h>
#include <rlgl.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static bool opened, drawing, mode3d, camera_ready;
static Camera3D camera;
static int32_t status;

static bool invalid(const char *message) {
    status = 1;
    fprintf(stderr, "Rill Raylib: %s\n", message);
    return false;
}
static bool finite3(float x, float y, float z) {
    return isfinite(x) && isfinite(y) && isfinite(z);
}
static Color color(uint8_t red, uint8_t green, uint8_t blue, uint8_t alpha) {
    return (Color){ red, green, blue, alpha };
}
static bool in_2d(void) {
    return (opened && drawing && !mode3d) || invalid("2D drawing requires BeginDrawing outside a 3D scope");
}
static bool in_3d(void) {
    return (opened && drawing && mode3d) || invalid("3D drawing requires BeginMode3D");
}

bool RillRaylib_InitWindow(int32_t width, int32_t height, const char *title) {
    if (opened) return invalid("close the existing window before initializing another");
    status = 0;
    if (width < 1 || width > 8192 || height < 1 || height > 8192 || !title)
        return invalid("window requires dimensions 1..8192 and a nonnull title");
    const char *hidden = getenv("RILL_RAYLIB_HIDDEN");
    bool want_hidden = hidden && strcmp(hidden, "1") == 0;
    SetConfigFlags(want_hidden ? FLAG_WINDOW_HIDDEN : 0);
    InitWindow(width, height, title);
    if (!IsWindowReady()) {
        status = 2;
        fprintf(stderr, "Rill Raylib: native window initialization failed\n");
        return false;
    }
    // SetConfigFlags adds flags and Raylib retains them across CloseWindow.
    if (!want_hidden) ClearWindowState(FLAG_WINDOW_HIDDEN);
    opened = true;
    drawing = mode3d = camera_ready = false;
    return true;
}
bool RillRaylib_CloseWindow(void) {
    if (!opened || drawing || mode3d) return invalid("close requires an open window with drawing scopes ended");
    CloseWindow();
    opened = camera_ready = false;
    return true;
}
int32_t RillRaylib_Status(void) { return status; }
int32_t RillRaylib_WindowShouldClose(void) {
    if (!opened || drawing) { invalid("query window close state between drawing frames"); return -1; }
    return WindowShouldClose() ? 1 : 0;
}
bool RillRaylib_SetTargetFPS(int32_t fps) {
    if (!opened || drawing || fps < 0 || fps > 1000) return invalid("FPS requires an open idle window and a value 0..1000");
    SetTargetFPS(fps);
    return true;
}
bool RillRaylib_BeginDrawing(void) {
    if (!opened || drawing) return invalid("BeginDrawing requires an open idle window");
    BeginDrawing();
    drawing = true;
    return true;
}
bool RillRaylib_EndDrawing(void) {
    if (!opened || !drawing || mode3d) return invalid("EndDrawing requires an active frame with its 3D scope ended");
    EndDrawing();
    drawing = false;
    return true;
}
bool RillRaylib_ClearBackground(uint8_t red, uint8_t green, uint8_t blue, uint8_t alpha) {
    if (!in_2d()) return false;
    ClearBackground(color(red, green, blue, alpha));
    return true;
}
bool RillRaylib_DrawRectangle(int32_t x, int32_t y, int32_t width, int32_t height,
    uint8_t red, uint8_t green, uint8_t blue, uint8_t alpha) {
    if (!in_2d()) return false;
    if (width <= 0 || height <= 0) return invalid("rectangle dimensions must be positive");
    DrawRectangle(x, y, width, height, color(red, green, blue, alpha));
    return true;
}
bool RillRaylib_DrawText(const char *text, int32_t x, int32_t y, int32_t size,
    uint8_t red, uint8_t green, uint8_t blue, uint8_t alpha) {
    if (!in_2d()) return false;
    if (!text || size <= 0) return invalid("text requires a nonnull C string and positive font size");
    DrawText(text, x, y, size, color(red, green, blue, alpha));
    return true;
}
bool RillRaylib_SetCamera(float px, float py, float pz, float tx, float ty, float tz,
    float ux, float uy, float uz, float fovy, bool perspective) {
    if (!opened || mode3d) return invalid("set camera on an open window outside a 3D scope");
    if (!finite3(px, py, pz) || !finite3(tx, ty, tz) || !finite3(ux, uy, uz)
        || !isfinite(fovy) || fovy <= 0 || (perspective && fovy >= 180))
        return invalid("camera requires finite vectors and positive projection size, perspective FOV below 180");
    double dx = (double)tx - px, dy = (double)ty - py, dz = (double)tz - pz;
    double cx = dy * uz - dz * uy, cy = dz * ux - dx * uz, cz = dx * uy - dy * ux;
    if ((dx == 0 && dy == 0 && dz == 0) || (cx == 0 && cy == 0 && cz == 0))
        return invalid("camera target must differ from position and up must not parallel the view direction");
    camera = (Camera3D){ .position = { px, py, pz }, .target = { tx, ty, tz },
        .up = { ux, uy, uz }, .fovy = fovy,
        .projection = perspective ? CAMERA_PERSPECTIVE : CAMERA_ORTHOGRAPHIC };
    camera_ready = true;
    return true;
}
bool RillRaylib_BeginMode3D(void) {
    if (!in_2d()) return false;
    if (!camera_ready) return invalid("set a valid camera before BeginMode3D");
    BeginMode3D(camera);
    mode3d = true;
    return true;
}
bool RillRaylib_EndMode3D(void) {
    if (!in_3d()) return false;
    EndMode3D();
    mode3d = false;
    return true;
}
bool RillRaylib_DrawCube(float x, float y, float z, float width, float height, float length,
    uint8_t red, uint8_t green, uint8_t blue, uint8_t alpha) {
    if (!in_3d()) return false;
    if (!finite3(x, y, z) || !finite3(width, height, length) || width <= 0 || height <= 0 || length <= 0)
        return invalid("cube requires finite position and positive finite dimensions");
    DrawCube((Vector3){ x, y, z }, width, height, length, color(red, green, blue, alpha));
    return true;
}
bool RillRaylib_DrawPlane(float x, float y, float z, float width, float length,
    uint8_t red, uint8_t green, uint8_t blue, uint8_t alpha) {
    if (!in_3d()) return false;
    if (!finite3(x, y, z) || !isfinite(width) || !isfinite(length) || width <= 0 || length <= 0)
        return invalid("plane requires finite position and positive finite dimensions");
    DrawPlane((Vector3){ x, y, z }, (Vector2){ width, length }, color(red, green, blue, alpha));
    return true;
}
bool RillRaylib_Capture(const char *path) {
    if (!in_2d()) return false;
    if (!path || !*path) return invalid("capture requires a nonempty output path");
    rlDrawRenderBatchActive();
    Image image = LoadImageFromScreen();
    bool ok = IsImageValid(image) && ExportImage(image, path);
    if (image.data) UnloadImage(image);
    if (!ok) {
        status = 3;
        fprintf(stderr, "Rill Raylib: image readback or export failed\n");
    }
    return ok;
}
