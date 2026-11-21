#include "update.h"
#include <inttypes.h>
#include <stdio.h>

int main(void) {
    int32_t state = rill_update(40, 2);
    printf("%" PRId32 "\n", state);
    return state == 42 ? 0 : 1;
}
