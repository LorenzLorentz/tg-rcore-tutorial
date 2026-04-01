#ifndef RCORE_DOOM_PORT_H
#define RCORE_DOOM_PORT_H

#include <stddef.h>
#include <stdint.h>

struct rcore_timespec {
    uint64_t tv_sec;
    uint64_t tv_nsec;
};

struct rcore_framebuffer_info {
    uint32_t width;
    uint32_t height;
    uint32_t stride;
    uint32_t format;
};

struct rcore_input_event {
    uint16_t event_type;
    uint16_t code;
    uint32_t value;
};

#define RCORE_CLOCK_MONOTONIC 1u

#define RCORE_INPUT_EVENT_KEY 0x01u
#define RCORE_INPUT_VALUE_RELEASE 0u
#define RCORE_INPUT_VALUE_PRESS 1u
#define RCORE_INPUT_VALUE_REPEAT 2u

long rcore_sys_read(int fd, void *buf, size_t count);
long rcore_sys_write(int fd, const void *buf, size_t count);
long rcore_sys_open(const char *path, int flags);
long rcore_sys_close(int fd);
long rcore_sys_lseek(int fd, long offset, int whence);
long rcore_sys_sched_yield(void);
long rcore_sys_clock_gettime(size_t clock_id, struct rcore_timespec *tp);
long rcore_sys_sbrk(long increment);
long rcore_sys_getpid(void);
long rcore_sys_framebuffer_getinfo(struct rcore_framebuffer_info *info);
long rcore_sys_framebuffer_present(const void *pixels, size_t width, size_t height);
long rcore_sys_input_poll(struct rcore_input_event *event);

void rcore_sys_exit(int status) __attribute__((noreturn));

#endif
