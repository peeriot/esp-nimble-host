#ifndef _STDINT_H
#define _STDINT_H

/* Use compiler builtins for bare-metal targets */
typedef __INT8_TYPE__   int8_t;
typedef __UINT8_TYPE__  uint8_t;
typedef __INT16_TYPE__  int16_t;
typedef __UINT16_TYPE__ uint16_t;
typedef __INT32_TYPE__  int32_t;
typedef __UINT32_TYPE__ uint32_t;
typedef __INT64_TYPE__  int64_t;
typedef __UINT64_TYPE__ uint64_t;
typedef __SIZE_TYPE__   size_t;
typedef size_t          uintptr_t;
typedef _Bool           bool;

/* Minimum of signed integral types. */
#define INT8_MIN   (-128)
#define INT16_MIN  (-32767-1)
#define INT32_MIN  (-2147483647-1)
#define INT64_MIN  (-9223372036854775807LL-1)

/* Maximum of signed integral types. */
#define INT8_MAX   (127)
#define INT16_MAX  (32767)
#define INT32_MAX  (2147483647)
#define INT64_MAX  (9223372036854775807LL)

/* Maximum of unsigned integral types. */
#define UINT8_MAX  (255)
#define UINT16_MAX (65535)
#define UINT32_MAX (4294967295U)
#define UINT64_MAX (18446744073709551615ULL)

#endif
