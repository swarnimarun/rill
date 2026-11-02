#include <SDL3/SDL.h>
#include <stdint.h>

static SDL_Window *window;
static SDL_Renderer *renderer;

int32_t game_init(void) {
    if (!SDL_Init(SDL_INIT_VIDEO)) {
        SDL_Log("SDL_Init: %s", SDL_GetError());
        return 0;
    }
    if (!SDL_CreateWindowAndRenderer("Rill collector", 640, 480, 0, &window, &renderer)) {
        SDL_Log("SDL_CreateWindowAndRenderer: %s", SDL_GetError());
        SDL_Quit();
        return 0;
    }
    return 1;
}

int32_t game_poll(void) {
    SDL_Event event;
    while (SDL_PollEvent(&event)) {
        if (event.type == SDL_EVENT_QUIT) return 0;
        if (event.type == SDL_EVENT_KEY_DOWN && event.key.scancode == SDL_SCANCODE_ESCAPE) return 0;
    }
    return 1;
}

int32_t game_left(void) { return SDL_GetKeyboardState(NULL)[SDL_SCANCODE_LEFT] ? 1 : 0; }
int32_t game_right(void) { return SDL_GetKeyboardState(NULL)[SDL_SCANCODE_RIGHT] ? 1 : 0; }
int32_t game_up(void) { return SDL_GetKeyboardState(NULL)[SDL_SCANCODE_UP] ? 1 : 0; }
int32_t game_down(void) { return SDL_GetKeyboardState(NULL)[SDL_SCANCODE_DOWN] ? 1 : 0; }
void game_draw(int32_t x, int32_t y, int32_t target_x, int32_t target_y, int32_t score) {
    SDL_FRect player = {(float)x, (float)y, 20.0f, 20.0f};
    SDL_FRect target = {(float)target_x, (float)target_y, 20.0f, 20.0f};
    char title[64];
    SDL_snprintf(title, sizeof title, "Rill collector | score: %d", (int)score);
    SDL_SetWindowTitle(window, title);
    SDL_SetRenderDrawColor(renderer, 18, 22, 32, 255);
    SDL_RenderClear(renderer);
    SDL_SetRenderDrawColor(renderer, 242, 190, 58, 255);
    SDL_RenderFillRect(renderer, &target);
    SDL_SetRenderDrawColor(renderer, 80, 165, 250, 255);
    SDL_RenderFillRect(renderer, &player);
    SDL_RenderPresent(renderer);
    SDL_Delay(16);
}
void game_close(void) {
    SDL_DestroyRenderer(renderer);
    SDL_DestroyWindow(window);
    SDL_Quit();
}
