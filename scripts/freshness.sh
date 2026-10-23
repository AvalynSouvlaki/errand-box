#!/bin/sh
# freshness.sh - open a pull request for each pinned download with a newer
# upstream release, and for a newer stable Rust.
#
# The defect this exists to stop: errand-box falls behind errand, kage and
# Rust, and nobody notices until a host needs the new version. It runs from
# .github/workflows/freshness.yml every week, and by hand.
#
# For each tool in pins.toml whose upstream latest release is not the pinned
# latest, and for a stable Rust newer than rust-toolchain.toml's channel, it:
#   1. makes the branch freshness/<tool>-<version> from the current commit;
#   2. moves the pin (scripts/pins.sh bump, which checks each digest);
#   3. runs the steps that cover the change: test, and errand-config for errand;
#   4. commits, pushes the branch, and opens a pull request.
# A branch that exists on origin already is left alone: its pull request is
# open or was closed on purpose. When step 3 fails, the pull request says so
# in its title, so a reviewer sees it before CI does.
#
# Usage: sh scripts/freshness.sh [--dry-run]
#   --dry-run  print what is out of date; change nothing.
# Environment: GH_TOKEN (pull requests: write, contents: write) for the push
# and `gh pr create`; GITHUB_REPOSITORY.
# Needs: git, gh (not with --dry-run), curl, cargo.
# Exit codes: 0 done, 1 an update failed, 2 could not run.
set -u
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root" || exit 2
dry=0
[ "${1:-}" = --dry-run ] && dry=1
base=$(git rev-parse HEAD) || exit 2
failed=0
say() { printf 'freshness: %s\n' "$*"; }

latest_rust() {
  curl -fsSL --retry 3 https://static.rust-lang.org/dist/channel-rust-stable.toml |
    awk '/^\[pkg\.rust\]$/ { s = 1; next } /^\[/ { s = 0 } s && $1 == "version" && !v { gsub(/"/, "", $3); v = $3 } END { print v }'
}

# One update on its own branch. $1 tool, $2 version, $3 the command that
# makes the change, $4 the steps of ci-local.sh that cover it.
update() {
  tool=$1
  version=$2
  branch="freshness/$tool-$version"
  if [ "$dry" = 1 ]; then
    say "$tool: $version is out (would open $branch)"
    return 0
  fi
  if git ls-remote --exit-code --heads origin "$branch" >/dev/null 2>&1; then
    say "$tool: $branch exists on origin; nothing to do"
    return 0
  fi
  git checkout -q -B "$branch" "$base" || return 1
  if ! sh -c "$3" > "$RUNNER_LOG" 2>&1; then
    say "$tool: the change failed:"
    cat "$RUNNER_LOG"
    git checkout -q --detach "$base"
    return 1
  fi
  status="passed"
  for st in $4; do
    if ! sh scripts/ci-local.sh --step "$st" >> "$RUNNER_LOG" 2>&1; then
      status="FAILED at $st"
      break
    fi
  done
  git add -A pins.toml rust-toolchain.toml
  git commit -q -m "Move $tool to $version" -m "Opened by scripts/freshness.sh. Local steps: $status." || return 1
  git push -q origin "$branch" || return 1
  title="Move $tool to $version"
  [ "$status" = passed ] || title="$title ($status)"
  {
    printf 'scripts/freshness.sh found %s %s upstream.\n\n' "$tool" "$version"
    q='`'
    printf 'The steps %s%s%s of scripts/ci-local.sh %s on the runner.\n\n' "$q" "$4" "$q" "$status"
    printf 'CI does not run on a pull request that the workflow token opens. Run %ssh scripts/ci-local.sh%s on a checkout of this branch before you merge it.\n\n' "$q" "$q"
    printf 'Output, last 40 lines:\n\n%s%s%stext\n' "$q" "$q" "$q"
    tail -n 40 "$RUNNER_LOG"
    printf '%s%s%s\n' "$q" "$q" "$q"
  } > "$RUNNER_LOG.body"
  gh pr create --base main --head "$branch" --title "$title" --body-file "$RUNNER_LOG.body" || return 1
  git checkout -q --detach "$base"
  [ "$status" = passed ]
}

RUNNER_LOG=$(mktemp) || exit 2
trap 'rm -f "$RUNNER_LOG" "$RUNNER_LOG.body" "$RUNNER_LOG.tools"' EXIT INT TERM

awk '/^\[[a-z0-9_-]+\]$/ { gsub(/[][]/, ""); print }' pins.toml > "$RUNNER_LOG.tools"
while read -r tool; do
  pinned=$(sh scripts/pins.sh pinned "$tool") || { failed=1; continue; }
  up=$(sh scripts/pins.sh upstream "$tool") || { say "$tool: cannot read the upstream release"; failed=1; continue; }
  if [ "$up" = "$pinned" ]; then
    say "$tool: $pinned is current"
    continue
  fi
  steps="test"
  [ "$tool" = errand ] && steps="test errand-config"
  update "$tool" "$up" "sh scripts/pins.sh bump $tool $up" "$steps" </dev/null || failed=1
done < "$RUNNER_LOG.tools"

have=$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml)
want=$(latest_rust)
if [ -z "$want" ]; then
  say "rust: cannot read the newest stable version"
  failed=1
elif [ "$want" = "$have" ]; then
  say "rust: $have is current"
else
  update rust "$want" "sed -i 's/^channel = \"$have\"\$/channel = \"$want\"/' rust-toolchain.toml" "fmt clippy test" || failed=1
fi
exit "$failed"
