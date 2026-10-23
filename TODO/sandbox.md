# The sandbox: the shim and the guest

Entries about the `bailey` shim and the guest image. [INDEX.md](INDEX.md)
lists every entry.

---

# T-002: errand runs unchanged, each session in its own system container

**Source:** the operator (next-session.md section 2 of the agent-box research)
**Category:** sandbox
**Priority:** P0
**Effort:** L
**Status:** done

## Problem

errand's own sandboxes are host processes (bailey, with Landlock) or
hardened app containers (podman). Neither runs systemd, Docker or a package
manager.

## Premise

READ and measured: errand reaches its sandbox only through the program
`bailey` on its PATH ([docs/bailey-contract.md](../docs/bailey-contract.md));
its podman backend asks for flags a system container cannot keep (section
1.1 there).

## Approach

`src/shim.rs`, `src/session.rs`, `src/trust.rs`: the bailey calls, mapped to
an Incus instance per session, copied from a golden instance.

## Prove

```sh
cargo test --locked
sh tests/live/live.sh probe session acp errand
```

## Closing

**Closed 2026-10-23T08:40Z.** `tests/shim.rs` passes against the fake Incus;
live, errand v0.3.1 under the supervisor runs its own probe through the
shim:

```text
PASS session: session exit 0 in 4273 ms
PASS acp: initialize: result (3.33 s);session/new: result (3.34 s);launcher exit: 0 (4.41 s)
PASS errand: errand's own probe of the bailey shim passed: the backend enforces every configured guarantee
```

---

# T-004: A session's processes cannot outlive it

**Source:** docs/bailey-contract.md section 8: errand has no orphan cleanup
for this backend
**Category:** sandbox
**Priority:** P1
**Effort:** M
**Status:** done

## Approach

The shim stops the instance on SIGTERM; a watchdog in its own session stops
it when the shim's pipe closes; `maint` reaps what both miss; the profile
sets `boot.autostart=false` (E-27, E-28).

## Prove

```sh
sh tests/live/live.sh signals reap
```

## Closing

**Closed 2026-10-23T08:40Z.**

```text
PASS signals: SIGTERM to the launcher's process group: instance STOPPED after 1015 ms
PASS signals: SIGKILL to the launcher's process group: instance STOPPED after 1126 ms
PASS reap: launcher and watchdog killed: instance still RUNNING, as after a crash
PASS reap: errand-box reap: instance STOPPED
```

---

# T-005: Docker, podman and FUSE work inside a session

**Source:** the operator; found broken by the live `inside` check, 2026-10-23
**Category:** sandbox
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

The first live run: `docker` got "permission denied" on its socket, and
rootless podman failed in newuidmap, then in pasta.

## Premise

Measured: `incus exec --user` gives no supplementary groups (E-29); the
guest's 65536 ids had no subordinate range, and pasta copies a default route
the guest does not have (E-30).

## Approach

`image/launch` starts as the guest's root and drops to the agent with
`setpriv --init-groups`; `image/provision.sh` gives the agent ids 1001 to
65535 and makes slirp4netns the rootless network. The image records a digest
of its inputs, so every host rebuilds it (E-31).

## Prove

```sh
sh tests/live/live.sh inside
```

## Closing

**Closed 2026-10-23T08:40Z.**

```text
PASS inside: docker run: 13.7
PASS inside: docker build and run: built
PASS inside: sudo podman run: 13.7
PASS inside: rootless podman as the agent (E-30): 13.7
PASS inside: FUSE mount as the agent: fuse-ok fuse.rclone
PASS inside: CPUs seen: 3, limit 3
PASS inside: memory seen: 12571 MiB of 16091 MiB on the host (limits.memory 80%)
```

An image file owned by an id above 64535 still needs `sudo podman`; that is
the size of the restricted project's range, stated in E-30.

---

# T-014: A distrobuilder image, built once and published

**Source:** the operator: "a distrobuilder image is in scope"
**Category:** sandbox
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

Each host builds the guest image itself: 220 to 502 s, and the result can
differ by the day of the build.

## Approach

A distrobuilder definition that runs `image/provision.sh` unchanged (it was
written for that), built by a workflow and published with its digest in
`pins.toml`; setup imports it and falls back to the local build.

## Prove

```sh
sh tests/live/live.sh   # on a host that imported the published image
```

---

# T-015: A live session with a real chat service and model gateway

**Source:** this session
**Category:** sandbox
**Priority:** P1
**Effort:** S
**Status:** blocked

## Problem

The live tests stop where errand needs a real chat service: a fake token
passes errand's probe and fails at the chat connection.

## Blocker

The operator's credentials: a Discord bot token, a channel id and a user id,
and a model gateway key and base URL. [SECURITY.md](../SECURITY.md) lists the
permissions each needs. They go in the environment's secrets, not in a chat.

## Prove

A message in the channel starts a session; the agent edits a file in the
workspace and runs `docker run hello-world`; the thread shows both.

---

# T-016: errand's description of this backend is partly wrong

**Source:** errand's log on the build host, 2026-10-23
**Category:** sandbox
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

errand logs, for errand-box: "sessions run as confined host processes using
the host's own tools" and "outbound access is not restricted by
destination". Both are errand's fixed text for bailey. A session is a
guest with its own tools, and its only way out is errand's broker.

## Approach

Find which `bailey doctor` lines errand reads for these two notes
(`src/sandbox/bailey.rs` at the pinned errand), and whether a line the shim
can print truthfully changes them. errand stays unchanged.

## Prove

```sh
grep -c "confined host processes" /var/log/errand-box/errand/current
```

0 after the change.
