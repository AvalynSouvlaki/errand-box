#!/bin/sh
# watch-upstreams.sh - is every upstream that `errand-box setup` reads still
# there?
#
# The defect this exists to find: a host's setup fails because something
# upstream moved, and the first to notice is an operator. It runs from
# .github/workflows/watchdog.yml every day, and by hand.
#
# Checks, each one line, PASS or FAIL:
#   zabbly-<channel>  the Zabbly Incus repository answers for each channel
#                     setup chooses (stable, lts-6.0), on the Ubuntu and
#                     Debian suites it supports
#   images            the image server lists Debian 13 (trixie), the base of
#                     the guest image, for amd64 and arm64
#   pins              each URL of pins.toml answers (the release asset is
#                     still there)
#
# Usage: sh scripts/watch-upstreams.sh [OUT_DIR]
#   OUT_DIR gets one <check>.md per failed check, for the issue body.
# Needs: curl.
# Exit codes: 0 all present, 1 one or more gone, 2 could not run.
set -u
root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-}
[ -z "$out" ] || mkdir -p "$out" || exit 2
command -v curl >/dev/null 2>&1 || { echo "watch-upstreams: curl is missing" >&2; exit 2; }
failed=0

result() { # name, 0 or 1, detail
  if [ "$2" = 0 ]; then
    printf 'PASS %s: %s\n' "$1" "$3"
  else
    printf 'FAIL %s: %s\n' "$1" "$3"
    failed=1
    [ -z "$out" ] || printf '%s\n\nMeasured by %s on %s.\n' "$3" "scripts/watch-upstreams.sh" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$out/$1.md"
  fi
}

head_ok() { curl -fsSL --retry 3 -o /dev/null -r 0-0 "$1" 2>/dev/null; }

for channel in stable lts-6.0; do
  bad=""
  for suite in noble jammy trixie bookworm; do
    head_ok "https://pkgs.zabbly.com/incus/$channel/dists/$suite/Release" || bad="$bad $suite"
  done
  [ -z "$bad" ]
  result "zabbly-$channel" $? "pkgs.zabbly.com/incus/$channel: Release files${bad:+ missing for:$bad}${bad:- present for noble jammy trixie bookworm}"
done

idx=$(curl -fsSL --retry 3 https://images.linuxcontainers.org/meta/1.0/index-system 2>/dev/null)
bad=""
for arch in amd64 arm64; do
  printf '%s\n' "$idx" | grep -q "^debian;trixie;$arch;default;" || bad="$bad $arch"
done
[ -z "$bad" ]
result images $? "images.linuxcontainers.org: debian/13 (trixie)${bad:+ missing for:$bad}${bad:- listed for amd64 and arm64}"

bad=""
n=0
urls=$(sed -n 's/^url = "\(.*\)"$/\1/p' "$root/pins.toml")
for u in $urls; do
  n=$((n + 1))
  head_ok "$u" || bad="$bad $u"
done
[ -z "$bad" ] && [ "$n" -gt 0 ]
result pins $? "pins.toml: $n URLs${bad:+; gone:$bad}${bad:-, every one answers}"
exit "$failed"
