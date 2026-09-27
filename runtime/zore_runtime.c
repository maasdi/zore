/*
 * Copyright 2026 The Zore Authors
 * SPDX-License-Identifier: Apache-2.0
 *
 * Minimal Zore runtime (decision record docs/decisions/0001-native-backend.md).
 *
 * Provides the process entry, `println` output (spec §37.1), string ordering
 * (§6.6), and panic reporting (§3.19, §18.10). The ABI is internal to the
 * bootstrap compiler and may change.
 */

#include <errno.h>
#include <signal.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

void zore_entry(void);

/* Exit status after a panic in the initial task (implementation-defined by
   §3.19; matches Go). */
enum { ZORE_PANIC_STATUS = 2 };

/* Write all bytes; returns 0 on success and -1 on failure. */
static int write_all(int fd, const char *data, size_t len) {
    while (len > 0) {
        ssize_t written = write(fd, data, len);
        if (written < 0) {
            if (errno == EINTR) {
                continue;
            }
            return -1;
        }
        data += written;
        len -= (size_t)written;
    }
    return 0;
}

_Noreturn void zore_panic(const char *message, int64_t len) {
    static const char prefix[] = "panic in the main task: ";
    /* Reporting is best effort: a failing stderr cannot be reported. */
    (void)write_all(2, prefix, sizeof prefix - 1);
    (void)write_all(2, message, (size_t)len);
    (void)write_all(2, "\n", 1);
    _exit(ZORE_PANIC_STATUS);
}

/* Write one complete line with a single write sequence, so each call emits
   its line as a unit (§37.1). A failed write panics. */
static void write_line(const char *text, size_t len) {
    char small[256];
    char *line = small;
    if (len + 1 > sizeof small) {
        line = malloc(len + 1);
        if (line == NULL) {
            static const char oom[] = "out of memory while printing";
            zore_panic(oom, sizeof oom - 1);
        }
    }
    memcpy(line, text, len);
    line[len] = '\n';
    int failed = write_all(1, line, len + 1);
    if (line != small) {
        free(line);
    }
    if (failed) {
        static const char message[] = "failed to write to standard output";
        zore_panic(message, sizeof message - 1);
    }
}

void zore_println_str(const char *data, int64_t len) {
    write_line(data, (size_t)len);
}

static size_t format_u64(char *end, uint64_t value) {
    size_t n = 0;
    do {
        *--end = (char)('0' + value % 10);
        value /= 10;
        n++;
    } while (value != 0);
    return n;
}

void zore_println_u64(uint64_t value) {
    char buffer[32];
    size_t n = format_u64(buffer + sizeof buffer, value);
    write_line(buffer + sizeof buffer - n, n);
}

void zore_println_i64(int64_t value) {
    char buffer[32];
    /* Negate in unsigned arithmetic so the minimum value is exact. */
    uint64_t magnitude = value < 0 ? 0 - (uint64_t)value : (uint64_t)value;
    size_t n = format_u64(buffer + sizeof buffer, magnitude);
    if (value < 0) {
        buffer[sizeof buffer - n - 1] = '-';
        n++;
    }
    write_line(buffer + sizeof buffer - n, n);
}

void zore_println_bool(_Bool value) {
    if (value) {
        write_line("true", 4);
    } else {
        write_line("false", 5);
    }
}

/* Runes are Unicode scalar values (§6.5); print their UTF-8 encoding. */
void zore_println_rune(uint32_t c) {
    char buffer[4];
    size_t n;
    if (c < 0x80) {
        buffer[0] = (char)c;
        n = 1;
    } else if (c < 0x800) {
        buffer[0] = (char)(0xC0 | c >> 6);
        buffer[1] = (char)(0x80 | (c & 0x3F));
        n = 2;
    } else if (c < 0x10000) {
        buffer[0] = (char)(0xE0 | c >> 12);
        buffer[1] = (char)(0x80 | (c >> 6 & 0x3F));
        buffer[2] = (char)(0x80 | (c & 0x3F));
        n = 3;
    } else {
        buffer[0] = (char)(0xF0 | c >> 18);
        buffer[1] = (char)(0x80 | (c >> 12 & 0x3F));
        buffer[2] = (char)(0x80 | (c >> 6 & 0x3F));
        buffer[3] = (char)(0x80 | (c & 0x3F));
        n = 4;
    }
    write_line(buffer, n);
}

/* Byte-wise UTF-8 ordering: negative, zero, or positive (§6.6). */
int32_t zore_string_compare(const char *a, int64_t a_len, const char *b, int64_t b_len) {
    size_t shorter = (size_t)(a_len < b_len ? a_len : b_len);
    int order = shorter == 0 ? 0 : memcmp(a, b, shorter);
    if (order != 0) {
        return order < 0 ? -1 : 1;
    }
    return a_len < b_len ? -1 : (a_len > b_len ? 1 : 0);
}

int main(void) {
    /* A closed stdout must surface as a failed write (and a panic), not as
       silent termination by SIGPIPE. */
    signal(SIGPIPE, SIG_IGN);
    zore_entry();
    return 0;
}
