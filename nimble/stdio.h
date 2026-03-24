#ifndef _STDIO_H
#define _STDIO_H

#include <stdint.h>

/* Bare-metal stubs for printf-family functions. */

/* Minimal sprintf — just copies the format string as-is (enough for NimBLE's
   UUID formatting which uses %02x via the host's own formatting path). */
int sprintf(char *buf, const char *fmt, ...);
int snprintf(char *buf, size_t size, const char *fmt, ...);

#endif
