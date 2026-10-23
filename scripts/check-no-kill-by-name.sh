#!/bin/sh
# check-no-kill-by-name.sh - does any code signal a process by its name?
#
# The defect it catches: every Incus guest's LXC monitor process is also
# named `incusd`. `pkill incusd`, `killall incusd` or `kill $(pgrep -x
# incusd)` stops the daemon AND every guest. Measured during the research
# (docs/edge-cases.md, E-12). errand-box stops daemons only through their
# supervisor, which signals the pid it started.
#
# Usage: sh scripts/check-no-kill-by-name.sh [--json]
# Exit codes: 0 clean, 1 a match, 2 could not run.
# Read the exit code from this process, unpiped.
set -u
cd "$(dirname "$0")/.." || exit 2
json=0
[ "${1:-}" = "--json" ] && json=1
command -v git >/dev/null 2>&1 || { echo "check-no-kill-by-name: git is missing" >&2; exit 2; }
files=$(git ls-files -co --exclude-standard -- 'src/*' 'image/*' 'scripts/*' 'bootstrap.sh' 'tests/*' 2>/dev/null | grep -v '^scripts/check-no-kill-by-name.sh$')
[ -n "$files" ] || { echo "check-no-kill-by-name: no files to check" >&2; exit 2; }
# shellcheck disable=SC2086
# Code lines only: Markdown and comment lines may name the commands.
hits=$(printf '%s\n' "$files" | grep -v '\.md$' | tr '\n' '\0' \
  | xargs -0 grep -nE '(^|[^A-Za-z_-])(pkill|killall)([^A-Za-z_-]|$)|kill[^\n]*\$\(pgrep' 2>/dev/null \
  | grep -vE '^[^:]+:[0-9]+:[[:space:]]*(#|//)' || true)
if [ -n "$hits" ]; then
  if [ $json = 1 ]; then
    printf '{"check":"no-kill-by-name","ok":false,"hits":%s}\n' "$(printf '%s' "$hits" | wc -l)"
  else
    printf 'check-no-kill-by-name: a process is signalled by name (E-12):\n%s\n' "$hits"
  fi
  exit 1
fi
if [ $json = 1 ]; then echo '{"check":"no-kill-by-name","ok":true}'; else echo "check-no-kill-by-name: clean"; fi
exit 0
