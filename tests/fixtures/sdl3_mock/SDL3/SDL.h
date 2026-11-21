#ifndef RILL_TEST_SDL3_H
#define RILL_TEST_SDL3_H
#include <stdbool.h>
#include <stdint.h>

typedef struct SDL_Window SDL_Window;
typedef struct SDL_Renderer SDL_Renderer;
#define SDL_INIT_VIDEO UINT32_C(0x20)
#define SDL_EVENT_QUIT INT32_C(0x100)

bool SDL_Init(uint32_t flags);
SDL_Window *SDL_CreateWindow(const char *title, int32_t w, int32_t h, uint64_t flags);
SDL_Renderer *SDL_CreateRenderer(SDL_Window *window, const char *name);
void SDL_PumpEvents(void);
bool SDL_HasEvent(uint32_t type);
uint32_t SDL_GetMouseState(float *x, float *y);
bool SDL_SetRenderDrawColor(SDL_Renderer *renderer, uint8_t r, uint8_t g, uint8_t b, uint8_t a);
bool SDL_RenderClear(SDL_Renderer *renderer);
bool SDL_RenderLine(SDL_Renderer *renderer, float x1, float y1, float x2, float y2);
bool SDL_RenderPresent(SDL_Renderer *renderer);
void SDL_Delay(uint32_t ms);
void SDL_DestroyRenderer(SDL_Renderer *renderer);
void SDL_DestroyWindow(SDL_Window *window);
void SDL_Quit(void);
#endif
