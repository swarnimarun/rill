#define _POSIX_C_SOURCE 200809L
#include "rill/raylib.h"
#include <raylib.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// Keep checks active in CMake Release builds, including side-effecting calls.
#define CHECK(expression) do { if (!(expression)) { \
    fprintf(stderr, "Raylib SDK check failed at line %d: %s\n", __LINE__, #expression); \
    return 1; } } while (0)

static int closed_state(void) {
    CHECK(!RillRaylib_CloseWindow());
    CHECK(RillRaylib_WindowShouldClose() == -1);
    CHECK(!RillRaylib_SetTargetFPS(60));
    CHECK(!RillRaylib_BeginDrawing());
    CHECK(!RillRaylib_EndDrawing());
    CHECK(!RillRaylib_ClearBackground(1, 2, 3, 255));
    CHECK(!RillRaylib_DrawRectangle(0, 0, 1, 1, 1, 2, 3, 255));
    CHECK(!RillRaylib_DrawText("closed", 0, 0, 12, 1, 2, 3, 255));
    CHECK(!RillRaylib_SetCamera(3, 3, 3, 0, 0, 0, 0, 1, 0, 45, true));
    CHECK(!RillRaylib_BeginMode3D());
    CHECK(!RillRaylib_EndMode3D());
    CHECK(!RillRaylib_DrawCube(0, 0, 0, 1, 1, 1, 1, 2, 3, 255));
    CHECK(!RillRaylib_DrawPlane(0, 0, 0, 1, 1, 1, 2, 3, 255));
    CHECK(!RillRaylib_Capture("closed.png"));
    CHECK(!RillRaylib_InitWindow(0, 240, "invalid"));
    CHECK(!RillRaylib_InitWindow(320, -1, "invalid"));
    CHECK(!RillRaylib_InitWindow(320, 240, NULL));
    CHECK(RillRaylib_Status() == 1);
    return 0;
}

static int window_test(const char *path) {
    CHECK(RillRaylib_InitWindow(320, 240, "Rill Raylib SDK hardware test"));
    CHECK(RillRaylib_SetTargetFPS(0));
    CHECK(!RillRaylib_InitWindow(320, 240, "duplicate"));
    CHECK(!RillRaylib_SetCamera(NAN, 3, 3, 0, 0, 0, 0, 1, 0, 45, true));
    CHECK(!RillRaylib_SetCamera(0, 0, 0, 0, 0, 0, 0, 1, 0, 45, true));
    CHECK(!RillRaylib_SetCamera(0, 0, 3, 0, 0, 0, 0, 0, 1, 45, true));
    CHECK(!RillRaylib_SetCamera(3, 3, 3, 0, 0, 0, 0, 1, 0, 180, true));
    CHECK(RillRaylib_BeginDrawing());
    CHECK(!RillRaylib_BeginDrawing());
    CHECK(!RillRaylib_CloseWindow());
    CHECK(RillRaylib_WindowShouldClose() == -1);
    CHECK(!RillRaylib_BeginMode3D());
    CHECK(RillRaylib_SetCamera(3, 3, 3, 0, 0, 0, 0, 1, 0, 45, true));
    CHECK(RillRaylib_ClearBackground(12, 20, 30, 255));
    CHECK(RillRaylib_BeginMode3D());
    CHECK(!RillRaylib_EndDrawing());
    CHECK(!RillRaylib_DrawRectangle(0, 0, 10, 10, 1, 2, 3, 255));
    CHECK(!RillRaylib_SetCamera(3, 3, 3, 0, 0, 0, 0, 1, 0, 45, true));
    CHECK(!RillRaylib_DrawCube(0, 0, 0, NAN, 2, 2, 1, 2, 3, 255));
    CHECK(!RillRaylib_DrawPlane(0, 0, 0, 0, 1, 1, 2, 3, 255));
    CHECK(RillRaylib_DrawPlane(0, -1, 0, 8, 8, 80, 90, 100, 255));
    CHECK(RillRaylib_DrawCube(0, 0, 0, 2, 2, 2, 200, 80, 40, 255));
    CHECK(RillRaylib_EndMode3D());
    CHECK(RillRaylib_DrawRectangle(8, 8, 48, 32, 0, 255, 0, 255));
    CHECK(RillRaylib_DrawText("Rill", 8, 60, 20, 255, 255, 255, 255));
    CHECK(RillRaylib_Capture(path));
    CHECK(RillRaylib_EndDrawing());
    CHECK(RillRaylib_CloseWindow());

    Image image = LoadImage(path);
    CHECK(IsImageValid(image));
    CHECK(image.width >= 320 && image.height >= 240);
    Color overlay = GetImageColor(image, 12 * image.width / 320, 12 * image.height / 240);
    Color center = GetImageColor(image, image.width / 2, image.height / 2);
    CHECK(overlay.r == 0 && overlay.g == 255 && overlay.b == 0);
    CHECK(center.r > 150 && center.g < 120 && center.b < 80);
    UnloadImage(image);
    // Raylib retains config flags after close. A visible reopen must explicitly
    // remove a previous hidden flag rather than merely OR-ing a zero flag mask.
    CHECK(setenv("RILL_RAYLIB_HIDDEN", "1", 1) == 0);
    CHECK(RillRaylib_InitWindow(64, 64, "Rill hidden reopen test"));
    CHECK(IsWindowHidden());
    CHECK(RillRaylib_CloseWindow());
    CHECK(unsetenv("RILL_RAYLIB_HIDDEN") == 0);
    CHECK(RillRaylib_InitWindow(64, 64, "Rill visible reopen test"));
    CHECK(!IsWindowHidden());
    CHECK(RillRaylib_CloseWindow());
    puts("Raylib SDK hardware checks passed");
    return 0;
}

int main(int argc, char **argv) {
    if (closed_state()) return 1;
    if (argc == 3 && strcmp(argv[1], "--window") == 0) return window_test(argv[2]);
    if (argc != 1) {
        fprintf(stderr, "usage: raylib_sdk_tests [--window <capture.png>]\n");
        return 1;
    }
    puts("Raylib SDK closed-state checks passed without creating a GPU window");
    return 0;
}
