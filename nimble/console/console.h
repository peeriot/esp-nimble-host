#ifndef __CONSOLE_H__
#define __CONSOLE_H__

/* Bare-metal no-op printf stub. */
static inline int os_printf(const char *format, ...) {
    (void)format;
    return 0;
}

#define console_printf(_fmt, ...) os_printf(_fmt, ##__VA_ARGS__)

#endif
