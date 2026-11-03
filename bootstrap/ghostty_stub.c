#include <stddef.h>
#include <stdint.h>
int ghostty_init(void) { return 0; }
void ghostty_shutdown(void) {}
typedef struct GhosttySurface GhosttySurface;
GhosttySurface *ghostty_surface_new(void *opts) { (void)opts; return (GhosttySurface *)(uintptr_t)1; }
void ghostty_surface_free(GhosttySurface *s) { (void)s; }
void ghostty_surface_write(GhosttySurface *s, const char *bytes, size_t len) {
  (void)s; (void)bytes; (void)len;
}
void ghostty_surface_key(GhosttySurface *s, uint32_t keycode, uint32_t mods, int down) {
  (void)s; (void)keycode; (void)mods; (void)down;
}
void ghostty_surface_resize(GhosttySurface *s, uint32_t cols, uint32_t rows, uint32_t px_w, uint32_t px_h) {
  (void)s; (void)cols; (void)rows; (void)px_w; (void)px_h;
}
void *ghostty_surface_layer(GhosttySurface *s) { (void)s; return NULL; }
