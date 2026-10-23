#ifndef ZORE_TLS_H
#define ZORE_TLS_H
#include <stddef.h>
#include <stdint.h>
void *zore_tls_config(int server, const unsigned char *cert, size_t cert_len,
                      const unsigned char *key, size_t key_len, char *error);
void zore_tls_config_free(void *config);
void *zore_tls_session(void *config, const char *name, char *error);
void zore_tls_session_free(void *session);
int zore_tls_handshake(void *session, char *error);
int zore_tls_read(void *session, unsigned char *data, size_t length, char *error);
int zore_tls_write(void *session, const unsigned char *data, size_t length, char *error);
int zore_tls_feed(void *session, const unsigned char *data, size_t length, char *error);
int zore_tls_output(void *session, unsigned char *data);
int zore_tls_status(void *session);
int zore_tls_shutdown(void *session, char *error);
#endif
