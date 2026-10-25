#if defined(_WIN32)
#define _CRT_SECURE_NO_WARNINGS
#define WIN32_LEAN_AND_MEAN
#endif
#include "tls.h"
#include "mbedtls/ssl.h"
#include "mbedtls/base64.h"
#include "mbedtls/entropy.h"
#include "mbedtls/ctr_drbg.h"
#include "mbedtls/error.h"
#include "mbedtls/platform_util.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#if defined(_WIN32)
#include <windows.h>
#include <wincrypt.h>
#pragma comment(lib, "crypt32.lib")
#endif

#define CAPACITY 65536
#define ERROR_CAPACITY 512
#define ROOTS_OUTPUT_LIMIT (1024 * 1024)
#define ROOTS_FILE_LIMIT (4 * 1024 * 1024)
#define PEM_LINE_BYTES 48

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
    unsigned char ip[16];
    size_t ip_length;
} Session;

static int verify_ip(void *pointer, mbedtls_x509_crt *certificate, int depth, uint32_t *flags) {
    const Session *session = pointer;
    if (depth != 0) return 0;
    for (const mbedtls_x509_sequence *name = &certificate->subject_alt_names; name != NULL; name = name->next) {
        if ((name->buf.tag & MBEDTLS_ASN1_TAG_VALUE_MASK) == MBEDTLS_X509_SAN_IP_ADDRESS &&
            name->buf.len == session->ip_length && memcmp(name->buf.p, session->ip, session->ip_length) == 0) return 0;
    }
    *flags |= MBEDTLS_X509_BADCERT_CN_MISMATCH;
    return 0;
}

static int looks_like_ip(const char *name) {
    int numeric = *name != 0;
    for (const char *c = name; *c != 0; c += 1) {
        if (*c == ':') return 1;
        if (*c != '.' && (*c < '0' || *c > '9')) numeric = 0;
    }
    return numeric;
}

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
        session->ip_length = mbedtls_x509_crt_parse_cn_inet_pton(name, session->ip);
        if (session->ip_length == 0 && looks_like_ip(name)) {
            snprintf(error, ERROR_CAPACITY, "TLS invalid IP address server name");
            zore_tls_session_free(session);
            return NULL;
        }
        status = mbedtls_ssl_set_hostname(&session->ssl, session->ip_length != 0 ? NULL : name);
        if (status != 0) goto fail;
        if (session->ip_length != 0) mbedtls_ssl_set_verify(&session->ssl, verify_ip, session);
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

void zore_tls_free(void *pointer) { free(pointer); }

static FILE *open_bundle(const char *path) {
#if defined(_WIN32)
    wchar_t wide[4096];
    if (MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path, -1, wide, 4096) == 0) return NULL;
    return _wfopen(wide, L"rb");
#else
    return fopen(path, "rb");
#endif
}

static int parse_bundle(mbedtls_x509_crt *chain, const char *path) {
    FILE *file = open_bundle(path);
    if (file == NULL) return 1;
    unsigned char *data = malloc(ROOTS_FILE_LIMIT + 2);
    if (data == NULL) { fclose(file); return 1; }
    size_t length = fread(data, 1, ROOTS_FILE_LIMIT + 1, file);
    int failed = ferror(file) || length > ROOTS_FILE_LIMIT;
    fclose(file);
    if (failed) { free(data); return 1; }
    data[length] = 0;
    int status = mbedtls_x509_crt_parse(chain, data, length + 1);
    free(data);
    return status >= 0 && chain->raw.len > 0 ? 0 : 2;
}

static size_t pem_size(size_t raw_length) {
    size_t encoded = (raw_length + 2) / 3 * 4;
    return 28 + encoded + (encoded + 63) / 64 + 26;
}

static size_t write_pem(unsigned char *out, const mbedtls_x509_crt *certificate) {
    unsigned char *cursor = out;
    memcpy(cursor, "-----BEGIN CERTIFICATE-----\n", 28);
    cursor += 28;
    for (size_t offset = 0; offset < certificate->raw.len; offset += PEM_LINE_BYTES) {
        size_t chunk = certificate->raw.len - offset;
        if (chunk > PEM_LINE_BYTES) chunk = PEM_LINE_BYTES;
        size_t written = 0;
        unsigned char line[PEM_LINE_BYTES / 3 * 4 + 1];
        mbedtls_base64_encode(line, sizeof(line), &written, certificate->raw.p + offset, chunk);
        memcpy(cursor, line, written);
        cursor += written;
        *cursor++ = '\n';
    }
    memcpy(cursor, "-----END CERTIFICATE-----\n", 26);
    return (size_t)(cursor - out) + 26;
}

#if defined(_WIN32)
static int load_platform_store(mbedtls_x509_crt *chain, char *error) {
    HCERTSTORE store = CertOpenSystemStoreW(0, L"ROOT");
    if (store == NULL) { snprintf(error, ERROR_CAPACITY, "cannot open the Windows ROOT certificate store"); return 1; }
    const CERT_CONTEXT *context = NULL;
    while ((context = CertEnumCertificatesInStore(store, context)) != NULL) {
        mbedtls_x509_crt_parse_der(chain, context->pbCertEncoded, context->cbCertEncoded);
    }
    CertCloseStore(store, 0);
    if (chain->raw.len == 0) { snprintf(error, ERROR_CAPACITY, "no usable system root certificates"); return 1; }
    return 0;
}
#else
static int load_platform_store(mbedtls_x509_crt *chain, char *error) {
    static const char *const bundles[] = {
        "/etc/ssl/certs/ca-certificates.crt",
        "/etc/pki/tls/certs/ca-bundle.crt",
        "/etc/ssl/ca-bundle.pem",
        "/etc/pki/tls/cacert.pem",
        "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
        "/etc/ssl/cert.pem",
    };
    for (size_t i = 0; i < sizeof(bundles) / sizeof(bundles[0]); i += 1) {
        if (parse_bundle(chain, bundles[i]) == 0) return 0;
        mbedtls_x509_crt_free(chain);
        mbedtls_x509_crt_init(chain);
    }
    snprintf(error, ERROR_CAPACITY, "no system root certificate bundle found");
    return 1;
}
#endif

unsigned char *zore_tls_system_roots(const char *path, size_t *length, char *error) {
    mbedtls_x509_crt chain;
    mbedtls_x509_crt_init(&chain);
    unsigned char *result = NULL;
    if (path[0] != 0) {
        int status = parse_bundle(&chain, path);
        if (status == 1) snprintf(error, ERROR_CAPACITY, "cannot read %.400s", path);
        else if (status == 2) snprintf(error, ERROR_CAPACITY, "no usable certificates in %.400s", path);
        if (status != 0) goto done;
    } else if (load_platform_store(&chain, error) != 0) {
        goto done;
    }
    size_t total = 0;
    for (const mbedtls_x509_crt *certificate = &chain; certificate != NULL && certificate->raw.len > 0; certificate = certificate->next) {
        total += pem_size(certificate->raw.len);
        if (total > ROOTS_OUTPUT_LIMIT) {
            snprintf(error, ERROR_CAPACITY, "system root certificates exceed 1 MiB");
            goto done;
        }
    }
    result = malloc(total + 1);
    if (result == NULL) { snprintf(error, ERROR_CAPACITY, "allocation failed"); goto done; }
    size_t used = 0;
    for (const mbedtls_x509_crt *certificate = &chain; certificate != NULL && certificate->raw.len > 0; certificate = certificate->next) {
        used += write_pem(result + used, certificate);
    }
    *length = used;
done:
    mbedtls_x509_crt_free(&chain);
    return result;
}
