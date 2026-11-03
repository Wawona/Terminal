#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <util.h>

/* Swift marks fork unavailable. Standalone host needs a real login shell PTY. */
pid_t wawona_pty_spawn_shell(int *master_out, const char *shell) {
  int m = -1, s = -1;
  if (openpty(&m, &s, NULL, NULL, NULL) != 0) {
    return -1;
  }
  pid_t pid = fork();
  if (pid < 0) {
    close(m);
    close(s);
    return -1;
  }
  if (pid == 0) {
    close(m);
    if (login_tty(s) != 0) {
      _exit(126);
    }
    const char *sh = shell && shell[0] ? shell : "/bin/zsh";
    execl(sh, sh, "-l", (char *)NULL);
    _exit(127);
  }
  close(s);
  *master_out = m;
  return pid;
}
