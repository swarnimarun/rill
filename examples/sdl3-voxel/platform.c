// SDL aggregate/event adapter. All world, physics, lighting and geometry are Rill.
#include "platform.h"
#include <SDL3/SDL.h>
#include <stdlib.h>
#include <string.h>

static SDL_Window *window;
static SDL_Renderer *renderer;
static int32_t actions[5];
static int32_t mouse_x, mouse_y, buttons;
static Uint64 last_tick;
static unsigned frames;
static bool smoke;

void vx_close(void) {
    if (renderer) SDL_DestroyRenderer(renderer);
    if (window) SDL_DestroyWindow(window);
    renderer = NULL;
    window = NULL;
    SDL_Quit();
}

static bool checked(bool success) {
    if (!success) SDL_Log("voxel SDL failure: %s", SDL_GetError());
    return success;
}

bool vx_open(void) {
    const char *mode = getenv("RILL_VOXEL_SMOKE");
    smoke = mode && strcmp(mode, "1") == 0;
    frames = 0;
    if (!checked(SDL_Init(SDL_INIT_VIDEO))) {
        vx_close();
        return false;
    }
    window = SDL_CreateWindow("Rill | voxel destruction lab", 960, 720, 0);
    if (!window) { checked(false); vx_close(); return false; }
    renderer = SDL_CreateRenderer(window, "software");
    if (!renderer) { checked(false); vx_close(); return false; }
    last_tick = SDL_GetTicks();
    return true;
}

bool vx_poll(void) {
    memset(actions, 0, sizeof actions);
    bool running = true;
    SDL_MouseButtonFlags pressed = 0;
    float click_x = 0, click_y = 0;
    SDL_Event event;
    while (SDL_PollEvent(&event)) {
        if (event.type == SDL_EVENT_QUIT) running = false;
        // Retain a click even if its release arrives during a lighting rebuild.
        if (event.type == SDL_EVENT_MOUSE_BUTTON_DOWN &&
            (event.button.button == SDL_BUTTON_LEFT || event.button.button == SDL_BUTTON_RIGHT)) {
            pressed |= SDL_BUTTON_MASK(event.button.button);
            click_x = event.button.x;
            click_y = event.button.y;
        }
        if (event.type == SDL_EVENT_KEY_DOWN && !event.key.repeat) {
            switch (event.key.scancode) {
            case SDL_SCANCODE_ESCAPE: running = false; break;
            case SDL_SCANCODE_R: actions[0] = 1; break;
            case SDL_SCANCODE_G: actions[1] = 1; break;
            case SDL_SCANCODE_SPACE: actions[2] = 1; break;
            case SDL_SCANCODE_TAB: actions[3] = 1; break;
            case SDL_SCANCODE_X: actions[4] = 1; break;
            default: break;
            }
        }
    }
    float x = 0, y = 0;
    SDL_GetMouseState(&x, &y);
    if (pressed) { x = click_x; y = click_y; }
    mouse_x = (int32_t)x;
    mouse_y = (int32_t)y;
    buttons = ((pressed & SDL_BUTTON_LMASK) ? 1 : 0) +
              ((pressed & SDL_BUTTON_RMASK) ? 2 : 0);
    return running && (!smoke || frames < 8);
}

int32_t vx_key(int32_t action) { return action >= 0 && action < 5 ? actions[action] : 0; }
int32_t vx_buttons(void) { return buttons; }
int32_t vx_mouse_x(void) { return mouse_x; }
int32_t vx_mouse_y(void) { return mouse_y; }
bool vx_smoke(void) { return smoke; }
int32_t vx_elapsed(void) {
    Uint64 now = SDL_GetTicks();
    Uint64 delta = now - last_tick;
    last_tick = now;
    return delta > 250 ? 250 : (int32_t)delta;
}
bool vx_color(int32_t r, int32_t g, int32_t b) {
    if (r < 0 || r > 255 || g < 0 || g > 255 || b < 0 || b > 255) return false;
    return checked(SDL_SetRenderDrawColor(renderer, (Uint8)r, (Uint8)g, (Uint8)b, 255));
}
bool vx_begin(void) { return vx_color(13, 19, 28) && checked(SDL_RenderClear(renderer)); }
bool vx_line(int32_t x1, int32_t y1, int32_t x2, int32_t y2) {
    return checked(SDL_RenderLine(renderer, (float)x1, (float)y1, (float)x2, (float)y2));
}
bool vx_text(int32_t x, int32_t y, const char *text) {
    return checked(SDL_RenderDebugText(renderer, (float)x, (float)y, text));
}
bool vx_hud(int32_t removed, int32_t moved, int32_t view, bool paused) {
    char text[192];
    SDL_snprintf(text, sizeof text, "removed %d   falling %d   %s   %s",
                 (int)removed, (int)moved,
                 view == 0 ? "distance-field GI" : view == 1 ? "unlit materials" : "signed-distance slice y=3",
                 paused ? "PAUSED" : "physics 10 Hz");
    return vx_color(155, 180, 193) && vx_text(32, 655, text);
}
bool vx_present(void) {
    // Optional real-framebuffer capture, useful with the bounded smoke mode.
    const char *capture = getenv("RILL_VOXEL_CAPTURE");
    if (capture && frames == 7) {
        SDL_Surface *surface = SDL_RenderReadPixels(renderer, NULL);
        if (!surface) return checked(false);
        bool saved = SDL_SaveBMP(surface, capture);
        SDL_DestroySurface(surface);
        if (!checked(saved)) return false;
    }
    if (!checked(SDL_RenderPresent(renderer))) return false;
    ++frames;
    if (!smoke) SDL_Delay(16);
    return true;
}
