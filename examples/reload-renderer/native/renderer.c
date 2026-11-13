#include "renderer.h"
#include <rill/raylib.h>
#include <stdbool.h>
#include <stdlib.h>

static bool opened;

int32_t rill_renderer_v1(uint32_t operation, const int32_t *arguments, uint32_t length) {
    if (operation == RILL_RENDERER_OPEN) {
        if (opened || length != 0) return -1;
        if (!RillRaylib_InitWindow(640, 360, "Rill live functions, persistent window")) return -1;
        opened = true;
        if (!RillRaylib_SetTargetFPS(60)) {
            RillRaylib_CloseWindow();
            opened = false;
            return -1;
        }
        return 0;
    }
    if (operation == RILL_RENDERER_CLOSE) {
        if (!opened || length != 0) return -1;
        bool ok = RillRaylib_CloseWindow();
        opened = false;
        return ok ? 0 : -1;
    }
    if (operation != RILL_RENDERER_FRAME || !opened || length != 2 || !arguments) return -1;
    if (arguments[0] < 0 || arguments[0] >= 500 || arguments[1] < 0 || arguments[1] > 255) return -1;
    int32_t close = RillRaylib_WindowShouldClose();
    if (close != 0) return close > 0 ? 1 : -1;
    if (!RillRaylib_BeginDrawing()) return -1;
    bool ok = RillRaylib_ClearBackground(20, 24, 32, 255);
    if (!RillRaylib_DrawRectangle(40 + arguments[0], 150, 60, 60,
            (uint8_t)arguments[1], 140, 220, 255)) ok = false;
    if (!RillRaylib_DrawText("Edit logic.rill: update reloads, color stays registered", 18, 22, 18,
            230, 235, 245, 255)) ok = false;
    const char *capture = getenv("RILL_RELOAD_CAPTURE");
    if (capture && *capture && !RillRaylib_Capture(capture)) ok = false;
    /* Balance the frame even after a failed drawing operation. */
    if (!RillRaylib_EndDrawing()) ok = false;
    return ok ? 0 : -1;
}
