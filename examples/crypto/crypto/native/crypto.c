#include "crypto.h"

#include "mbedtls/constant_time.h"
#include "mbedtls/ctr_drbg.h"
#include "mbedtls/entropy.h"
#include "mbedtls/md.h"
#include "mbedtls/platform_util.h"
#include "mbedtls/sha256.h"

static int32_t random_with_entropy(uint8_t *output, size_t length,
                                  int (*entropy)(void *, unsigned char *, size_t),
                                  void *context) {
    if (length > 1048576 || (length != 0 && output == NULL)) return -1;
    if (length == 0) return 0;
    mbedtls_ctr_drbg_context drbg;
    mbedtls_ctr_drbg_init(&drbg);
    const unsigned char personalization[] = "zore-crypto-v1";
    int status = mbedtls_ctr_drbg_seed(&drbg, entropy, context,
                                     personalization, sizeof(personalization) - 1);
    size_t offset = 0;
    while (status == 0 && offset < length) {
        size_t count = length - offset;
        if (count > MBEDTLS_CTR_DRBG_MAX_REQUEST) count = MBEDTLS_CTR_DRBG_MAX_REQUEST;
        status = mbedtls_ctr_drbg_random(&drbg, output + offset, count);
        offset += count;
    }
    mbedtls_ctr_drbg_free(&drbg);
    if (status != 0) mbedtls_platform_zeroize(output, length);
    return status;
}

int32_t zore_crypto_random(uint8_t *output, size_t length) {
    if (length > 1048576 || (length != 0 && output == NULL)) return -1;
    if (length == 0) return 0;
    mbedtls_entropy_context entropy;
    mbedtls_entropy_init(&entropy);
    int32_t status = random_with_entropy(output, length, mbedtls_entropy_func, &entropy);
    mbedtls_entropy_free(&entropy);
    return status;
}

int32_t zore_crypto_sha256(const uint8_t *data, size_t length, uint8_t output[32]) {
    if (output == NULL || (length != 0 && data == NULL)) return -1;
    static const uint8_t empty = 0;
    int status = mbedtls_sha256(length == 0 ? &empty : data, length, output, 0);
    if (status != 0) mbedtls_platform_zeroize(output, 32);
    return status;
}

int32_t zore_crypto_hmac_sha256(const uint8_t *key, size_t key_length,
                               const uint8_t *data, size_t length, uint8_t output[32]) {
    if (output == NULL || (length != 0 && data == NULL) ||
        (key_length != 0 && key == NULL)) return -1;
    static const uint8_t empty = 0;
    const mbedtls_md_info_t *algorithm = mbedtls_md_info_from_type(MBEDTLS_MD_SHA256);
    if (algorithm == NULL) {
        mbedtls_platform_zeroize(output, 32);
        return -1;
    }
    int status = mbedtls_md_hmac(algorithm, key_length == 0 ? &empty : key, key_length,
                               length == 0 ? &empty : data, length, output);
    if (status != 0) mbedtls_platform_zeroize(output, 32);
    return status;
}

int32_t zore_crypto_equal(const uint8_t *left, size_t left_length,
                          const uint8_t *right, size_t right_length) {
    if (left_length != right_length) return 0;
    if (left_length == 0) return 1;
    if (left == NULL || right == NULL) return 0;
    return mbedtls_ct_memcmp(left, right, left_length) == 0;
}
