/* Rust libghostty C ABI for Wawona Terminal hosts.
   Grid only. Chrome (windows, PTY, ToolbarKeys) stays in Wawona L4. */
#ifndef GHOSTTY_H
#define GHOSTTY_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

int ghostty_init(void);
void ghostty_shutdown(void);

typedef struct GhosttySurface GhosttySurface;

typedef struct GhosttyCell {
  uint32_t cp;
  uint32_t fg;
  uint32_t bg;
  uint32_t attrs;
} GhosttyCell;

#define GHOSTTY_BOLD 1u
#define GHOSTTY_DIM 2u
#define GHOSTTY_ITALIC 4u
#define GHOSTTY_UNDERLINE 8u

GhosttySurface *ghostty_surface_new(uint32_t cols, uint32_t rows);
void ghostty_surface_free(GhosttySurface *s);
void ghostty_surface_resize(GhosttySurface *s, uint32_t cols, uint32_t rows);
void ghostty_surface_write(GhosttySurface *s, const uint8_t *bytes, size_t len);
void ghostty_surface_key(GhosttySurface *s, uint32_t keycode, uint32_t mods, int down);
size_t ghostty_surface_take_reply(GhosttySurface *s, uint8_t *buf, size_t cap);
uint32_t ghostty_surface_viewport(
    const GhosttySurface *s, uint32_t origin, GhosttyCell *out, uint32_t cap,
    uint32_t *cols, uint32_t *rows);
uint32_t ghostty_surface_origin(const GhosttySurface *s);
uint32_t ghostty_surface_title(const GhosttySurface *s, uint8_t *out, uint32_t cap);
void ghostty_surface_set_cell_px(GhosttySurface *s, uint32_t width, uint32_t height);
void ghostty_surface_pointer(
    GhosttySurface *s, uint8_t button, uint32_t col, uint32_t row, int pressed,
    int motion);
char *ghostty_surface_encode_paste(const GhosttySurface *s, const uint8_t *bytes, size_t len);
void ghostty_string_free(char *ptr);

#ifdef __cplusplus
}
#endif

#endif
