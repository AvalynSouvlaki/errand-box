# Setup and the host

Entries about `errand-box setup`, `up`, the supervisor and the hosts they
run on. [INDEX.md](INDEX.md) lists every entry.

---

# T-001: One command sets up a host

**Source:** the operator (next-session.md section 2 of the agent-box research)
**Category:** setup
**Priority:** P0
**Effort:** L
**Status:** done

## Problem

A host needs Incus, a storage pool, a guest image, kage, errand, errand's
config and a supervisor before errand can start one session. By hand that
is an afternoon, and each host differs.

## Premise

Measured: `errand-box setup --config F --env F --creds F` does all of it on
a host with no init and cgroup v1, and a second run changes nothing.

## Approach

`src/setup.rs` runs each step and reports `ok`, `changed`, `DEGRADED`,
`FAILED` or `skipped`. Each step checks before it acts.

## Prove

```sh
errand-box setup --config examples/errand-box.toml --env E --creds C; echo "exit $?"
errand-box setup --config examples/errand-box.toml --env E --creds C | tail -n 1
```

Exit 0 both times, and the second run says `0 changed`.

## Closing

**Closed 2026-10-23T08:40Z.** Built and run on the build host (Ubuntu
24.04, no init, cgroup v1). Twelve live runs while the code changed; the
last one rebuilt the image because its inputs had changed (E-31):

```text
guest image              changed   errand-box/debian-13 was built from other inputs; rebuilding (E-31)
guest image              changed   built errand-box/debian-13 in 220 s
golden instance          changed   eb-golden from errand-box/debian-13 (never started)
6 changed, 30 unchanged, 5 degraded, 0 failed, 0 skipped
exit 0
```

then twice with the static release binary
(`target/x86_64-unknown-linux-musl/release/errand-box`, Rust 1.99.1): the
first run installs it, the second changes nothing.

```text
1 changed, 32 unchanged, 5 degraded, 0 failed, 0 skipped
exit 0
0 changed, 33 unchanged, 5 degraded, 0 failed, 0 skipped
exit 0
```

---

# T-003: The daemons run, and come back, on a host with no init

**Source:** the operator; E-01 and E-02 in docs/edge-cases.md
**Category:** setup
**Priority:** P0
**Effort:** M
**Status:** done

## Problem

On a host whose PID 1 is not an init, nothing starts incusd or restarts it,
and a restart loses every daemon, device mode and kernel setting.

## Premise

Measured: s6 from the distribution supervises the daemons, and `errand-box
up` restores all of it. Never signal a daemon by name (E-12).

## Approach

`src/up.rs`: device modes, sysctls, the proxy setting, one s6 service per
daemon with a bounded logger, a check for a daemon that restarts in a loop,
and a second pass when s6-svscan exits under the first (E-33).

## Prove

```sh
sh tests/live/live.sh up
```

Every check PASS, exit 0.

## Closing

**Closed 2026-10-23T08:40Z.** The first live run found that `up` trusted a
zombie s6-svscan and started nothing (E-33); the fix treats a zombie as
dead and runs a second pass. After it:

```text
PASS up: daemons stopped through s6-svscanctl, as a restart loses them
PASS up: errand-box up exit 0 in 5617 ms
PASS up: incusd answers
PASS up: /dev/fuse mode 666 (E-03)
PASS up: service incusd: up (pid 5155) 5 seconds
PASS up: a second up changes nothing: 0 changed, 12 unchanged, 1 degraded, 0 failed, 0 skipped
```

---

# T-006: The config setup writes is one the pinned errand accepts

**Source:** found by the live `errand` check, 2026-10-23
**Category:** setup
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

errand v0.3.1 refused the config and its supervisor restarted it every
second: "output.postImages is not a setting". The generator was written
against a later errand.

## Premise

Measured: errand checks its config before anything else and logs
"egress is brokered" once the config is accepted.

## Approach

`src/errandcfg.rs` no longer writes the key. `errand-box errand-config`
writes the config alone, and `tests/errand-config-test.sh` runs the pinned
errand against it. The gate runs it as the step `errand-config`, so a pin
bump that changes errand's schema fails before it reaches a host.

## Prove

```sh
sh scripts/ci-local.sh --step errand-config
```

## Closing

**Closed 2026-10-23T08:40Z.** With the key put back, the check fails; without
it, it passes:

```text
FAIL errand v0.3.1 refuses the config errand-box writes:
... ConfigError: configuration rejected: - output.postImages is not a setting; check the spelling
PASS errand v0.3.1 accepts the config errand-box writes
```

---

# T-008: Bounded logs, and a daemon that restarts in a loop is reported

**Source:** found while closing T-006
**Category:** setup
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

All daemons wrote to one unbounded file, and `up` reported a daemon that
restarted every second as `up (pid N) 0 seconds`, which reads as healthy.

## Approach

`src/supervise.rs` `s6_log_run`: each daemon logs through `s6-log` to
`/var/log/errand-box/<daemon>/`, ten files of 1 MB. `src/up.rs` looks at a
daemon twice, 2.5 s apart, and reports a changed pid as FAILED with the last
log line.

## Prove

```sh
cargo test --locked supervise
sh tests/live/live.sh up
```

## Closing

**Closed 2026-10-23T08:40Z.** `supervise::tests::s6_log_is_bounded` and
`supervise::tests::svstat_lines` pass; `ps` on the host shows one `s6-log -b
n10 s1000000 T /var/log/errand-box/<daemon>` per daemon, and the live `up`
test passes.

---

# T-010: Setup without root at run time

**Source:** docs/research/rootless.md section 3, definition A, rank 1
**Category:** setup
**Priority:** P2
**Effort:** L
**Status:** open

## Problem

errand-box needs a root incusd. Some operators want no root process at run
time.

## Premise

Measured in the research: incusd as root of a user namespace runs guests at
runc speed with every feature but the disk quota; new instances take 1 to
3.5 s; the broker needs a unix-socket relay, which the shim already has
(`broker_link = "unix"`, E-23). On cgroup v1 with no init, a root step at
each boot makes the delegated cgroups.

## Approach

`engine.mode = "rootless"` in `src/config.rs`; a setup path that writes the
private subuid file, the delegated cgroups and a small init as PID 1 of the
namespace (research/experiments/rootless/), and records `incus_dir` and
`broker_link = "unix"` in runtime.toml.

## Prove

```sh
sh tests/live/live.sh   # with engine.mode = "rootless"; quota reports DEGRADED (E-24)
```

---

# T-011: Live test on a host with systemd

**Source:** this session; the build host has no init
**Category:** setup
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

`systemd_up` in `src/up.rs` writes units and enables the packaged ones. No
live run had covered it.

## Done

2026-10-23, Ubuntu 24.04 as a nested host with systemd as PID 1
([tests/hosts/host.sh](../tests/hosts/host.sh)): bootstrap, setup twice
(idempotent but for errand's dummy credentials, T-015), the live tests (39
PASS, the quota test skipped on a nested dir pool), a container restart with
systemd bringing the sandbox back on its own, and a probe and a session
after the restart (17 PASS). The run drove out real defects on systemd,
each with an edge-case row and a fix: E-34 (start order), E-37 (the
packaged lxcfs unit skipped in a container, and stopping it stopping
incusd), E-13 (incusd's proxy through `core.proxy_*`), and the unit settle
check.

## Prove

```sh
sh tests/hosts/host.sh u2404 ubuntu/24.04
```

Every check PASS (errand itself needs real credentials, T-015).

---

# T-012: Live test on musl and on WSL2

**Source:** the operator: "any Linux host (glibc, musl, WSL2)"
**Category:** setup
**Priority:** P1
**Effort:** M
**Status:** partial

## Problem

`src/setup.rs` has an apk branch for Alpine, and `src/host.rs` detects
WSL2. WSL2 has not run on such a host.

## Done, and open

2026-10-23, Alpine 3.22 as a nested host (musl, OpenRC, busybox): setup
installs Incus and s6, creates the `incus-admin` and `incus` groups the
package does not (E-35), writes the OpenRC boot hook (E-36), and `errand-box
up` brings every daemon back after a restart. The run drove out E-35 and
E-62 (newer `s6-svstat` prints `pid N pgid N`, which the crash-loop check
misread). Open: a nested host cannot build the guest image, because that
launches a third level of container nesting that the test host's kernel
refuses (`forkstart` exit 1), so the full live suite has not run on musl;
and WSL2 is untouched. A real (not triply nested) Alpine host, or a nested
host that imports the image (`errand-box image import`) instead of building
it, would finish musl.

## Prove

```sh
sh tests/hosts/host.sh alpine alpine/3.22
sh tests/live/live.sh   # on a real Alpine host, and on WSL2 with Ubuntu
```

All PASS.

---

# T-013: Live test on aarch64

**Source:** this session; pins.toml carries aarch64 assets that no run used
**Category:** setup
**Priority:** P2
**Effort:** S
**Status:** partial

## Done, and open

The `cross` step of [scripts/ci-local.sh](../scripts/ci-local.sh) builds the
static binary for aarch64 with `rust-lld` and runs every unit and
integration test there under `qemu-aarch64-static` (106 + 6 pass). That
proves the code on aarch64, not a host: no aarch64 machine has run setup or
the live tests, which is what stays open.

## Prove

```sh
sh scripts/ci-local.sh --step cross   # done
sh tests/live/live.sh                 # open: on an aarch64 host
```

All PASS.

---

# T-018: Choose a pool size that fits the host disk

**Source:** E-32, found 2026-10-23: a 20 GiB pool on a disk with 14 GiB free
**Category:** setup
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`pool_size` in `src/setup.rs` made a pool of at least 20 GiB, even when the
host disk had less free space. When the disk fills, every guest gets I/O
errors.

## Done

setup reports the condition as DEGRADED with both numbers (E-32), the auto
size is half the free space with a floor of three times `box.disk` (so one
session cannot fill the pool), and `errand-box status` reports the pool and
host-disk use, degraded at 85 % and failed at 95 %.

## Prove

```sh
cargo test --locked setup
errand-box status
```

---

# T-023: Observability: metrics over time, and session traces

**Source:** the operator, 2026-10-23: "best-in-class observability/tracing
logs ... beyond errand/kage, something like system-info-collector"
**Category:** setup
**Priority:** P2
**Effort:** L
**Status:** open

## Problem

errand-box has operational logging and a point-in-time snapshot, nothing
over time. It ships: bounded per-daemon s6 logs
(`/var/log/errand-box/<daemon>/current`, `src/supervise.rs`), `errand-box
logs` (`src/ops.rs`), journald for the units on systemd, and `errand-box
status` (daemons, pool %, host-disk %, image, golden, session counts,
`src/status.rs`). A grep of `src/` for `metric|telemetry|tracing|otel|
prometheus|statsd|/proc/stat|cgroup.*stat` returns nothing: no time series,
no per-session resource accounting, no tracing, no export. An operator
cannot answer "what did this session use, and when" or "what was the pool
doing an hour ago."

## Premise

READ, not yet built: each session is a cgroup-delegated Incus instance, so
per-session CPU, memory, pids and io are readable from `/sys/fs/cgroup` and
the Incus API without errand's help, and `maint` already runs on a loop
(`src/maint.rs`) that could sample. The host daemons' state is already
computed by `status`.

## Approach

A sampler (in `maint`, or its own supervised service) writes a bounded
time series of host and per-session resource use - the shape
[system-info-collector](https://github.com/qarmin/system-info-collector)
takes, CSV or a small on-disk store, not a daemon to operate - and a
read command renders or exports it (`errand-box status --watch`, or an
export for a TSDB the operator already runs). Session lifecycle events
(start, stop, reap, gc, image rebuild) become structured records a trace can
be built from. Decide the retention and the on-disk budget the way the logs
are bounded (E-08-style: never fill the disk).

Safety: the sampler records resource numbers and lifecycle events, never the
contents of a session, its environment or its files; a trace must not become
a place secrets land (T-021).

## Prove

```sh
# after some session activity, the series shows per-session CPU and memory
# over time and the lifecycle events, bounded on disk, with no secret in it
errand-box status --watch   # or the chosen read/export path
```
