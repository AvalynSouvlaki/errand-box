# Progress

Read this first. It has the baseline, what the last session did, and the
work order. It carries no history: every session rewrites it. The history
is the git log and the entries.

How this repository is worked on: [`RULES.md`](RULES.md). Every entry, one
line each: [`INDEX.md`](INDEX.md). Routing for an agent:
[`AGENTS.md`](../AGENTS.md).

---

## State

- **Last session:** 2026-10-23T10:28Z, unattended: a hardening, QOL and
  host-matrix pass over the build of the earlier session the same day.
- **Tree:** clean after the session's commit on `main`.
- **Deployed:** not released; no tag. errand-box is installed on the build
  host only, which is a hosted container that is taken back after the
  session.
- **CI:** off on this repository. Every step ran through
  `sh scripts/ci-local.sh` (below).

## Baseline, as measured this session

Measure again with the same commands before you change anything.

| check | result |
| --- | --- |
| `sh scripts/ci-local.sh` | every step passed: gate, kill-by-name, shellcheck, fmt, clippy, test, pins, errand-config, musl, cross |
| `cargo test --locked` | 106 unit tests and 6 integration tests pass, on Rust 1.99.1 |
| `sh scripts/ci-local.sh --step cross` | the aarch64 build and all 106 + 6 tests pass under `qemu-aarch64-static` |
| `sh tests/live/live.sh` on the build host | 52 of 52 PASS |
| `sh tests/hosts/host.sh u2404 ubuntu/24.04` | every check PASS (systemd, T-011); Debian and Alpine drove out defects but cannot build the image in triple nesting |
| `actionlint .github/workflows/*.yml` | clean |

The build host: Ubuntu 24.04 in a hosted container, kernel 6.18, x86_64, 4
CPUs, 16 GiB, cgroup v1, no init, no KVM, egress through an HTTP proxy;
Incus 6.0.6 from Zabbly, errand v0.3.1, kage v0.5.0.

**23 entries: 9 done, 4 partial, 2 blocked, 8 open.**

---

## What this session did

An adversarial review (six lenses, a workflow of subagents) and a nested
host matrix found defects the single-host build could not. The fixes, the
QOL commands and the record:

- **Start-up and supervision.** Each daemon now waits for the ones it needs
  (`errand-box __wait`), and `up` watches them all together until each keeps
  one pid (E-34). The crash-loop check reads the newer `s6-svstat` format
  `pid N pgid N` (E-62). Under systemd, `up` verifies each unit stays up
  (`systemctl show`), sets incusd's egress proxy through Incus `core.proxy_*`
  (E-13), and handles the packaged lxcfs unit with a drop-in that never
  stops it  -  stopping it stopped incusd, whose `Requires=` it (E-37).
- **Other init systems.** setup creates the `incus-admin` and `incus` groups
  the Alpine package omits (E-35) and writes an OpenRC boot hook (E-36); the
  daemons come back after a restart on Ubuntu, Debian and Alpine.
- **New commands.** `status` (daemons, pool, disk, image, golden, sessions),
  `shell`, `logs`, `uninstall [--purge]`, `image import`, `gc --dry-run`,
  and `--json` for setup, up, status, doctor and image.
- **Lifecycle and safety.** gc and reap no longer stop the whole pass on one
  instance (E-57), keep their own lock shared with the launcher (E-40), and
  gc deletes by age only when errand's state root is missing (E-40). A run
  no longer reuses a running orphan (E-56). maint keeps its trim and gc
  times across restarts (E-41) and rereads the config each pass. `up` falls
  back to the last applied config when the live one will not parse (E-42).
- **Security.** setup's chown under errand's home no longer follows a
  planted symlink (E-54); proxy.env is 0600, quoted, and refuses shell
  metacharacters (E-46); the Zabbly keyring must hold exactly the pinned key
  (E-55); the profile sets `limits.processes` and `bailey doctor` reports
  the limits the profile really holds (E-63). The one gap found and not
  closed: errand's project must allow proxy devices for the broker, which
  lets the errand account reach the admin socket (E-61, entry T-020).
- **The image.** `scripts/build-image.sh` builds the guest with distrobuilder
  and records an inputs digest; `errand-box image import` checks it against
  SHA256SUMS and records that digest, so setup takes it as current (E-31).
- **aarch64.** The `cross` CI step builds for the other architecture with
  `rust-lld` and runs every test under qemu-user.

Closed T-011 and T-018; T-012, T-013 and T-014 to partial, each entry
naming its open half; E-34 to E-65 in [edge-cases.md](../docs/edge-cases.md).
Filed, from the operator's questions, T-021 (operator-chosen session
credentials), T-022 (egress a session can use beyond the HTTP broker) and
T-023 (observability: metrics over time and session traces); the first two
are to be resolved together, as the best compromise between safety and
usability.

Premises that a measurement disproved:

- Believed: disabling the packaged lxcfs unit is a safe way to run our own.
  Measured: Zabbly's incus.service `Requires=` it, so stopping it stopped
  incusd mid-build; a drop-in is the only safe way (E-37).
- Believed: `s6-svstat` prints `up (pid N)`. Measured: s6 2.13 on Alpine
  prints `up (pid N pgid N)`, which the pid parser could not read, so every
  daemon looked like a crash loop (E-62).
- Believed: the errand account is not root-equivalent, held to its project.
  Measured: a proxy device, which the project must allow for the broker, can
  connect to the admin socket (E-61).

## What is in progress

Nothing. T-007, T-012, T-013 and T-014 are partial; each entry names its
open half.

---

## Start here next session

1. T-020, `TODO/sandbox.md`: a broker link without proxy devices, so
   errand's project can forbid them and the errand account loses its path to
   the admin socket (E-61). The highest open item.
2. T-009, `TODO/upkeep.md`: turn Actions on, push the tag `v0.1.0`, check
   the release, then `bootstrap.sh` on a host with no checkout.
3. T-014, `TODO/sandbox.md`: build the distrobuilder image in a workflow and
   publish it with its digest in `pins.toml` (waits on Actions, T-009).
4. T-015, `TODO/sandbox.md`: when the operator has given the chat and
   gateway credentials, a real session.
5. T-021 and T-022, `TODO/sandbox.md`: resolve operator-chosen credentials
   and the egress to use them (the ssh-to-another-machine case) together.
6. T-023, `TODO/setup.md`: the observability sampler and session traces.
7. T-012 and T-013, `TODO/setup.md`: a real Alpine host and WSL2 (the full
   live suite), and an aarch64 host.

---

## Open questions for the operator

1. **Credentials for T-015.** A Discord bot token, a channel id and a user
   id; a model gateway key and base URL; optionally a GitHub token of a
   throwaway account. [`SECURITY.md`](../SECURITY.md) lists the permissions.
   Put them in the environment's secrets, not in a chat. Recommendation:
   a test server and a gateway key with a spending limit.
2. **Actions.** CI and the scheduled workflows are written and linted but
   off. Recommendation: turn Actions on, with "Allow GitHub Actions to
   create and approve pull requests", and add `FRESHNESS_TOKEN` so that CI
   runs on the pull requests freshness opens.
3. **The broker's proxy devices (E-61, T-020).** errand's project allows
   proxy devices because the broker link needs them, and a proxy device can
   reach the host's admin socket, so a compromised errand account is root. A
   guest cannot reach this. Recommendation: T-020 replaces the broker link
   with a unix socket shared through a disk device, so the project can
   forbid proxy devices. Until then, SECURITY.md states the gap.

---

## Settled, and not to be raised again

See [`RULES.md`](RULES.md) section 4: Incus system containers; errand
unchanged behind `bailey`; root at setup; s6 from the distribution.
