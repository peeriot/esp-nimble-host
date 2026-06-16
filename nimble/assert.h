#ifndef _ASSERT_H
#define _ASSERT_H

#define assert(expr) ((void)0)
#define static_assert(expr, str) _Static_assert(expr, str)

#endif
