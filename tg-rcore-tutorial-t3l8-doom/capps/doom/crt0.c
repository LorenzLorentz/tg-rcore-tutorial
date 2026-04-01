#include "rcore.h"

extern int main(int argc, char **argv);

char **environ = 0;

static char arg0[] = "doom";
static char arg_iwad[] = "-iwad";
static char arg_doom1[] = "doom1.wad";
static char arg_freedoom1[] = "freedoom1.wad";
static char arg_nosound[] = "-nosound";
static char arg_nomusic[] = "-nomusic";
static char arg_cdrom[] = "-cdrom";
static char *argv_buffer[8];

static char *select_iwad(void)
{
    long fd = rcore_sys_open(arg_doom1, 0);
    if (fd >= 0) {
        rcore_sys_close((int)fd);
        return arg_doom1;
    }
    fd = rcore_sys_open(arg_freedoom1, 0);
    if (fd >= 0) {
        rcore_sys_close((int)fd);
        return arg_freedoom1;
    }
    return arg_doom1;
}

__attribute__((section(".text.entry"), noreturn)) void _start(void)
{
    int argc;
    int rc;

    argv_buffer[0] = arg0;
    argv_buffer[1] = arg_iwad;
    argv_buffer[2] = select_iwad();
    argv_buffer[3] = arg_nosound;
    argv_buffer[4] = arg_nomusic;
    argv_buffer[5] = arg_cdrom;
    argv_buffer[6] = 0;
    argc = 6;

    rc = main(argc, argv_buffer);
    rcore_sys_exit(rc);
}
