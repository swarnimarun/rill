#include <SDL3/SDL.h>
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct SDL_Window { bool live; };
struct SDL_Renderer { bool live; };
static struct SDL_Window window;
static struct SDL_Renderer renderer;
static const char *scenario;
static bool initialized, failed, quitting;
static unsigned frame, colors, lines;

static bool selected(const char *name) {
    return strcmp(scenario, name) == 0;
}

static bool render_result(const char *name, SDL_Renderer *value) {
    assert(value == &renderer && renderer.live && window.live && initialized);
    assert(!failed && !quitting);
    if (selected(name)) {
        failed = true;
        fputs("render-failed\n", stderr);
        return false;
    }
    return true;
}

bool SDL_Init(uint32_t flags) {
    assert(flags == SDL_INIT_VIDEO);
    scenario = getenv("RILL_SDL_TEST_CASE");
    assert(scenario);
    fputs("init\n", stderr);
    initialized = !selected("init");
    return initialized;
}

SDL_Window *SDL_CreateWindow(const char *title, int32_t w, int32_t h, uint64_t flags) {
    assert(initialized && title && w == 640 && h == 400 && flags == 0);
    fputs("window\n", stderr);
    if (selected("window")) return NULL;
    window.live = true;
    return &window;
}

SDL_Renderer *SDL_CreateRenderer(SDL_Window *value, const char *name) {
    assert(value == &window && window.live && strcmp(name, "software") == 0);
    fputs("renderer\n", stderr);
    if (selected("renderer")) return NULL;
    renderer.live = true;
    return &renderer;
}

void SDL_PumpEvents(void) {
    assert(renderer.live && !failed && !quitting);
    ++frame;
    colors = lines = 0;
    assert(frame <= 40);
}

bool SDL_HasEvent(uint32_t type) {
    assert(type == SDL_EVENT_QUIT && renderer.live);
    quitting = selected("quit") && frame == 3;
    return quitting;
}

uint32_t SDL_GetMouseState(float *x, float *y) {
    assert(!x && !y && !quitting && renderer.live);
    if (selected("miss")) return frame == 2;
    return frame >= 36 && frame <= 38;
}

bool SDL_SetRenderDrawColor(SDL_Renderer *value, uint8_t r, uint8_t g, uint8_t b, uint8_t a) {
    static const uint8_t expected[3][3] = {{20, 24, 32}, {70, 210, 140}, {250, 190, 60}};
    static const char *names[] = {"color-1", "color-2", "color-3"};
    assert(colors < 3);
    assert(r == expected[colors][0] && g == expected[colors][1] && b == expected[colors][2] && a == 255);
    return render_result(names[colors++], value);
}

bool SDL_RenderClear(SDL_Renderer *value) {
    assert(colors == 1);
    return render_result("clear", value);
}

bool SDL_RenderLine(SDL_Renderer *value, float x1, float y1, float x2, float y2) {
    static const char *names[] = {"line-1", "line-2", "line-3", "line-4"};
    assert(lines < 4);
    if (lines == 0) assert(x1 == 280 && y1 == 120 && x2 == 280 && y2 == 300);
    if (lines == 1) assert(x1 == 360 && y1 == 120 && x2 == 360 && y2 == 300);
    if (lines == 2) assert(x1 == 40 + ((frame - 1) * 7) % 560 && y1 == 150 && x2 == x1 && y2 == 270);
    if (lines == 3) assert(frame >= 36 && x1 == 40 && y1 == 350 && x2 == 54 && y2 == 350);
    return render_result(names[lines++], value);
}

bool SDL_RenderPresent(SDL_Renderer *value) {
    assert(colors == 3 && lines >= 3);
    return render_result("present", value);
}

void SDL_Delay(uint32_t ms) {
    assert(ms == 16 && renderer.live && !failed && !quitting);
}

void SDL_DestroyRenderer(SDL_Renderer *value) {
    assert(value == &renderer && renderer.live && window.live);
    renderer.live = false;
    fputs("destroy-renderer\n", stderr);
}

void SDL_DestroyWindow(SDL_Window *value) {
    assert(value == &window && window.live && !renderer.live);
    window.live = false;
    fputs("destroy-window\n", stderr);
}

void SDL_Quit(void) {
    assert(!window.live && !renderer.live);
    initialized = false;
    fputs("quit\n", stderr);
}
