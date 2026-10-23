# errand-box and sandhome

[sandhome](https://github.com/talaria0101/sandhome) (read at commit
5792a3e) is a home for an agent inside a sealed sandbox: one bootstrap that
finds a directory that can run programs, installs toolchains there, and
works around a sandbox with no pty, no passwd entry and no bind mounts. It
was written inside errand's own bailey sandbox, where the session's home
(`/state/home`) can be written but cannot run programs.

errand-box solves a different problem: it replaces the seal. This page
compares the two, with each claim measured inside an errand-box session on
2026-10-23 (the host of [tests.md](../tests.md)), or marked READ.

## The conditions sandhome works around, inside an errand-box session

| condition in a sealed sandbox (READ, sandhome README) | inside errand-box, measured |
| --- | --- |
| the home can be written and cannot run a program | `/state/home` and `/workspace` both run a program; `sandhome space --probe` says `exec=yes` for both |
| no pty | stdin is not a tty (errand speaks ACP on it, by design), but a program gets a pty: `script -qec tty` prints `/dev/pts/1` |
| no passwd entry for the uid | uid 1000 is `agent`, home `/home/agent` |
| no bind mount | `sudo mount --bind` works |
| ptrace refused | `strace -f true` works |
| tools must be fetched into the home | the image has them: sandhome adopted node, python, perl, ripgrep and the C tool chain instead of fetching them |

## sandhome inside errand-box

`sh bootstrap.sh --toolset minimal` in a session: exit 0 in 2 s, 0
failures. `sandhome doctor`: 15 checks pass, 1 fails:
`global_hook=stale:/state/home/bin`. errand's policy puts
`/state/home/bin` on PATH, and a new session has no such directory;
`sandhome global` is its repair. Nothing in errand-box needs to change for
it.

## The answer

The two are complementary, not alternatives. sandhome makes a sealed
sandbox usable; errand-box makes the sandbox a whole system, so the seal's
restrictions are gone and the host boundary is the container's. An agent in
errand-box does not need sandhome, and sandhome still works there.

Two things errand-box takes from sandhome:
- its documentation check (`tests/docs.sh`: a flag or a path that a page
  names must exist), entry T-017;
- its one-paste entry point (`ROUTE.md`) is how errand-box's own
  `bootstrap.sh` is meant to be used: one command with nothing installed.
