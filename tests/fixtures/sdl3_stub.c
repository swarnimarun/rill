#include <assert.h>
#include <stdint.h>

static int frames;
static int draws;

int32_t game_init(void) { return 1; }
int32_t game_poll(void) { assert(frames < 81); return ++frames <= 80; }
int32_t game_left(void) { return frames > 1; }
int32_t game_right(void) { return frames == 1; }
int32_t game_up(void) { return frames <= 30 || frames >= 49; }
int32_t game_down(void) { return 0; }
void game_draw(int32_t x, int32_t y, int32_t target_x, int32_t target_y, int32_t score) {
    int32_t expected_x = 304 - 4 * (frames - 1);
    int32_t expected_y = frames <= 30 ? 220 - 4 * frames : 100;
    if (frames >= 49) expected_y -= 4 * (frames - 48);
    if (expected_x < 0) expected_x = 0;
    if (expected_y < 0) expected_y = 0;
    assert(x == expected_x && y == expected_y);
    assert(score == (frames >= 48));
    assert(target_x == (frames >= 48 ? 273 : 100));
    assert(target_y == (frames >= 48 ? 237 : 100));
    assert(frames == ++draws);
}
void game_close(void) { assert(frames == 81 && draws == 80); }
