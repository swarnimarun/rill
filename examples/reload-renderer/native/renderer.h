#ifndef RILL_RELOAD_RENDERER_H
#define RILL_RELOAD_RENDERER_H
#include <stdint.h>

/* This example's fixed C ABI, version 1. All calls are synchronous on the main
   thread. The adapter and its Raylib dependency remain loaded for the complete
   window lifetime. No pointer or callback enters a Rill reload generation.
   OPEN: no arguments. FRAME: [x, red]. CLOSE: no arguments.
   Return 0 on success, 1 for a close request, or -1 for invalid state/failure. */
enum { RILL_RENDERER_OPEN = 1, RILL_RENDERER_FRAME = 2, RILL_RENDERER_CLOSE = 3 };
int32_t rill_renderer_v1(uint32_t operation, const int32_t *arguments, uint32_t length);
#endif
