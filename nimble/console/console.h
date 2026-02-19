#ifndef __CONSOLE_H__
#define __CONSOLE_H__

#define console_printf(_fmt, ...) os_printf(_fmt, ##__VA_ARGS__)

#endif
