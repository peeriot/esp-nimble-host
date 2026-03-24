#ifndef _STRING_H
#define _STRING_H

#include <stdint.h>

/* Use compiler builtins for bare-metal */
#define memcpy(dst, src, size)  __builtin_memcpy(dst, src, size)
#define memcmp(ptr1, ptr2, size) __builtin_memcmp(ptr1, ptr2, size)
#define memset(dst, value, size) __builtin_memset(dst, value, size)
#define memmove(dst, src, size) __builtin_memmove(dst, src, size)
#define strlen(str)             __builtin_strlen(str)
#define strcmp(str1, str2)      __builtin_strcmp(str1, str2)
#define strncmp(str1, str2, n)  __builtin_strncmp(str1, str2, n)
#define strcpy(dst, src)        __builtin_strcpy(dst, src)
#define strncpy(dst, src, n)    __builtin_strncpy(dst, src, n)
#define strcat(dst, src)        __builtin_strcat(dst, src)
#define strncat(dst, src, n)    __builtin_strncat(dst, src, n)

#endif
