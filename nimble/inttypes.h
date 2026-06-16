#ifndef _INTTYPES_H
#define _INTTYPES_H

/* Bare-metal stub — just pull in our stdint.h which defines all integer types. */
#include <stdint.h>

/* Format macros for printf-family (stubbed — no printf on bare-metal). */
#define PRId8   "d"
#define PRId16  "d"
#define PRId32  "d"
#define PRId64  "lld"
#define PRIu8   "u"
#define PRIu16  "u"
#define PRIu32  "u"
#define PRIu64  "llu"
#define PRIx8   "x"
#define PRIx16  "x"
#define PRIx32  "x"
#define PRIx64  "llx"

#endif
