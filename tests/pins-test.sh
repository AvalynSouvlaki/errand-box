#!/bin/sh
# pins-test.sh - scripts/pins.sh against releases on file:// URLs: a bump
# writes the digests and moves latest; a digest that disagrees with
# upstream's .sha256 changes nothing; a second bump is a no-op.
# Needs: curl with file:// support, sha256sum. Exit 0 pass, 1 fail.

# `ok` only prints, so `test && ok || no` cannot run both.
# shellcheck disable=SC2015
set -u
here=$(cd "$(dirname "$0")" && pwd)
pins_sh="$here/../scripts/pins.sh"
t=$(mktemp -d) || exit 2
trap 'rm -rf "$t"' EXIT INT TERM
fail=0
no() { printf 'FAIL %s\n' "$1"; fail=1; }
ok() { printf 'PASS %s\n' "$1"; }

mkdir -p "$t/rel/v1.2.0" "$t/rel/v1.3.0" "$t/rel/v1.4.0"
for a in x86_64 aarch64; do
  echo "old $a" > "$t/rel/v1.2.0/t-1.2.0-$a.tar.xz"
  echo "new $a" > "$t/rel/v1.3.0/t-1.3.0-$a.tar.xz"
  (cd "$t/rel/v1.3.0" && sha256sum "t-1.3.0-$a.tar.xz" > "t-1.3.0-$a.tar.xz.sha256")
  echo "bad $a" > "$t/rel/v1.4.0/t-1.4.0-$a.tar.xz"
  echo "0000 t-1.4.0-$a.tar.xz" > "$t/rel/v1.4.0/t-1.4.0-$a.tar.xz.sha256"
done
{
  for tool in a b; do
    printf '[%s]\nlatest = "v1.2.0"\n' "$tool"
    for a in x86_64 aarch64; do
      printf '\n[%s.versions."v1.2.0".%s]\nurl = "file://%s/rel/v1.2.0/t-1.2.0-%s.tar.xz"\nsha256 = "00"\n' "$tool" "$a" "$t" "$a"
    done
    [ "$tool" = a ] && printf '\n'
  done
} > "$t/pins.toml"
export PINS_FILE="$t/pins.toml"

sh "$pins_sh" bump a v1.3.0 > "$t/out" 2>&1
rc=$?
[ "$rc" = 0 ] && [ "$(sh "$pins_sh" pinned a)" = v1.3.0 ] && [ "$(sh "$pins_sh" pinned b)" = v1.2.0 ] &&
  grep -q "^sha256 = \"$(sha256sum "$t/rel/v1.3.0/t-1.3.0-aarch64.tar.xz" | cut -d' ' -f1)\"$" "$t/pins.toml" &&
  ok "bump a v1.3.0 writes both digests and moves only a's latest" || no "bump a v1.3.0 (exit $rc): $(cat "$t/out")"

sh "$pins_sh" bump b v1.3.0 >/dev/null 2>&1
[ "$(grep -c '^\[b.versions' "$t/pins.toml")" = 4 ] && tail -n 1 "$t/pins.toml" | grep -q '^sha256 = "[0-9a-f]\{64\}"$' &&
  ok "bump of the last tool appends at the end of the file" || no "bump of the last tool"

cp "$t/pins.toml" "$t/before"
sh "$pins_sh" bump a v1.4.0 > "$t/out" 2>&1
rc=$?
[ "$rc" = 1 ] && cmp -s "$t/before" "$t/pins.toml" && grep -q "digest mismatch" "$t/out" &&
  ok "a digest that disagrees with upstream's .sha256 changes nothing (exit 1)" || no "mismatch (exit $rc): $(cat "$t/out")"

sh "$pins_sh" bump a v1.3.0 > "$t/out" 2>&1
rc=$?
[ "$rc" = 0 ] && cmp -s "$t/before" "$t/pins.toml" && ok "a bump to the pinned latest is a no-op" || no "no-op bump (exit $rc)"

sh "$pins_sh" bump a 'v1;rm' >/dev/null 2>&1
[ $? = 2 ] && ok "a version with a shell character is refused (exit 2)" || no "bad version not refused"
exit $fail
