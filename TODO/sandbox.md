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
**Status:** partial

## Problem

Each host builds the guest image itself: 220 to 502 s, and the result can
differ by the day of the build.

## Done, and open

[scripts/build-image.sh](../scripts/build-image.sh) builds the image with
distrobuilder (`image/distrobuilder.yaml`), running `image/provision.sh`
unchanged, and writes `OUT_DIR/inputs`, the digest `errand-box image
import` records so setup takes the image as current (E-31). A unit test
proves the shell and the Rust compute the same digest
(`image::tests::the_distrobuilder_build_records_the_same_inputs`). Built
and imported on the build host 2026-10-23 (a 395 MiB rootfs; `image import`
checked its SHA256SUMS, recorded the inputs and moved the alias). Open:
building it in a GitHub workflow and publishing it with its digest in
`pins.toml`, so a host imports a release instead of building (waits on
Actions, T-009).

## Prove

```sh
sudo sh scripts/build-image.sh OUT_DIR && errand-box image import OUT_DIR
sh tests/live/live.sh
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

---

# T-019: An incus backend in errand

**Source:** the operator, 2026-10-23; the specification is
[docs/incus-backend.md](../docs/incus-backend.md)
**Category:** sandbox
**Priority:** P2
**Effort:** L
**Status:** blocked

## Problem

errand reaches Incus only through errand-box's stand-in for `bailey`. That
costs a policy file round trip, a digest check on a file the guest can
write, doctor lines written for Landlock, and one limit for every session,
because errand's bailey policy carries no memory, CPU or process limit.
errand then describes the sandbox with bailey's words, which are wrong here
(T-016).

## Premise

Read, at errand v0.3.1: errand's `Backend` has `probe`, `launch`,
`list_orphans` and `remove_orphans`, and the podman backend implements them
with a child process and labels (`src/sandbox/mod.rs`,
`src/sandbox/podman.rs`). An `incus` backend fits the same shape. Measured:
each behaviour the specification asks for runs in errand-box and passed its
live tests (`tests/live/2026-10-23.txt`).

## Approach

The work is in errand, which this repository does not change.
[docs/incus-backend.md](../docs/incus-backend.md) specifies the backend:
configuration, the instance, the guest image, the lifecycle, the settings,
the host prerequisites, and the acceptance tests. When errand ships it,
errand-box keeps `setup`, `up` and `maint`, writes `sandbox.backend =
"incus"` into errand's config, and stops installing the `bailey` link for
that errand version and later.

## Decision

D1 to D4 in section 10 of the specification are errand's author's.

## Blocker

errand's author: whether errand takes an `incus` backend, and the decisions
D1 to D4. Unblock: an errand release with the backend, or a ruling against
it, in which case this entry records the ruling and errand-box keeps the
stand-in.

## Prove

```sh
sh tests/live/live.sh   # with sandbox.backend = "incus" and no `bailey` on errand's PATH
```

Every check PASS.

---

# T-020: A broker link that does not need proxy devices

**Source:** the review of 2026-10-23 (security lens), measured in build
**Category:** sandbox
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

errand's project allows proxy devices, because the broker link is two
proxy devices per session (`src/session.rs` `desired_devices`; setup sets
`restricted.devices.proxy=allow`, `src/setup.rs`). Incus has no setting
that limits what a proxy device may connect to, so the errand account can
attach one whose host end is any host socket, the Incus admin socket
included. That grants the errand account more than the restricted project
is meant to (E-61). A guest cannot add devices, so this is not reachable
from inside a session; it matters when the errand account itself is
compromised.

## Premise

Measured in build on Incus 6.0.6: with `restricted.devices.proxy=allow`,
the errand account attached such a device to a throwaway instance. With
`restricted.devices.proxy=block`, Incus refused the device for the errand
account and for root alike ("Proxy devices are forbidden"), so root cannot
add the broker devices on errand's behalf in a project that forbids them.

## Approach

Carry the broker over a unix socket instead: the shim already relays the
broker through a unix socket for the rootless engine (`relay::unix_to_tcp`,
E-23). Share that socket into the guest with a disk device, which the
project already allows under errand's home, and have the guest's boot step
listen on 169.254.169.1 and relay to it, so kage and the guest's services
see the same address. Then set `restricted.devices.proxy=block`. The guest
image changes, so every host rebuilds it (E-31).

## Prove

```sh
sh tests/live/live.sh   # the broker and system-proxy checks pass with proxy devices blocked
incus project get user-UID restricted.devices.proxy   # block
```

---

# T-021: Operator-chosen session credentials, safely

**Source:** the operator, 2026-10-23: "I want agents to use any [credential]
I choose via env vars, for instance an ssh key the agent can use to ssh into
another machine"
**Category:** sandbox
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

What reaches the agent is entirely errand's per-session policy, which
errand-box trust-verifies (`src/shim.rs:262`) and then relays faithfully:
`[env] pass` names are read from bailey's own environment
(`src/shim.rs:283`, `src/policy.rs:409`) and `set` is applied verbatim, into
a 0600 file the launcher sources and deletes (`src/session.rs:390`). Two
consequences: errand's defaults flow through unfiltered (the GitHub token
reaches the guest verbatim, `docs/design.md:76`), and the operator has no
errand-box-side way to add a credential errand's policy does not already
grant. errand-box cannot simply bolt extra env or mounts on top, because the
policy is a trusted artifact signed as errand's; doing so silently would
break that trust.

## Approach: research and resolve the best compromise between safety and usability

The task is to find where operator-chosen secrets belong without weakening
the trust model. Options to measure and compare, not to assume:
- An errand-box overlay the operator signs, merged with errand's policy
  under its own trust record, so the agent's environment and mounts are the
  union and the provenance of each is auditable.
- A secrets directory (e.g. `/etc/errand-box/secrets/`, mode 0700) whose
  files errand-box mounts read-only into the session at operator-named
  guest paths (an ssh key at `/state/home/.ssh/id_ed25519`, 0600), named in
  the host config rather than in errand's per-session policy.
- Deciding what, if anything, errand-box should redact or refuse to relay
  from errand's defaults (the verbatim git token), versus leaving that to
  errand.

Each option is a point on the safety/usability line; the entry resolves to
the one that lets the operator add arbitrary credentials while keeping every
grant verifiable and least-privilege. An ssh key is the worked example: it
is useless without T-022, so the two are resolved together.

## Prove

```sh
# the chosen mechanism puts an operator secret in a session, 0600, not in
# any argv or log, and absent from a session the operator did not grant it to
sh tests/live/live.sh
```

---

# T-022: Egress a session can use beyond the HTTP broker

**Source:** the operator, 2026-10-23: the ssh-into-another-machine case
**Category:** sandbox
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

A session has no network device; its only path out is the two proxy devices
to errand's broker at `169.254.169.1:P`, under errand's host-and-port
allowlist (`src/session.rs` `desired_devices`, `docs/design.md:73`, measured:
a direct connection fails with errno 101). The guest's clients are pointed
at it as an HTTP proxy (`HTTPS_PROXY` in the policy `set`). Raw-TCP protocols
the agent may legitimately need - ssh to a machine it is diagnosing, a
database connection, a git+ssh remote - have no path: ssh is TCP/22, not
HTTP, and whether the broker tunnels it is errand's to decide, not
errand-box's.

## Approach: research and resolve the best compromise between safety and usability

Measure what each egress shape costs in containment and buys in usability,
rather than opening the network:
- A per-session allowlist of raw-TCP host:port the broker (or an errand-box
  relay beside it) tunnels, so ssh to a named host works and nothing else
  does. This is largely errand's broker; errand-box's part is the transport
  and the allowlist plumbing.
- An operator-opt-in egress profile per session (still no general NIC) that
  errand-box carries into the policy errand already shapes with
  `sandbox.network` and `--proxy-net` (`docs/bailey-contract.md:268-272`).
- Keeping the default exactly as it is (HTTP allowlist only), so broader
  egress is never silent.

The entry resolves to the narrowest mechanism that makes the operator's
named outbound connection work while every other destination still fails
closed. It is resolved with T-021 (the credential is useless without the
path, and the path is dangerous without a scoped credential).

## Prove

```sh
# with an operator-granted ssh target, the agent reaches host:22 and no other
# host; every ungranted raw-TCP connection still fails
sh tests/live/live.sh
```
