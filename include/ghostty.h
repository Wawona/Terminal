/* Compatibility shim. Rust VT C ABI moved to wwn_term.h (Phase 5).
   Zig GhosttyLib owns ghostty_surface_* / ghostty_init for Metal. */
#ifndef GHOSTTY_H
#define GHOSTTY_H

#include "wwn_term.h"

/* Legacy type aliases for in-tree hosts still compiling against GhosttyCell. */
typedef WwnTermCell GhosttyCell;
typedef WwnTermSurface GhosttySurface;

#define GHOSTTY_BOLD WWN_TERM_BOLD
#define GHOSTTY_DIM WWN_TERM_DIM
#define GHOSTTY_ITALIC WWN_TERM_ITALIC
#define GHOSTTY_UNDERLINE WWN_TERM_UNDERLINE

#endif
