#include "tls.h"
#include "mbedtls/ssl.h"
#include "mbedtls/entropy.h"
#include "mbedtls/ctr_drbg.h"
#include "mbedtls/error.h"
#include "mbedtls/platform_util.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

#define CAPACITY 65536
#define ERROR_CAPACITY 512

typedef struct {
    int server;
    mbedtls_x509_crt certificates;
    mbedtls_pk_context key;
    unsigned char *cert_pem;
    size_t cert_len;
    unsigned char *key_pem;
    size_t key_len;
} Identity;

typedef struct {
    mbedtls_ssl_context ssl;
    mbedtls_ssl_config config;
    mbedtls_entropy_context entropy;
    mbedtls_ctr_drbg_context random;
    unsigned char input[CAPACITY];
    unsigned char output[CAPACITY];
    size_t input_length;
    size_t output_length;
    int server;
    mbedtls_x509_crt certificates;
    mbedtls_pk_context key;
    int status;
    int peer_closed;
    int shutdown_sent;
    int failed;
} Session;

static int failure(int status, char *error) {
    mbedtls_strerror(status, error, ERROR_CAPACITY);
    return -1;
}

static unsigned char *pem(const unsigned char *data, size_t length) {
    if (length == 0 || length > 1024 * 1024 || memchr(data, 0, length) != NULL) return NULL;
    unsigned char *result = malloc(length + 1);
    if (result != NULL) { memcpy(result, data, length); result[length] = 0; }
    return result;
}

void zore_tls_config_free(void *pointer) {
    Identity *identity = pointer;
    if (identity == NULL) return;
    mbedtls_x509_crt_free(&identity->certificates);
    mbedtls_pk_free(&identity->key);
    free(identity->cert_pem);
    if (identity->key_pem != NULL) { mbedtls_platform_zeroize(identity->key_pem, identity->key_len + 1); free(identity->key_pem); }
    free(identity);
}

void *zore_tls_config(int server, const unsigned char *cert, size_t cert_len,
                      const unsigned char *key, size_t key_len, char *error) {
    mbedtls_entropy_context entropy;
    mbedtls_ctr_drbg_context random;
    mbedtls_entropy_init(&entropy);
    mbedtls_ctr_drbg_init(&random);
    Identity *identity = calloc(1, sizeof(*identity));
    unsigned char *cert_pem = pem(cert, cert_len);
    unsigned char *key_pem = server ? pem(key, key_len) : NULL;
    if (identity == NULL || cert_pem == NULL || (server && key_pem == NULL)) {
        snprintf(error, ERROR_CAPACITY, "PEM input must be nonempty, NUL-free and at most 1 MiB, or allocation failed");
        goto fail;
    }
    identity->server = server;
    mbedtls_x509_crt_init(&identity->certificates);
    mbedtls_pk_init(&identity->key);
    int status = mbedtls_x509_crt_parse(&identity->certificates, cert_pem, cert_len + 1);
    if (status != 0) { failure(status, error); goto fail; }
    if (server) {
        status = mbedtls_ctr_drbg_seed(&random, mbedtls_entropy_func, &entropy, (const unsigned char *)"zore-tls-config", 15);
        if (status != 0) { failure(status, error); goto fail; }
        status = mbedtls_pk_parse_key(&identity->key, key_pem, key_len + 1, NULL, 0, mbedtls_ctr_drbg_random, &random);
        if (status != 0) { failure(status, error); goto fail; }
        status = mbedtls_pk_check_pair(&identity->certificates.pk, &identity->key, mbedtls_ctr_drbg_random, &random);
        if (status != 0) { failure(status, error); goto fail; }
    }
    identity->cert_pem = cert_pem;
    identity->cert_len = cert_len;
    identity->key_pem = key_pem;
    identity->key_len = key_len;
    mbedtls_ctr_drbg_free(&random);
    mbedtls_entropy_free(&entropy);
    return identity;
fail:
    mbedtls_ctr_drbg_free(&random);
    mbedtls_entropy_free(&entropy);
    free(cert_pem);
    if (key_pem != NULL) { mbedtls_platform_zeroize(key_pem, key_len + 1); free(key_pem); }
    zore_tls_config_free(identity);
    return NULL;
}

static int receive_bytes(void *pointer, unsigned char *data, size_t length) {
    Session *session = pointer;
    if (session->input_length == 0) return MBEDTLS_ERR_SSL_WANT_READ;
    if (length > session->input_length) length = session->input_length;
    memcpy(data, session->input, length);
    session->input_length -= length;
    memmove(session->input, session->input + length, session->input_length);
    return (int) length;
}

static int send_bytes(void *pointer, const unsigned char *data, size_t length) {
    Session *session = pointer;
    if (length > CAPACITY - session->output_length) return MBEDTLS_ERR_SSL_WANT_WRITE;
    memcpy(session->output + session->output_length, data, length);
    session->output_length += length;
    return (int) length;
}

void zore_tls_session_free(void *pointer) {
    Session *session = pointer;
    if (session == NULL) return;
    mbedtls_ssl_free(&session->ssl);
    mbedtls_ssl_config_free(&session->config);
    mbedtls_ctr_drbg_free(&session->random);
    mbedtls_entropy_free(&session->entropy);
    mbedtls_x509_crt_free(&session->certificates);
    mbedtls_pk_free(&session->key);
    mbedtls_platform_zeroize(session, sizeof(*session));
    free(session);
}

void *zore_tls_session(void *pointer, const char *name, char *error) {
    const Identity *source = pointer;
    Session *session = calloc(1, sizeof(*session));
    if (session == NULL) { snprintf(error, ERROR_CAPACITY, "allocation failed"); return NULL; }
    session->server = source->server;
    mbedtls_ssl_init(&session->ssl);
    mbedtls_ssl_config_init(&session->config);
    mbedtls_entropy_init(&session->entropy);
    mbedtls_ctr_drbg_init(&session->random);
    mbedtls_x509_crt_init(&session->certificates);
    mbedtls_pk_init(&session->key);
    int status = mbedtls_ctr_drbg_seed(&session->random, mbedtls_entropy_func, &session->entropy,
                                     (const unsigned char *)"zore-tls-v1", 11);
    if (status != 0) goto fail;
    /* Per-session copies: shared RSA and X.509 objects would need MBEDTLS_THREADING_C. */
    status = mbedtls_x509_crt_parse(&session->certificates, source->cert_pem, source->cert_len + 1);
    if (status != 0) goto fail;
    if (session->server) {
        status = mbedtls_pk_parse_key(&session->key, source->key_pem, source->key_len + 1, NULL, 0,
                                      mbedtls_ctr_drbg_random, &session->random);
        if (status != 0) goto fail;
    }
    status = mbedtls_ssl_config_defaults(&session->config,
        session->server ? MBEDTLS_SSL_IS_SERVER : MBEDTLS_SSL_IS_CLIENT,
        MBEDTLS_SSL_TRANSPORT_STREAM, MBEDTLS_SSL_PRESET_DEFAULT);
    if (status != 0) goto fail;
    mbedtls_ssl_conf_rng(&session->config, mbedtls_ctr_drbg_random, &session->random);
    if (session->server) {
        status = mbedtls_ssl_conf_own_cert(&session->config, &session->certificates, &session->key);
        if (status != 0) goto fail;
        mbedtls_ssl_conf_authmode(&session->config, MBEDTLS_SSL_VERIFY_NONE);
    } else {
        mbedtls_ssl_conf_ca_chain(&session->config, &session->certificates, NULL);
        mbedtls_ssl_conf_authmode(&session->config, MBEDTLS_SSL_VERIFY_REQUIRED);
    }
    status = mbedtls_ssl_setup(&session->ssl, &session->config);
    if (status != 0) goto fail;
    if (!session->server) {
        status = mbedtls_ssl_set_hostname(&session->ssl, name);
        if (status != 0) goto fail;
    }
    mbedtls_ssl_set_bio(&session->ssl, session, send_bytes, receive_bytes, NULL);
    return session;
fail:
    failure(status, error);
    zore_tls_session_free(session);
    return NULL;
}

static int operation_result(Session *session, int status, char *error) {
    session->status = status == MBEDTLS_ERR_SSL_WANT_READ ? 0 : status == MBEDTLS_ERR_SSL_WANT_WRITE ? -2 : 1;
    if (status == MBEDTLS_ERR_SSL_WANT_READ) return 0;
    if (status == MBEDTLS_ERR_SSL_WANT_WRITE) return -2;
    if (status == MBEDTLS_ERR_SSL_PEER_CLOSE_NOTIFY) { session->peer_closed = 1; snprintf(error, ERROR_CAPACITY, "EOF"); return -1; }
    if (status < 0) {
        session->failed = 1;
        uint32_t flags = mbedtls_ssl_get_verify_result(&session->ssl);
        if (flags != 0) mbedtls_x509_crt_verify_info(error, ERROR_CAPACITY, "certificate verification: ", flags);
        else failure(status, error);
        return -1;
    }
    return status;
}

int zore_tls_handshake(void *pointer, char *error) {
    Session *session = pointer;
    if (session->failed) { snprintf(error, ERROR_CAPACITY, "TLS failed connection"); return -1; }
    if (session->peer_closed) { snprintf(error, ERROR_CAPACITY, "TLS peer closed connection"); return -1; }
    int status = mbedtls_ssl_handshake(&session->ssl);
    if (status == 0) return 1;
    return operation_result(session, status, error);
}

int zore_tls_read(void *pointer, unsigned char *data, size_t length, char *error) {
    Session *session = pointer;
    if (session->failed) { snprintf(error, ERROR_CAPACITY, "TLS failed connection"); return -1; }
    if (session->peer_closed) { snprintf(error, ERROR_CAPACITY, "EOF"); return -1; }
    return operation_result(session, mbedtls_ssl_read(&session->ssl, data, length), error);
}

int zore_tls_write(void *pointer, const unsigned char *data, size_t length, char *error) {
    Session *session = pointer;
    if (session->failed) { snprintf(error, ERROR_CAPACITY, "TLS failed connection"); return -1; }
    if (session->peer_closed) { snprintf(error, ERROR_CAPACITY, "TLS peer closed connection"); return -1; }
    return operation_result(session, mbedtls_ssl_write(&session->ssl, data, length), error);
}

int zore_tls_feed(void *pointer, const unsigned char *data, size_t length, char *error) {
    Session *session = pointer;
    if (session->failed) { snprintf(error, ERROR_CAPACITY, "TLS failed connection"); return -1; }
    if (session->peer_closed) { snprintf(error, ERROR_CAPACITY, "TLS peer closed connection"); return -1; }
    if (length > CAPACITY - session->input_length) {
        snprintf(error, ERROR_CAPACITY, "ciphertext buffer limit exceeded"); return -1;
    }
    memcpy(session->input + session->input_length, data, length);
    session->input_length += length;
    return 0;
}

size_t zore_tls_pending(void *pointer) { return ((Session *)pointer)->output_length; }

int zore_tls_output(void *pointer, unsigned char *data) {
    Session *session = pointer;
    int length = (int) session->output_length;
    memcpy(data, session->output, session->output_length);
    session->output_length = 0;
    return length;
}

int zore_tls_shutdown(void *pointer, char *error) {
    Session *session = pointer;
    if (session->failed) { snprintf(error, ERROR_CAPACITY, "TLS failed connection"); return -1; }
    if (session->shutdown_sent) return 0;
    int status = mbedtls_ssl_close_notify(&session->ssl);
    if (status == 0) session->shutdown_sent = 1;
    return operation_result(session, status, error);
}

int zore_tls_status(void *pointer) { return ((Session *)pointer)->status; }
