/* Capture builds redirect game calls here; this file calls the real SDL. */
#include <SDL3/SDL.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>

bool rill_capture_present(SDL_Renderer *renderer) {
    static unsigned frame;
    static unsigned capture_frame;
    if (capture_frame == 0) {
        const char *configured = getenv("RILL_CAPTURE_FRAME");
        char *end = NULL;
        errno = 0;
        unsigned long value = configured ? strtoul(configured, &end, 10) : 40;
        if (errno || (configured && (!*configured || !end || *end)) ||
            value < 1 || value > 900) {
            fprintf(stderr, "RILL_CAPTURE_FRAME must be an integer from 1 to 900\n");
            exit(97);
        }
        capture_frame = (unsigned)value;
    }
    ++frame;
    if (frame == capture_frame) {
        const char *path = getenv("RILL_CAPTURE_BMP");
        SDL_Surface *surface = SDL_RenderReadPixels(renderer, NULL);
        if (!surface || !path || !*path || !SDL_SaveBMP(surface, path)) {
            fprintf(stderr, "SDL framebuffer capture failed: %s\n", SDL_GetError());
            SDL_DestroySurface(surface);
            exit(97);
        }
        fprintf(stderr,
                "captured frame %u: %dx%d, SDL %d, renderer %s, video %s\n",
                frame, surface->w, surface->h, SDL_GetVersion(),
                SDL_GetRendererName(renderer), SDL_GetCurrentVideoDriver());
        SDL_DestroySurface(surface);
    }
    bool result = SDL_RenderPresent(renderer);
    if (frame == capture_frame) {
        /* Both original game loops handle this ordinary SDL quit event. */
        SDL_Event event = {0};
        event.type = SDL_EVENT_QUIT;
        if (!SDL_PushEvent(&event)) {
            fprintf(stderr, "SDL quit event failed: %s\n", SDL_GetError());
            exit(98);
        }
    }
    return result;
}
