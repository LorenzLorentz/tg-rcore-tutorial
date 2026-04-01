#include "doomgeneric.h"
#include "rcore.h"

int main(int argc, char **argv)
{
    doomgeneric_Create(argc, argv);

    for (;;) {
        doomgeneric_Tick();
        rcore_sys_sched_yield();
    }
}
