#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <raylib.h>

/* This shim stores the clock origin and adapts raylib's C ABI. */
static double start_time;

void click_open(const char *title) {
    InitWindow(640, 480, title);
    SetTargetFPS(60);
    start_time = GetTime();
}
int32_t click_should_close(void) { return WindowShouldClose() ? 1 : 0; }
int32_t click_pressed(void) { return IsMouseButtonPressed(MOUSE_BUTTON_LEFT) ? 1 : 0; }
int32_t click_mouse_x(void) { return (int32_t)GetMouseX(); }
int32_t click_mouse_y(void) { return (int32_t)GetMouseY(); }
int32_t click_elapsed(void) {
    double elapsed = GetTime() - start_time;
    return elapsed >= 3600.0 ? 3600 : elapsed <= 0.0 ? 0 : (int32_t)elapsed;
}
void click_draw(int32_t score, int32_t seconds_left) {
    char status[96];
    snprintf(status, sizeof status, "Score: %" PRId32 "   Time: %" PRId32, score, seconds_left);
    BeginDrawing();
    ClearBackground(RAYWHITE);
    DrawText(status, 24, 24, 24, DARKGRAY);
    DrawRectangle(220, 160, 200, 200, RED);
    DrawText(seconds_left ? "Click the red square" : "Time up! Close the window.", 24, 420, 20, DARKGRAY);
    EndDrawing();
}
void click_close(void) { CloseWindow(); }
