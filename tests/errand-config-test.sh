#!/bin/sh
# errand-config-test.sh - the config.json errand-box writes is one that the
# pinned errand accepts.
#
# errand v0.3.1 refused a key that the generator wrote for a later errand
# (`output.postImages`), and its supervisor restarted it every second. This
# check finds that class of defect before setup does: it builds the config
# from tests/data with `errand-box errand-config`, downloads the pinned errand
# for this machine (digest checked against pins.toml), and runs `errand run`
# with that config for a few seconds. errand checks its config before it does
# anything else. The pass condition is the line errand logs after the check
# ("egress is brokered"); with no bailey on PATH it then stops.
#
# Environment: ERRAND_BOX, the errand-box binary (default target/debug).
# Needs: curl, tar, xz, sha256sum, and the network to fetch errand.
# Exit codes: 0 errand accepts the config, 1 it refuses it, 2 could not run.
set -u
root=$(cd "$(dirname "$0")/.." && pwd)
eb=${ERRAND_BOX:-$root/target/debug/errand-box}
[ -x "$eb" ] || { echo "errand-config-test: $eb is missing; run cargo build" >&2; exit 2; }
case $(uname -m) in
  x86_64 | amd64) arch=x86_64 ;;
  aarch64 | arm64) arch=aarch64 ;;
  *) echo "errand-config-test: no errand for $(uname -m)" >&2; exit 2 ;;
esac
t=$(mktemp -d) || exit 2
trap 'rm -rf "$t"' EXIT INT TERM

v=$(PINS_FILE="$root/pins.toml" sh "$root/scripts/pins.sh" pinned errand) || exit 2
field() {
  awk -v t="[errand.versions.\"$v\".$arch]" -v k="$1" '$0 == t { s = 1; next } /^\[/ { s = 0 } s && $1 == k { gsub(/"/, "", $3); print $3; exit }' "$root/pins.toml"
}
url=$(field url)
want=$(field sha256)
if [ -z "$url" ] || [ -z "$want" ]; then echo "errand-config-test: pins.toml has no errand $v for $arch" >&2; exit 2; fi
curl -fsSL --retry 3 -o "$t/errand.tar.xz" "$url" || { echo "errand-config-test: cannot download $url" >&2; exit 2; }
got=$(sha256sum "$t/errand.tar.xz" | cut -d' ' -f1)
[ "$got" = "$want" ] || { echo "errand-config-test: digest mismatch for $url" >&2; exit 1; }
mkdir -p "$t/x"
tar -xJf "$t/errand.tar.xz" -C "$t/x" || exit 2
errand=$(find "$t/x" -type f -name errand -perm -u+x | head -n 1)
[ -n "$errand" ] || { echo "errand-config-test: no errand binary in $url" >&2; exit 2; }

"$eb" errand-config --env "$root/tests/data/errand.env" --creds "$root/tests/data/creds.env" \
  --home "$t/errand-home" --out "$t/h/.config/errand/config.json" || exit 1
out=$(cd "$t" && HOME="$t/h" PATH=/usr/bin:/bin timeout 20 "$errand" run 2>&1)
case $out in
  *ConfigError* | *"configuration rejected"*)
    printf 'FAIL errand %s refuses the config errand-box writes:\n%s\n' "$v" "$(printf '%s\n' "$out" | grep -m 3 -i 'config')"
    exit 1
    ;;
  *"egress is brokered"*)
    printf 'PASS errand %s accepts the config errand-box writes\n' "$v"
    exit 0
    ;;
  *)
    printf 'FAIL errand %s stopped before its config check passed:\n%s\n' "$v" "$(printf '%s\n' "$out" | tail -n 5)"
    exit 1
    ;;
esac
