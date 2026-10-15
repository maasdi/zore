#include "adler32.h"
#include <stdlib.h>

typedef struct {
    uint32_t low;
    uint32_t high;
} Adler32;

void *zore_adler32_create(void) {
    Adler32 *state = malloc(sizeof(*state));
    if (state != NULL) {
        state->low = 1;
        state->high = 0;
    }
    return state;
}

int32_t zore_adler32_write(void *opaque, const uint8_t *data, uint64_t length) {
    if (opaque == NULL || length > 65536 || (length != 0 && data == NULL)) {
        return 1;
    }
    Adler32 *state = opaque;
    for (uint64_t index = 0; index < length; index++) {
        state->low = (state->low + data[index]) % 65521;
        state->high = (state->high + state->low) % 65521;
    }
    return 0;
}

uint32_t zore_adler32_sum(const void *opaque) {
    const Adler32 *state = opaque;
    return state == NULL ? 0 : (state->high << 16) | state->low;
}

void zore_adler32_free(void *state) {
    free(state);
}
