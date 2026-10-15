#ifndef ZORE_ADLER32_H
#define ZORE_ADLER32_H

#include <stdint.h>

void *zore_adler32_create(void);
int32_t zore_adler32_write(void *state, const uint8_t *data, uint64_t length);
uint32_t zore_adler32_sum(const void *state);
void zore_adler32_free(void *state);

#endif
