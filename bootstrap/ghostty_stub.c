/* Legacy bootstrap stub. Product tip uses Rust wwn_term_* from
   libwawona_terminal.a. Zig GhosttyLib owns real ghostty_surface_*. */
#include <stddef.h>
#include <stdint.h>

int wwn_term_init(void) { return 0; }
void wwn_term_shutdown(void) {}

typedef struct WwnTermSurface WwnTermSurface;

WwnTermSurface *wwn_term_new(uint32_t cols, uint32_t rows) {
  (void)cols;
  (void)rows;
  return (WwnTermSurface *)(uintptr_t)1;
}
void wwn_term_free(WwnTermSurface *s) { (void)s; }
void wwn_term_feed(WwnTermSurface *s, const uint8_t *bytes, size_t len) {
  (void)s;
  (void)bytes;
  (void)len;
}
void wwn_term_key(
    WwnTermSurface *s, uint32_t keycode, uint32_t mods, uint32_t action, const uint8_t *text,
    size_t text_len) {
  (void)s;
  (void)keycode;
  (void)mods;
  (void)action;
  (void)text;
  (void)text_len;
}
void wwn_term_resize(WwnTermSurface *s, uint32_t cols, uint32_t rows) {
  (void)s;
  (void)cols;
  (void)rows;
}
