#include "doomgeneric.h"
#include "doomkeys.h"
#include "rcore.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define KEYQUEUE_SIZE 128u

struct queued_key {
    int pressed;
    unsigned char key;
};

static struct rcore_framebuffer_info g_framebuffer;
static struct queued_key g_key_queue[KEYQUEUE_SIZE];
static unsigned int g_key_read = 0;
static unsigned int g_key_write = 0;
static uint32_t g_input_boot_ms = 0;
static int g_dropped_startup_escape = 0;
static int g_seen_key_press = 0;

static void queue_key(int pressed, unsigned char key)
{
    unsigned int next;

    if (key == 0) {
        return;
    }

    next = (g_key_write + 1) % KEYQUEUE_SIZE;
    if (next == g_key_read) {
        g_key_read = (g_key_read + 1) % KEYQUEUE_SIZE;
    }
    g_key_queue[g_key_write].pressed = pressed;
    g_key_queue[g_key_write].key = key;
    g_key_write = next;
}

static unsigned char translate_key(uint16_t code)
{
    switch (code) {
    case 1:
        return KEY_ESCAPE;
    case 2:
        return '1';
    case 3:
        return '2';
    case 4:
        return '3';
    case 5:
        return '4';
    case 6:
        return '5';
    case 7:
        return '6';
    case 8:
        return '7';
    case 9:
        return '8';
    case 10:
        return '9';
    case 11:
        return '0';
    case 12:
        return '-';
    case 13:
        return '=';
    case 14:
        return KEY_BACKSPACE;
    case 15:
        return KEY_TAB;
    case 16:
        return 'q';
    case 17:
        return 'w';
    case 18:
        return 'e';
    case 19:
        return 'r';
    case 20:
        return 't';
    case 21:
        return 'y';
    case 22:
        return 'u';
    case 23:
        return 'i';
    case 24:
        return 'o';
    case 25:
        return 'p';
    case 26:
        return '[';
    case 27:
        return ']';
    case 28:
        return KEY_ENTER;
    case 29:
    case 97:
        return KEY_FIRE;
    case 30:
        return 'a';
    case 31:
        return 's';
    case 32:
        return 'd';
    case 33:
        return 'f';
    case 34:
        return 'g';
    case 35:
        return 'h';
    case 36:
        return 'j';
    case 37:
        return 'k';
    case 38:
        return 'l';
    case 39:
        return ';';
    case 40:
        return '\'';
    case 41:
        return '`';
    case 42:
    case 54:
        return KEY_RSHIFT;
    case 43:
        return '\\';
    case 44:
        return 'z';
    case 45:
        return 'x';
    case 46:
        return 'c';
    case 47:
        return 'v';
    case 48:
        return 'b';
    case 49:
        return 'n';
    case 50:
        return 'm';
    case 51:
        return ',';
    case 52:
        return '.';
    case 53:
        return '/';
    case 56:
    case 100:
        return KEY_LALT;
    case 57:
        return KEY_USE;
    case 103:
        return KEY_UPARROW;
    case 105:
        return KEY_LEFTARROW;
    case 106:
        return KEY_RIGHTARROW;
    case 108:
        return KEY_DOWNARROW;
    default:
        return 0;
    }
}

static void pump_input(void)
{
    struct rcore_input_event event;

    while (rcore_sys_input_poll(&event) > 0) {
        int pressed;
        unsigned char key;

        if (event.event_type != RCORE_INPUT_EVENT_KEY) {
            continue;
        }
        key = translate_key(event.code);
        if (key == 0) {
            continue;
        }
        if (event.value == RCORE_INPUT_VALUE_PRESS) {
            // Some host/QEMU combinations can deliver an initial stray Escape
            // while the window focus settles. Dropping the first startup Escape
            // avoids opening the menu on top of the first playable frame.
            if (!g_dropped_startup_escape
                    && !g_seen_key_press
                    && key == KEY_ESCAPE
                    && (uint32_t)(DG_GetTicksMs() - g_input_boot_ms) < 2000u) {
                g_dropped_startup_escape = 1;
                continue;
            }
            g_seen_key_press = 1;
        }
        pressed = event.value != RCORE_INPUT_VALUE_RELEASE;
        queue_key(pressed, key);
    }
}

void DG_Init(void)
{
    g_input_boot_ms = DG_GetTicksMs();
    if (rcore_sys_framebuffer_getinfo(&g_framebuffer) != 0) {
        fprintf(stderr, "doom: framebuffer unavailable\n");
        exit(1);
    }
    printf("doom: framebuffer %ux%u stride=%u format=%u\n",
           g_framebuffer.width,
           g_framebuffer.height,
           g_framebuffer.stride,
           g_framebuffer.format);
}

void DG_DrawFrame(void)
{
    pump_input();
    rcore_sys_framebuffer_present(DG_ScreenBuffer, DOOMGENERIC_RESX, DOOMGENERIC_RESY);
}

void DG_SleepMs(uint32_t ms)
{
    uint32_t deadline;

    deadline = DG_GetTicksMs() + ms;
    while ((int32_t)(DG_GetTicksMs() - deadline) < 0) {
        rcore_sys_sched_yield();
    }
}

uint32_t DG_GetTicksMs(void)
{
    struct rcore_timespec tp;

    if (rcore_sys_clock_gettime(RCORE_CLOCK_MONOTONIC, &tp) != 0) {
        return 0;
    }
    return (uint32_t)(tp.tv_sec * 1000u + tp.tv_nsec / 1000000u);
}

int DG_GetKey(int *pressed, unsigned char *doomKey)
{
    pump_input();
    if (g_key_read == g_key_write) {
        return 0;
    }

    *pressed = g_key_queue[g_key_read].pressed;
    *doomKey = g_key_queue[g_key_read].key;
    g_key_read = (g_key_read + 1) % KEYQUEUE_SIZE;
    return 1;
}

void DG_SetWindowTitle(const char *title)
{
    (void)title;
}
