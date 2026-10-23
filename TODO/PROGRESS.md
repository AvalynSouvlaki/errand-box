# Progress

Read this first. It has the baseline, what the last session did, and the
work order. It carries no history: every session rewrites it. The history
is the git log and the entries.

How this repository is worked on: [`RULES.md`](RULES.md). Every entry, one
line each: [`INDEX.md`](INDEX.md). Routing for an agent:
[`AGENTS.md`](../AGENTS.md).

---

## State

- **Last session:** 2026-10-23T05:28Z, attended at the start, then
  unattended.
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
| `sh scripts/ci-local.sh` | every step passed: gate, kill-by-name, shellcheck, fmt, clippy, test, pins, errand-config, musl |
| `cargo test --locked` | 82 unit tests and 6 integration tests pass, on Rust 1.99.1 |
| `sh tests/live/live.sh` on the build host | 52 of 52 PASS ([`tests/live/2026-10-23.txt`](../tests/live/2026-10-23.txt)) |
| `actionlint .github/workflows/*.yml` | clean |

The build host: Ubuntu 24.04 in a hosted container, kernel 6.18, x86_64, 4
CPUs, 16 GiB, cgroup v1, no init, no KVM, egress through an HTTP proxy;
Incus 6.0.6 from Zabbly, errand v0.3.1, kage v0.5.0.

**19 entries: 7 done, 2 partial, 2 blocked, 8 open.**

---

## What this session did

- Checked the research of the agent-box repository on a second host
  ([`docs/research/engines.md`](../docs/research/engines.md)), with its open
  questions explained: Incus 6.0.0 rewrites a rootfs because it never writes
  the map of the user namespace it probes with (fixed upstream); on 6.0.6 a
  golden copy takes 0.44 to 0.54 s.
- Measured and ranked every route without root, under both definitions
  ([`docs/research/rootless.md`](../docs/research/rootless.md)), and
  corrected four cells of the proposed VMM matrix.
- Built errand-box: setup, `up` with s6, the bailey shim, the guest image,
  maintenance, 33 edge cases with fallbacks and tests, the scheduled
  workflows, and the local CI runner. Closed T-001 to T-006 and T-008.
- Compared it with sandhome, by running sandhome inside a session
  ([`docs/research/sandhome.md`](../docs/research/sandhome.md)).
- Wrote [`docs/incus-backend.md`](../docs/incus-backend.md), a specification
  of an `incus` backend in errand, for errand's author; entry T-019, blocked
  on that author.
- Moved the toolchain to Rust 1.99.1, which `scripts/freshness.sh
  --dry-run` reported as newer than the pinned 1.99.0.

Premises that a measurement disproved:

- Believed: an agent in the guest can use Docker because it is in the
  `docker` group. Measured: `incus exec --user` gives no supplementary
  groups (E-29).
- Believed: rootless podman needs only subordinate ids. Measured: pasta
  also needs a default route, which the guest does not have (E-30).
- Believed: a `pid:starttime` lease shows whether s6-svscan runs. Measured:
  a zombie keeps both, and this host's PID 1 never reaps (E-33).
- Believed: the errand config generator matched the pinned errand. Measured:
  errand v0.3.1 refuses `output.postImages`, a key of a later errand.
- Believed: kage's digests could only be recorded on first use. Measured:
  GitHub records a digest for each release asset, and both pins match it.

## What is in progress

Nothing. T-007 and T-018 are partial; each entry names its open half.

---

## Start here next session

1. T-009, `TODO/upkeep.md`: turn Actions on, push the tag `v0.1.0`, check
   the release, then `bootstrap.sh` on a host with no checkout.
2. T-007, `TODO/upkeep.md`: run `freshness.yml` and `watchdog.yml` once by
   hand on GitHub, and read the pull requests and issues they open.
3. T-011, `TODO/setup.md`: the live tests on a host with systemd, before
   and after a reboot.
4. T-015, `TODO/sandbox.md`: when the operator has given the chat and
   gateway credentials, a real session.
5. T-012 and T-013, `TODO/setup.md`: Alpine, WSL2, aarch64.

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

---

## Settled, and not to be raised again

See [`RULES.md`](RULES.md) section 4: Incus system containers; errand
unchanged behind `bailey`; root at setup; s6 from the distribution.
