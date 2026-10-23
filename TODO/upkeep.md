# Upkeep: CI, releases and the scheduled workflows

Entries about how this repository keeps itself current. [INDEX.md](INDEX.md)
lists every entry.

---

# T-007: Scheduled workflows that open pull requests and issues

**Source:** the operator: "self-maintaining, as mirror-archlinux does"
**Category:** upkeep
**Priority:** P1
**Effort:** M
**Status:** partial

## Problem

errand, kage, Rust and the upstreams setup reads move without notice.

## Approach

`.github/workflows/freshness.yml` (weekly) runs `scripts/freshness.sh`: a
pull request per newer errand, kage or stable Rust, with the pin moved by
`scripts/pins.sh`, which checks each digest against upstream's `.sha256`
and GitHub's asset digest. `.github/workflows/watchdog.yml` (daily) runs
`scripts/watch-upstreams.sh` and keeps one issue per missing upstream with
`scripts/track-issue.sh`. Each workflow keeps the other's schedule on.

## Prove

```sh
sh tests/pins-test.sh
sh scripts/watch-upstreams.sh
GITHUB_API=https://api.github.com sh scripts/freshness.sh --dry-run
```

and one run of each workflow on GitHub.

## Closing

Partial, 2026-10-23T08:40Z. The local half passes: `tests/pins-test.sh` 5 of
5; `scripts/watch-upstreams.sh` 4 of 4 PASS; `scripts/freshness.sh
--dry-run` reports kage and errand current and Rust 1.99.1 newer than the
pinned 1.99.0; this session then moved the pin to 1.99.1. A bump of errand
from v0.3.0 to v0.3.1 reproduced the pinned
digests exactly, and matched both upstream's `.sha256` and GitHub's digest.
`actionlint` passes on every workflow. Open: Actions is off on this
repository, so no workflow has run on GitHub. Unblock: turn Actions on, set
"Allow GitHub Actions to create and approve pull requests", and run each
workflow once by hand.

---

# T-009: The first release

**Source:** this session; `bootstrap.sh` downloads a release that does not
exist yet
**Category:** upkeep
**Priority:** P1
**Effort:** S
**Status:** open

## Problem

`bootstrap.sh` outside a checkout fetches `errand-box-<arch>-linux-musl`
and `SHA256SUMS` from the latest release. There is none.

## Approach

With Actions on, push the tag `v0.1.0`; `.github/workflows/release.yml`
builds both architectures through `scripts/ci-local.sh --step musl` and
publishes the release.

## Prove

```sh
sh bootstrap.sh --dry-run && sh bootstrap.sh
```

on a host with no checkout: the digest line, then `installed`.

---

# T-017: A documentation check: every command, key and path a page names exists

**Source:** docs/research/sandhome.md (sandhome's `tests/docs.sh`)
**Category:** upkeep
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

A page can name an `errand-box` command, a config key or a path that the
code no longer has, and nothing notices.

## Approach

A script in the gate that extracts `errand-box <word>`, the keys of
`examples/errand-box.toml` and the paths in `docs/operations.md`, and checks
each against `errand-box help`, `src/config.rs` and the tree.

## Prove

```sh
sh scripts/ci-local.sh --step gate
```

fails on a page that names `errand-box frobnicate`.
