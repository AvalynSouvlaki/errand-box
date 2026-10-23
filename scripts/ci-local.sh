#!/bin/sh
# ci-local.sh - run every step of .github/workflows/gates.yml on this machine.
#
# The defect this exists to stop: CI that is off (it is off on this
# repository) and a push that nobody checked. gates.yml calls this script one
# step at a time, so the two cannot drift: a step that passes here passes
# there, on the same runner image.
#
# Usage:
#   sh scripts/ci-local.sh                 every step, in order
#   sh scripts/ci-local.sh --step NAME     one step
#   sh scripts/ci-local.sh --list          the step names
#
# Steps: gate kill-by-name shellcheck fmt clippy test pins errand-config musl.
# `musl` builds the static release binary for this machine's architecture.
# The live tests (tests/live/live.sh) need a host with Incus and are not a
# step here; docs/tests.md says when to run them.
#
# Needs: cargo (the toolchain of rust-toolchain.toml), shellcheck, curl.
# CARGO_BUILD_JOBS defaults to 4.
# Exit codes: 0 every step passed, 1 a step failed, 2 could not run.
set -u
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root" || exit 2
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
STEPS="gate kill-by-name shellcheck fmt clippy test pins errand-config musl"

# The shell files this repository ships. research/ is a record of what was
# run and is not linted.
shell_files() {
  printf '%s\n' bootstrap.sh image/provision.sh image/launch image/guest-init tests/fake-incus \
    tests/pins-test.sh tests/errand-config-test.sh tests/live/live.sh scripts/ci-local.sh scripts/pins.sh \
    scripts/check-no-kill-by-name.sh scripts/check-todo.sh scripts/track-issue.sh \
    scripts/freshness.sh scripts/watch-upstreams.sh
}

musl_target() {
  case $(uname -m) in
    x86_64 | amd64) echo x86_64-unknown-linux-musl ;;
    aarch64 | arm64) echo aarch64-unknown-linux-musl ;;
    *) echo "" ;;
  esac
}

step() {
  case $1 in
    gate) sh scripts/common/check-gate.sh --strict && sh scripts/check-todo.sh ;;
    kill-by-name) sh scripts/check-no-kill-by-name.sh ;;
    shellcheck)
      command -v shellcheck >/dev/null 2>&1 || { echo "shellcheck is missing" >&2; return 2; }
      # shellcheck disable=SC2046 # one file per word; no names have spaces
      shellcheck -s sh $(shell_files)
      ;;
    fmt) cargo fmt --all -- --check ;;
    clippy) cargo clippy --locked --all-targets -- -D warnings ;;
    test) cargo test --locked ;;
    pins) sh tests/pins-test.sh ;;
    # Downloads the pinned errand: the one step that needs the network.
    errand-config) cargo build --locked && sh tests/errand-config-test.sh ;;
    musl)
      t=$(musl_target)
      [ -n "$t" ] || { echo "no musl target for $(uname -m)" >&2; return 2; }
      cargo build --locked --release --target "$t" || return 1
      bin="target/$t/release/errand-box"
      # A static binary has no interpreter; `file` says so where it exists.
      if command -v file >/dev/null 2>&1; then
        file "$bin" | grep -q 'static' || { echo "$bin is not static: $(file "$bin")" >&2; return 1; }
      fi
      "$bin" version
      ;;
    *) echo "unknown step: $1 (steps: $STEPS)" >&2; return 2 ;;
  esac
}

case ${1:-} in
  --list) for s in $STEPS; do echo "$s"; done; exit 0 ;;
  --step)
    [ -n "${2:-}" ] || { echo "usage: ci-local.sh --step NAME" >&2; exit 2; }
    step "$2"
    rc=$?
    [ "$rc" -le 1 ] || exit 2
    exit "$rc"
    ;;
  "") ;;
  *) echo "usage: ci-local.sh [--step NAME | --list]" >&2; exit 2 ;;
esac

failed=0
summary=""
for s in $STEPS; do
  printf '== %s\n' "$s"
  t0=$(date +%s)
  step "$s"
  rc=$?
  summary="$summary$(printf '%-14s exit %s  %4s s' "$s" "$rc" "$(($(date +%s) - t0))")
"
  [ "$rc" = 0 ] || failed=1
done
printf '\n%s' "$summary"
[ "$failed" = 0 ] && echo "ci-local: every step passed" || echo "ci-local: a step failed"
exit "$failed"
