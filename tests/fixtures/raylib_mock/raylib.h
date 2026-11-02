#ifndef RILL_TEST_RAYLIB_H
#define RILL_TEST_RAYLIB_H
#include <stdbool.h>
typedef struct Color { unsigned char r, g, b, a; } Color;
#define RAYWHITE ((Color){245, 245, 245, 255})
#define DARKGRAY ((Color){80, 80, 80, 255})
#define RED ((Color){230, 41, 55, 255})
#define MOUSE_BUTTON_LEFT 0
void InitWindow(int width, int height, const char *title);
void CloseWindow(void);
void SetTargetFPS(int fps);
double GetTime(void);
bool WindowShouldClose(void);
bool IsMouseButtonPressed(int button);
int GetMouseX(void);
int GetMouseY(void);
void BeginDrawing(void);
void EndDrawing(void);
void ClearBackground(Color color);
void DrawRectangle(int x, int y, int width, int height, Color color);
void DrawText(const char *text, int x, int y, int size, Color color);
#endif
