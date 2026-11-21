#include "scene_window.h"
#include <SDL3/SDL.h>
#include <stdio.h>

enum { columns = 20, rows = 8, cell_size = 32 };
static SDL_Window *window;
static SDL_Renderer *renderer;

static bool checked(bool success) {
    if (!success) fprintf(stderr, "scene window: %s\n", SDL_GetError());
    return success;
}

bool scene_window_open(void) {
    if (!checked(SDL_Init(SDL_INIT_VIDEO))) return false;
    window = SDL_CreateWindow("Rill Scene", columns * cell_size, rows * cell_size, 0);
    if (!checked(window != NULL)) return false;
    renderer = SDL_CreateRenderer(window, "software");
    return checked(renderer != NULL);
}

void scene_window_close(void) {
    if (renderer) SDL_DestroyRenderer(renderer);
    if (window) SDL_DestroyWindow(window);
    renderer = NULL;
    window = NULL;
    SDL_Quit();
}

int32_t scene_window_event(void) {
    SDL_Event event;
    while (SDL_WaitEvent(&event)) {
        switch (event.type) {
        case SDL_EVENT_QUIT:
        case SDL_EVENT_WINDOW_CLOSE_REQUESTED:
            return 'q';
        case SDL_EVENT_WINDOW_EXPOSED:
            return 0;
        case SDL_EVENT_KEY_DOWN:
            switch (event.key.scancode) {
            case SDL_SCANCODE_ESCAPE: case SDL_SCANCODE_Q: return 'q';
            case SDL_SCANCODE_LEFT:   case SDL_SCANCODE_H: return 'h';
            case SDL_SCANCODE_RIGHT:  case SDL_SCANCODE_L: return 'l';
            case SDL_SCANCODE_UP:     case SDL_SCANCODE_K: return 'k';
            case SDL_SCANCODE_DOWN:   case SDL_SCANCODE_J: return 'j';
            default: break;
            }
            break;
        default: break;
        }
    }
    checked(false);
    return -1;
}

bool scene_window_begin(void) {
    return checked(SDL_SetRenderDrawColor(renderer, 22, 28, 34, 255) && SDL_RenderClear(renderer));
}

bool scene_window_cell(int32_t index, int32_t glyph) {
    if (index < 0 || index >= columns * rows) return false;
    SDL_Color color;
    switch (glyph) {
    case '@': color = (SDL_Color){132, 202, 232, 255}; break;
    case 'o': color = (SDL_Color){100, 172, 115, 255}; break;
    case '*': color = (SDL_Color){250, 195, 72, 255}; break;
    default: color = (SDL_Color){180, 180, 180, 255}; break;
    }
    const SDL_FRect rect = {
        (float)(index % columns * cell_size + 2),
        (float)(index / columns * cell_size + 2),
        cell_size - 4, cell_size - 4
    };
    return checked(SDL_SetRenderDrawColor(renderer, color.r, color.g, color.b, color.a)
        && SDL_RenderFillRect(renderer, &rect));
}

bool scene_window_present(int32_t score) {
    char title[96];
    SDL_snprintf(title, sizeof title, "Rill Scene | score %d | arrows or hjkl | Q to quit", (int)score);
    return checked(SDL_SetWindowTitle(window, title) && SDL_RenderPresent(renderer));
}
