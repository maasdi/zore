#include "mbedtls/constant_time.h"
#include "mbedtls/ctr_drbg.h"
#include "mbedtls/md.h"
#include "mbedtls/sha256.h"
#include <assert.h>
#include <string.h>

static int fail_digest;
static int fail_hmac;
static int fail_algorithm;
static int fail_random_chunk;
static int random_calls;
static int equal_calls;
static int freed_states;

static int test_sha256(const unsigned char *input, size_t length,
                       unsigned char *output, int is224) {
    if (fail_digest) { memset(output, 0xaa, 32); return -1; }
    return mbedtls_sha256(input, length, output, is224);
}
static const mbedtls_md_info_t *test_algorithm(mbedtls_md_type_t type) {
    return fail_algorithm ? NULL : mbedtls_md_info_from_type(type);
}
static int test_hmac(const mbedtls_md_info_t *info, const unsigned char *key,
                     size_t key_length, const unsigned char *input,
                     size_t length, unsigned char *output) {
    if (fail_hmac) { memset(output, 0xaa, 32); return -1; }
    return mbedtls_md_hmac(info, key, key_length, input, length, output);
}
static int test_random(void *state, unsigned char *output, size_t length) {
    random_calls++;
    if (fail_random_chunk && random_calls == 2) {
        memset(output, 0xaa, length);
        return -1;
    }
    return mbedtls_ctr_drbg_random(state, output, length);
}
static void test_free(mbedtls_ctr_drbg_context *state) {
    freed_states++;
    mbedtls_ctr_drbg_free(state);
}
static int test_equal(const void *left, const void *right, size_t length) {
    equal_calls++;
    return mbedtls_ct_memcmp(left, right, length);
}
#define mbedtls_sha256 test_sha256
#define mbedtls_md_info_from_type test_algorithm
#define mbedtls_md_hmac test_hmac
#define mbedtls_ctr_drbg_random test_random
#define mbedtls_ctr_drbg_free test_free
#define mbedtls_ct_memcmp test_equal
#include "crypto.c"

static int entropy_failure(void *context, unsigned char *output, size_t length) {
    (void)context;
    memset(output, 0xbb, length);
    return -1;
}
static void assert_zero(const unsigned char *bytes, size_t length) {
    for (size_t i = 0; i < length; i++) assert(bytes[i] == 0);
}
int main(void) {
    unsigned char bytes[MBEDTLS_CTR_DRBG_MAX_REQUEST + 1];
    memset(bytes, 0xcc, sizeof(bytes));
    assert(random_with_entropy(bytes, sizeof(bytes), entropy_failure, NULL) != 0);
    assert_zero(bytes, sizeof(bytes));
    assert(freed_states == 1 && random_calls == 0);
    fail_random_chunk = 1;
    assert(zore_crypto_random(bytes, sizeof(bytes)) != 0);
    assert(random_calls == 2 && freed_states == 2);
    assert_zero(bytes, sizeof(bytes));
    assert(zore_crypto_random(NULL, 0) == 0);
    assert(zore_crypto_random(NULL, 1) != 0);
    assert(zore_crypto_random(bytes, 1048577) != 0);
    assert(zore_crypto_random(bytes, (size_t)-1) != 0);
    assert(random_calls == 2 && freed_states == 2);
    unsigned char digest[32];
    assert(zore_crypto_sha256(NULL, 0, digest) == 0);
    assert(zore_crypto_sha256(NULL, 1, digest) != 0);
    assert(zore_crypto_sha256(bytes, 1, NULL) != 0);
    fail_digest = 1;
    assert(zore_crypto_sha256(bytes, 1, digest) != 0);
    assert_zero(digest, sizeof(digest));
    assert(zore_crypto_hmac_sha256(NULL, 0, NULL, 0, digest) == 0);
    assert(zore_crypto_hmac_sha256(NULL, 1, bytes, 1, digest) != 0);
    assert(zore_crypto_hmac_sha256(bytes, 1, NULL, 1, digest) != 0);
    fail_hmac = 1;
    assert(zore_crypto_hmac_sha256(bytes, 1, bytes, 1, digest) != 0);
    assert_zero(digest, sizeof(digest));
    fail_hmac = 0;
    fail_algorithm = 1;
    memset(digest, 0xcc, sizeof(digest));
    assert(zore_crypto_hmac_sha256(bytes, 1, bytes, 1, digest) != 0);
    assert_zero(digest, sizeof(digest));
    assert(zore_crypto_equal(NULL, 0, NULL, 0) == 1);
    assert(zore_crypto_equal(bytes, 1, bytes, 0) == 0);
    assert(equal_calls == 0);
    unsigned char left[32] = {0};
    unsigned char right[32] = {0};
    assert(zore_crypto_equal(left, 32, right, 32) == 1);
    for (size_t i = 0; i < 32; i++) {
        right[i] = 1;
        assert(zore_crypto_equal(left, 32, right, 32) == 0);
        right[i] = 0;
    }
    assert(equal_calls == 33);
    return 0;
}
