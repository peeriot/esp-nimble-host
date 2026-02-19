#ifndef _STDINT_H
#define _STDINT_H

// FIXME: Needs LIBC consistency
#include <inttypes.h>

/* Minimum of signed integral types.  */
#define INT8_MIN		(-128)
#define INT16_MIN		(-32767-1)
#define INT32_MIN		(-2147483647-1)
#define INT64_MIN		(-INT64_C(9223372036854775807)-1)

/* Maximum of signed integral types.  */
#define INT8_MAX		(127)
#define INT16_MAX		(32767)
#define INT32_MAX		(2147483647)
#define INT64_MAX		(INT64_C(9223372036854775807))

/* Maximum of unsigned integral types.  */
#define UINT8_MAX		(255)
#define UINT16_MAX		(65535)
#define UINT32_MAX		(4294967295U)
#define UINT64_MAX		(UINT64_C(18446744073709551615))


typedef __SIZE_TYPE__ size_t;
typedef size_t uintptr_t;


//#include <inttypes.h>
//
//#ifndef bool
//    typedef _Bool bool;
//#endif

#endif