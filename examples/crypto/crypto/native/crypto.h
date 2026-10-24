#ifndef ZORE_CRYPTO_H
#define ZORE_CRYPTO_H

#include <stddef.h>
#include <stdint.h>

int32_t zore_crypto_random(uint8_t *output, size_t length);
int32_t zore_crypto_sha256(const uint8_t *data, size_t length, uint8_t output[32]);
int32_t zore_crypto_hmac_sha256(const uint8_t *key, size_t key_length,
                               const uint8_t *data, size_t length, uint8_t output[32]);
int32_t zore_crypto_equal(const uint8_t *left, size_t left_length,
                          const uint8_t *right, size_t right_length);

#endif
