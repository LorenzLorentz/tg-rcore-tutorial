#include "doomgeneric.h"
#include "rcore.h"
#include <stdio.h>
#include <string.h>

static char arg_skill[] = "-skill";
static char arg_skill_value[] = "2";
static char arg_warp[] = "-warp";
static char arg_episode1[] = "1";
static char arg_map1[] = "1";
static char *launch_argv[16];

static int has_arg(int argc, char **argv, const char *arg)
{
    int i;

    for (i = 1; i < argc; ++i) {
        if (strcmp(argv[i], arg) == 0) {
            return 1;
        }
    }

    return 0;
}

static void print_controls(void)
{
    puts("doom: controls");
    puts("  move: W/S");
    puts("  strafe: A/D");
    puts("  turn: Left/Right");
    puts("  fire: Ctrl");
    puts("  use/open: Space");
    puts("  run: Right Shift");
    puts("  menu: Esc");
    puts("  menu nav: W/A/S/D or Arrow keys");
    puts("  menu confirm: Enter");
    puts("  menu back: Backspace");
    puts("  automap: Tab");
    puts("  automap move: W/A/S/D");
    puts("  automap zoom: +/-");
}

int main(int argc, char **argv)
{
    int launch_argc = 0;
    int i;

    for (i = 0; i < argc && launch_argc < (int)(sizeof(launch_argv) / sizeof(launch_argv[0])) - 1; ++i) {
        launch_argv[launch_argc++] = argv[i];
    }

    if (!has_arg(argc, argv, arg_skill)
            && launch_argc + 2 < (int)(sizeof(launch_argv) / sizeof(launch_argv[0]))) {
        launch_argv[launch_argc++] = arg_skill;
        launch_argv[launch_argc++] = arg_skill_value;
    }

    if (!has_arg(argc, argv, arg_warp)
            && !has_arg(argc, argv, "-episode")
            && launch_argc + 3 < (int)(sizeof(launch_argv) / sizeof(launch_argv[0]))) {
        launch_argv[launch_argc++] = arg_warp;
        launch_argv[launch_argc++] = arg_episode1;
        launch_argv[launch_argc++] = arg_map1;
    }
    launch_argv[launch_argc] = 0;

    doomgeneric_Create(launch_argc, launch_argv);
    print_controls();

    for (;;) {
        doomgeneric_Tick();
        rcore_sys_sched_yield();
    }
}
