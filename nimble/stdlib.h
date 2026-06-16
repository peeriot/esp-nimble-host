#ifndef _STDLIB_H
#define _STDLIB_H

#include <stdint.h>

/* Declared here, must be provided by the Rust linker (e.g. via esp-alloc). */
void *malloc(size_t size);
void *calloc(size_t nmemb, size_t size);
void *realloc(void *ptr, size_t size);
void  free(void *ptr);

#endif
