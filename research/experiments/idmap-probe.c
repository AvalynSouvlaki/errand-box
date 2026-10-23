#define _GNU_SOURCE
#include <fcntl.h>
#include <sched.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <unistd.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <linux/mount.h>
/* The same sequence as Incus 6.0 shared/idmap/shift_linux.go
   create_detached_idmapped_mount: open_tree(CLONE), a userns mapped "0 0 1",
   mount_setattr(MOUNT_ATTR_IDMAP). Prints 0 or the errno name. */
static int child(void *a) { (void)a; kill(getpid(), SIGSTOP); return 0; }
static void wr(int pid, const char *f, const char *s) {
  char p[64]; snprintf(p, sizeof p, "/proc/%d/%s", pid, f);
  int fd = open(p, O_WRONLY); if (fd < 0 || write(fd, s, strlen(s)) < 0) perror(p); close(fd);
}
int main(int argc, char **argv) {
  static char stack[65536];
  int mnt = syscall(SYS_open_tree, -100, argv[1], OPEN_TREE_CLONE | OPEN_TREE_CLOEXEC);
  if (mnt < 0) { printf("%s open_tree: %s\n", argv[1], strerror(errno)); return 1; }
  int pid = clone(child, stack + sizeof stack, CLONE_NEWUSER | SIGCHLD, NULL);
  if (pid < 0) { perror("clone"); return 2; }
  waitpid(pid, NULL, WUNTRACED);
  char p[64]; snprintf(p, sizeof p, "/proc/%d/ns/user", pid);
  int ns = open(p, O_RDONLY | O_CLOEXEC);
  wr(pid, "uid_map", argc > 2 ? argv[2] : "0 0 1"); wr(pid, "setgroups", "deny"); wr(pid, "gid_map", argc > 2 ? argv[2] : "0 0 1");
  struct mount_attr at = { .attr_set = MOUNT_ATTR_IDMAP, .propagation = MS_SLAVE, .userns_fd = ns };
  int r = syscall(SYS_mount_setattr, mnt, "", AT_EMPTY_PATH, &at, sizeof at);
  printf("%s map[%s]: %s\n", argv[1], argc > 2 ? argv[2] : "0 0 1", r == 0 ? "OK" : strerror(errno));
  kill(pid, SIGKILL); waitpid(pid, NULL, 0); return r != 0;
}
