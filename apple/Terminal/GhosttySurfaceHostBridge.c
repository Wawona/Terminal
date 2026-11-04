/* Zig GhosttyLib surface host trampoline. See GhosttySurfaceHostBridge.h. */
#include "GhosttySurfaceHostBridge.h"

#include <stdlib.h>
#include <string.h>

#if defined(__APPLE__)
#include <TargetConditionals.h>
#endif

/* Prebuild writes this into DERIVED_FILE_DIR when WAWONA_LINK_LIBGHOSTTY=1.
   Do not rely on GCC_PREPROCESSOR_DEFINITIONS from the late xcconfig. */
#if defined(__has_include)
#if __has_include("wwn_ghostty_host_config.h")
#include "wwn_ghostty_host_config.h"
#endif
#endif

#if defined(WAWONA_HAS_LIBGHOSTTY) && defined(__APPLE__) && TARGET_OS_OSX

#ifndef GHOSTTY_STATIC
#define GHOSTTY_STATIC 1
#endif
#include <ghostty.h>

struct WwnGhosttySurfaceHost {
  ghostty_config_t cfg;
  ghostty_app_t app;
  ghostty_surface_t surface;
};

static void wakeup_cb(void *ud) { (void)ud; }

static bool action_cb(ghostty_app_t app, ghostty_target_s target, ghostty_action_s action) {
  (void)app;
  (void)target;
  (void)action;
  return true;
}

static ghostty_clipboard_read_result_e read_clipboard_cb(
    void *ud, ghostty_clipboard_e clip, void *req, const char *const *mimes, size_t mime_count,
    bool confirmed) {
  (void)ud;
  (void)clip;
  (void)req;
  (void)mimes;
  (void)mime_count;
  (void)confirmed;
  return GHOSTTY_CLIPBOARD_READ_UNAVAILABLE;
}

static void confirm_read_clipboard_cb(
    void *ud, const ghostty_clipboard_confirm_s *c, void *req, ghostty_clipboard_request_e r) {
  (void)ud;
  (void)c;
  (void)req;
  (void)r;
}

static void write_clipboard_cb(
    void *ud, ghostty_clipboard_e clip, const ghostty_clipboard_content_s *contents,
    size_t contents_len, bool confirmed) {
  (void)ud;
  (void)clip;
  (void)contents;
  (void)contents_len;
  (void)confirmed;
}

static void close_surface_cb(void *ud, bool force) {
  (void)ud;
  (void)force;
}

bool wwn_ghostty_surface_host_available(void) { return true; }

WwnGhosttySurfaceHost *wwn_ghostty_surface_host_start(
    void *nsview, double scale, uint32_t width_px, uint32_t height_px) {
  if (!nsview || width_px == 0 || height_px == 0) {
    return NULL;
  }

  static int inited = 0;
  if (!inited) {
    char *argv[] = {"wawona", NULL};
    int rc = ghostty_init(1, argv);
    if (rc != GHOSTTY_SUCCESS) {
      return NULL;
    }
    inited = 1;
  }

  ghostty_config_t cfg = ghostty_config_new();
  if (!cfg) {
    return NULL;
  }
  ghostty_config_load_default_files(cfg);
  ghostty_config_finalize(cfg);

  ghostty_runtime_config_s runtime = {
      .userdata = nsview,
      .supports_selection_clipboard = false,
      .wakeup_cb = wakeup_cb,
      .action_cb = action_cb,
      .read_clipboard_cb = read_clipboard_cb,
      .confirm_read_clipboard_cb = confirm_read_clipboard_cb,
      .write_clipboard_cb = write_clipboard_cb,
      .close_surface_cb = close_surface_cb,
  };

  ghostty_app_t app = ghostty_app_new(&runtime, cfg);
  if (!app) {
    ghostty_config_free(cfg);
    return NULL;
  }

  ghostty_surface_config_s sc = ghostty_surface_config_new();
  sc.platform_tag = GHOSTTY_PLATFORM_MACOS;
  sc.platform.macos.nsview = nsview;
  sc.userdata = nsview;
  sc.scale_factor = scale > 0.0 ? scale : 2.0;
  sc.font_size = 13.0f;
  /* Visible paint for tip/screenshot. Own shell; not wwn_term feed path. */
  sc.command = "/bin/zsh -c 'printf \"\\nWawona GhosttyLib paint (Zig)\\n\"; exec sleep 86400'";
  sc.wait_after_command = false;
  sc.context = GHOSTTY_SURFACE_CONTEXT_WINDOW;

  ghostty_surface_t surface = ghostty_surface_new(app, &sc);
  if (!surface) {
    ghostty_app_free(app);
    ghostty_config_free(cfg);
    return NULL;
  }

  ghostty_surface_set_size(surface, width_px, height_px);
  ghostty_surface_set_content_scale(surface, sc.scale_factor, sc.scale_factor);
  ghostty_surface_set_focus(surface, true);
  ghostty_app_tick(app);
  ghostty_surface_refresh(surface);
  ghostty_surface_draw(surface);

  WwnGhosttySurfaceHost *host = calloc(1, sizeof(*host));
  if (!host) {
    ghostty_surface_free(surface);
    ghostty_app_free(app);
    ghostty_config_free(cfg);
    return NULL;
  }
  host->cfg = cfg;
  host->app = app;
  host->surface = surface;
  return host;
}

void wwn_ghostty_surface_host_resize(
    WwnGhosttySurfaceHost *host, uint32_t width_px, uint32_t height_px, double scale) {
  if (!host || !host->surface || width_px == 0 || height_px == 0) {
    return;
  }
  double s = scale > 0.0 ? scale : 2.0;
  ghostty_surface_set_size(host->surface, width_px, height_px);
  ghostty_surface_set_content_scale(host->surface, s, s);
  ghostty_surface_refresh(host->surface);
}

void wwn_ghostty_surface_host_draw(WwnGhosttySurfaceHost *host) {
  if (!host || !host->app || !host->surface) {
    return;
  }
  ghostty_app_tick(host->app);
  ghostty_surface_draw(host->surface);
}

void wwn_ghostty_surface_host_free(WwnGhosttySurfaceHost *host) {
  if (!host) {
    return;
  }
  if (host->surface) {
    ghostty_surface_free(host->surface);
  }
  if (host->app) {
    ghostty_app_free(host->app);
  }
  if (host->cfg) {
    ghostty_config_free(host->cfg);
  }
  free(host);
}

#else /* stub: GhosttyGpu owns paint */

bool wwn_ghostty_surface_host_available(void) { return false; }

WwnGhosttySurfaceHost *wwn_ghostty_surface_host_start(
    void *nsview, double scale, uint32_t width_px, uint32_t height_px) {
  (void)nsview;
  (void)scale;
  (void)width_px;
  (void)height_px;
  return NULL;
}

void wwn_ghostty_surface_host_resize(
    WwnGhosttySurfaceHost *host, uint32_t width_px, uint32_t height_px, double scale) {
  (void)host;
  (void)width_px;
  (void)height_px;
  (void)scale;
}

void wwn_ghostty_surface_host_draw(WwnGhosttySurfaceHost *host) { (void)host; }

void wwn_ghostty_surface_host_free(WwnGhosttySurfaceHost *host) { (void)host; }

#endif
