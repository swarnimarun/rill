#include <raylib.h>
#include <stdio.h>
#include <string.h>
static int frame;
void InitWindow(int w, int h, const char *title) { (void)w; (void)h; (void)title; }
void CloseWindow(void) {}
void SetTargetFPS(int fps) { (void)fps; }
double GetTime(void) { return frame == 4 ? 31.0 : (double)frame; }
bool WindowShouldClose(void) { return ++frame > 4; }
bool IsMouseButtonPressed(int button) { (void)button; return true; }
int GetMouseX(void) { return frame == 2 ? 20 : 250; }
int GetMouseY(void) { return 200; }
void BeginDrawing(void) {}
void EndDrawing(void) {}
void ClearBackground(Color color) { (void)color; }
void DrawRectangle(int x, int y, int width, int height, Color color) { (void)x; (void)y; (void)width; (void)height; (void)color; }
void DrawText(const char *text, int x, int y, int size, Color color) { (void)x; (void)y; (void)size; (void)color; if (strncmp(text, "Score:", 6) == 0) puts(text); }
