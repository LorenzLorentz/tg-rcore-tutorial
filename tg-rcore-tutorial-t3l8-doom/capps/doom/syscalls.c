#include "rcore.h"

#include <errno.h>
#include <fcntl.h>
#include <reent.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

#define RCORE_SYS_LSEEK 62u
#define RCORE_SYS_READ 63u
#define RCORE_SYS_WRITE 64u
#define RCORE_SYS_OPENAT 56u
#define RCORE_SYS_CLOSE 57u
#define RCORE_SYS_EXIT 93u
#define RCORE_SYS_CLOCK_GETTIME 113u
#define RCORE_SYS_SCHED_YIELD 124u
#define RCORE_SYS_GETPID 172u
#define RCORE_SYS_BRK 214u
#define RCORE_SYS_FRAMEBUFFER_GETINFO 5000u
#define RCORE_SYS_FRAMEBUFFER_PRESENT 5001u
#define RCORE_SYS_INPUT_POLL 5002u

static long rcore_raw_syscall0(long id)
{
    register long a0 asm("a0");
    register long a7 asm("a7") = id;
    asm volatile("ecall" : "=r"(a0) : "r"(a7) : "memory");
    return a0;
}

static long rcore_raw_syscall1(long id, long arg0)
{
    register long a0 asm("a0") = arg0;
    register long a7 asm("a7") = id;
    asm volatile("ecall" : "+r"(a0) : "r"(a7) : "memory");
    return a0;
}

static long rcore_raw_syscall2(long id, long arg0, long arg1)
{
    register long a0 asm("a0") = arg0;
    register long a1 asm("a1") = arg1;
    register long a7 asm("a7") = id;
    asm volatile("ecall" : "+r"(a0) : "r"(a1), "r"(a7) : "memory");
    return a0;
}

static long rcore_raw_syscall3(long id, long arg0, long arg1, long arg2)
{
    register long a0 asm("a0") = arg0;
    register long a1 asm("a1") = arg1;
    register long a2 asm("a2") = arg2;
    register long a7 asm("a7") = id;
    asm volatile("ecall" : "+r"(a0) : "r"(a1), "r"(a2), "r"(a7) : "memory");
    return a0;
}

static int set_errno(struct _reent *reent, int value)
{
    if (reent != NULL) {
        reent->_errno = value;
    } else {
        errno = value;
    }
    return -1;
}

long rcore_sys_read(int fd, void *buf, size_t count)
{
    return rcore_raw_syscall3(RCORE_SYS_READ, fd, (long)buf, (long)count);
}

long rcore_sys_write(int fd, const void *buf, size_t count)
{
    return rcore_raw_syscall3(RCORE_SYS_WRITE, fd, (long)buf, (long)count);
}

long rcore_sys_open(const char *path, int flags)
{
    return rcore_raw_syscall2(RCORE_SYS_OPENAT, (long)path, flags);
}

long rcore_sys_close(int fd)
{
    return rcore_raw_syscall1(RCORE_SYS_CLOSE, fd);
}

long rcore_sys_lseek(int fd, long offset, int whence)
{
    return rcore_raw_syscall3(RCORE_SYS_LSEEK, fd, offset, whence);
}

long rcore_sys_sched_yield(void)
{
    return rcore_raw_syscall0(RCORE_SYS_SCHED_YIELD);
}

long rcore_sys_clock_gettime(size_t clock_id, struct rcore_timespec *tp)
{
    return rcore_raw_syscall2(RCORE_SYS_CLOCK_GETTIME, (long)clock_id, (long)tp);
}

long rcore_sys_sbrk(long increment)
{
    return rcore_raw_syscall1(RCORE_SYS_BRK, increment);
}

long rcore_sys_getpid(void)
{
    return rcore_raw_syscall0(RCORE_SYS_GETPID);
}

long rcore_sys_framebuffer_getinfo(struct rcore_framebuffer_info *info)
{
    return rcore_raw_syscall1(RCORE_SYS_FRAMEBUFFER_GETINFO, (long)info);
}

long rcore_sys_framebuffer_present(const void *pixels, size_t width, size_t height)
{
    return rcore_raw_syscall3(
        RCORE_SYS_FRAMEBUFFER_PRESENT,
        (long)pixels,
        (long)width,
        (long)height
    );
}

long rcore_sys_input_poll(struct rcore_input_event *event)
{
    return rcore_raw_syscall1(RCORE_SYS_INPUT_POLL, (long)event);
}

void rcore_sys_exit(int status)
{
    rcore_raw_syscall1(RCORE_SYS_EXIT, status);
    for (;;) {
    }
}

int _open_r(struct _reent *reent, const char *path, int flags, int mode)
{
    long rc;
    (void)mode;
    rc = rcore_sys_open(path, flags);
    if (rc < 0) {
        return set_errno(reent, ENOENT);
    }
    return (int)rc;
}

int _open(const char *path, int flags, int mode)
{
    return _open_r(_impure_ptr, path, flags, mode);
}

int _close_r(struct _reent *reent, int fd)
{
    long rc = rcore_sys_close(fd);
    if (rc < 0) {
        return set_errno(reent, EBADF);
    }
    return 0;
}

int _close(int fd)
{
    return _close_r(_impure_ptr, fd);
}

_ssize_t _read_r(struct _reent *reent, int fd, void *buf, size_t count)
{
    long rc = rcore_sys_read(fd, buf, count);
    if (rc < 0) {
        return set_errno(reent, EBADF);
    }
    return (_ssize_t)rc;
}

_ssize_t _read(int fd, void *buf, size_t count)
{
    return _read_r(_impure_ptr, fd, buf, count);
}

_ssize_t _write_r(struct _reent *reent, int fd, const void *buf, size_t count)
{
    long rc = rcore_sys_write(fd, buf, count);
    if (rc < 0) {
        return set_errno(reent, EBADF);
    }
    return (_ssize_t)rc;
}

_ssize_t _write(int fd, const void *buf, size_t count)
{
    return _write_r(_impure_ptr, fd, buf, count);
}

_off_t _lseek_r(struct _reent *reent, int fd, _off_t offset, int whence)
{
    long rc = rcore_sys_lseek(fd, offset, whence);
    if (rc < 0) {
        return set_errno(reent, EINVAL);
    }
    return (_off_t)rc;
}

_off_t _lseek(int fd, _off_t offset, int whence)
{
    return _lseek_r(_impure_ptr, fd, offset, whence);
}

int _fstat_r(struct _reent *reent, int fd, struct stat *st)
{
    memset(st, 0, sizeof(*st));
    if (fd >= 0 && fd <= 2) {
        st->st_mode = S_IFCHR;
        st->st_nlink = 1;
        return 0;
    }
    if (fd < 0) {
        return set_errno(reent, EBADF);
    }
    st->st_mode = S_IFREG;
    st->st_nlink = 1;
    st->st_blksize = 512;
    return 0;
}

int _fstat(int fd, struct stat *st)
{
    return _fstat_r(_impure_ptr, fd, st);
}

int _isatty_r(struct _reent *reent, int fd)
{
    (void)reent;
    return fd >= 0 && fd <= 2;
}

int _isatty(int fd)
{
    return _isatty_r(_impure_ptr, fd);
}

void *_sbrk_r(struct _reent *reent, ptrdiff_t increment)
{
    long rc = rcore_sys_sbrk((long)increment);
    if (rc < 0) {
        set_errno(reent, ENOMEM);
        return (void *)-1;
    }
    return (void *)rc;
}

void *_sbrk(ptrdiff_t increment)
{
    return _sbrk_r(_impure_ptr, increment);
}

int _getpid_r(struct _reent *reent)
{
    long rc = rcore_sys_getpid();
    if (rc < 0) {
        return set_errno(reent, EIO);
    }
    return (int)rc;
}

int _getpid(void)
{
    return _getpid_r(_impure_ptr);
}

int _kill_r(struct _reent *reent, int pid, int sig)
{
    (void)pid;
    (void)sig;
    return set_errno(reent, ENOSYS);
}

int _kill(int pid, int sig)
{
    return _kill_r(_impure_ptr, pid, sig);
}

int _gettimeofday_r(struct _reent *reent, struct timeval *tv, void *tz)
{
    struct rcore_timespec tp;
    long rc;

    (void)tz;
    rc = rcore_sys_clock_gettime(RCORE_CLOCK_MONOTONIC, &tp);
    if (rc < 0) {
        return set_errno(reent, EIO);
    }
    if (tv != NULL) {
        tv->tv_sec = (time_t)tp.tv_sec;
        tv->tv_usec = (suseconds_t)(tp.tv_nsec / 1000u);
    }
    return 0;
}

int _gettimeofday(struct timeval *tv, void *tz)
{
    return _gettimeofday_r(_impure_ptr, tv, tz);
}

void _exit(int status)
{
    rcore_sys_exit(status);
}

int mkdir(const char *path, mode_t mode)
{
    (void)path;
    (void)mode;
    return 0;
}

int remove(const char *path)
{
    (void)path;
    errno = ENOSYS;
    return -1;
}

int rename(const char *oldpath, const char *newpath)
{
    (void)oldpath;
    (void)newpath;
    errno = ENOSYS;
    return -1;
}

int system(const char *command)
{
    (void)command;
    errno = ENOSYS;
    return -1;
}
