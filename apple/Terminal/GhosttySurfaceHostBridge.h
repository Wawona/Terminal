/* Thin C trampoline for Zig GhosttyLib surface paint (Phase 5).
   When WAWONA_HAS_LIBGHOSTTY=1 and macOS tip links libghostty-internal.a,
   start/draw call ghostty_app_new + ghostty_surface_new(nsview) + draw.
   Otherwise stubs return unavailable so GhosttyGpu owns paint. */
#ifndef WWN_GHOSTTY_SURFACE_HOST_BRIDGE_H
#define WWN_GHOSTTY_SURFACE_HOST_BRIDGE_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct WwnGhosttySurfaceHost WwnGhosttySurfaceHost;

/** Compile-time Zig path (WAWONA_HAS_LIBGHOSTTY on macOS tip). */
bool wwn_ghostty_surface_host_available(void);

/** Attach GhosttyLib Metal surface to an NSView*. NULL on failure / stub. */
WwnGhosttySurfaceHost *wwn_ghostty_surface_host_start(
    void *nsview, double scale, uint32_t width_px, uint32_t height_px);

void wwn_ghostty_surface_host_resize(
    WwnGhosttySurfaceHost *host, uint32_t width_px, uint32_t height_px, double scale);

void wwn_ghostty_surface_host_draw(WwnGhosttySurfaceHost *host);

void wwn_ghostty_surface_host_free(WwnGhosttySurfaceHost *host);

#ifdef __cplusplus
}
#endif

#endif
