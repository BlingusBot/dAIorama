/* xdg-clamp.c — LD_PRELOAD shim for the P1/L1 spike.
 *
 * aquamarine 0.15.1's Wayland backend hardcodes registry binds above what
 * capable-but-older parents advertise (xdg_wm_base 6, wl_compositor 6,
 * wl_seat 9). The parent then kills the client with a protocol error.
 * This shim clamps those binds to versions the parent can honor; aquamarine
 * uses no request introduced after the clamped version for these interfaces.
 *
 * Spike-only (LD_PRELOAD); not part of the product story.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <wayland-util.h>

typedef void *(*bind_t)(void *, uint32_t, const struct wl_interface *, uint32_t);

__attribute__((constructor)) static void clamp_init(void) {
    fprintf(stderr, "[xdg-clamp] loaded\n");
}

static uint32_t clamp_for(const char *name, uint32_t version) {
    struct { const char *name; uint32_t max; } caps[] = {
        {"xdg_wm_base", 5},
        {"wl_compositor", 5},
        {"wl_seat", 5},
    };
    for (size_t i = 0; i < sizeof(caps) / sizeof(caps[0]); i++) {
        if (name && strcmp(name, caps[i].name) == 0 && version > caps[i].max)
            return caps[i].max;
    }
    return version;
}

void *wl_registry_bind(void *registry, uint32_t name,
                       const struct wl_interface *iface, uint32_t version) {
    static bind_t real = NULL;
    if (!real)
        real = (bind_t)dlsym(RTLD_NEXT, "wl_registry_bind");
    if (iface && iface->name) {
        fprintf(stderr, "[xdg-clamp] bind: %s v%u\n", iface->name, version);
        uint32_t clamped = clamp_for(iface->name, version);
        if (clamped != version) {
            fprintf(stderr, "[xdg-clamp] %s: bind version %u -> %u\n", iface->name, version, clamped);
            version = clamped;
        }
    }
    return real(registry, name, iface, version);
}
