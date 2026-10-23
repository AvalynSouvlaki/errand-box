#!/bin/sh
# bootstrap.sh - put errand-box on a Linux host, then hand over to it.
#
#   sh bootstrap.sh [--version vX.Y.Z] [--base-url URL] [--dry-run]
#                   [-- <errand-box setup options>]
#
# From a checkout (Cargo.toml next to this file) it builds errand-box with
# cargo. Otherwise it downloads the release binary for this machine and
# checks it against the release's SHA256SUMS before it runs anything. Then
# it installs the program where setup keeps it,
# /usr/local/lib/errand-box/bin, links /usr/local/bin/errand-box to it and,
# if options follow `--`, runs `errand-box setup` with them.
#
# --base-url (or ERRAND_BOX_RELEASE_URL) is a directory that holds the
# release assets, for a mirror or a host without GitHub; the digest check
# is the same.
#
# Needs: a POSIX shell, and curl or wget for a release; cargo for a checkout.
# Runs as root, or through sudo without a password; it never prompts.
# Exit codes: 0 done, 1 a step failed, 2 could not run (usage, no root,
# no fetcher, an unsupported machine).
set -u

repo=AvalynSouvlaki/errand-box
version=latest
base_url=${ERRAND_BOX_RELEASE_URL:-}
dry=0
dest=/usr/local/lib/errand-box/bin/errand-box
link=/usr/local/bin/errand-box
say() { printf 'bootstrap: %s\n' "$*"; }
die() { printf 'bootstrap: %s\n' "$*" >&2; exit "${2:-1}"; }

while [ $# -gt 0 ]; do
  case $1 in
    --version) [ $# -ge 2 ] || die "--version needs a value" 2; version=$2; shift 2 ;;
    --base-url) [ $# -ge 2 ] || die "--base-url needs a value" 2; base_url=${2%/}; shift 2 ;;
    --dry-run) dry=1; shift ;;
    --) shift; break ;;
    -h|--help) sed -n '2,23p' "$0"; exit 0 ;;
    *) die "unknown option $1" 2 ;;
  esac
done

run() {
  if [ $dry = 1 ]; then say "would run: $*"; return 0; fi
  "$@"
}

if [ "$(id -u)" = 0 ]; then
  as_root=""
elif command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null; then
  as_root="sudo -n"
else
  die "run as root, or give this user passwordless sudo" 2
fi

case $(uname -m) in
  x86_64|amd64) arch=x86_64 ;;
  aarch64|arm64) arch=aarch64 ;;
  *) die "unsupported machine $(uname -m); errand-box builds for x86_64 and aarch64" 2 ;;
esac
[ "$(uname -s)" = Linux ] || die "errand-box runs on Linux" 2

# shellcheck disable=SC2317 # reached through run(), which shellcheck cannot follow
fetch() { # url dest
  if command -v curl >/dev/null 2>&1; then curl -fsSL --retry 3 -o "$2" "$1"
  elif command -v wget >/dev/null 2>&1; then wget -q -O "$2" "$1"
  else die "neither curl nor wget is installed" 2
  fi
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | cut -d' ' -f1
  elif command -v openssl >/dev/null 2>&1; then openssl dgst -sha256 "$1" | sed 's/.*= //'
  else die "no SHA-256 tool (sha256sum, shasum or openssl)" 2
  fi
}

here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d) || die "cannot make a temporary directory" 2
trap 'rm -rf "$tmp"' EXIT INT TERM

if [ -f "$here/Cargo.toml" ] && grep -q '^name = "errand-box"' "$here/Cargo.toml"; then
  command -v cargo >/dev/null 2>&1 || die "this is a checkout; install Rust (rustup) to build it, or run bootstrap.sh outside the checkout to fetch a release" 2
  say "building from the checkout in $here"
  (cd "$here" && CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-4} run cargo build --release --locked) || die "cargo build failed"
  bin="$here/target/release/errand-box"
else
  if [ -n "$base_url" ]; then
    base=$base_url
  elif [ "$version" = latest ]; then
    base="https://github.com/$repo/releases/latest/download"
  else
    base="https://github.com/$repo/releases/download/$version"
  fi
  asset="errand-box-$arch-linux-musl"
  say "fetching $asset and SHA256SUMS from $base"
  run fetch "$base/$asset" "$tmp/$asset" || die "download failed: $base/$asset"
  run fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" || die "download failed: $base/SHA256SUMS"
  if [ $dry = 0 ]; then
    want=$(awk -v f="$asset" '$2 == f || $2 == "*" f {print $1}' "$tmp/SHA256SUMS")
    [ -n "$want" ] || die "SHA256SUMS lists no $asset"
    got=$(sha256_of "$tmp/$asset")
    [ "$got" = "$want" ] || die "digest mismatch for $asset: got $got, want $want; nothing was installed"
    say "digest checked: $got"
  fi
  bin="$tmp/$asset"
fi

# The same place setup installs to, so that a later setup replaces this
# copy instead of leaving a second, older one on PATH.
run $as_root mkdir -p "$(dirname "$dest")" "$(dirname "$link")" || die "cannot create $(dirname "$dest")"
run $as_root install -m 0755 "$bin" "$dest" || die "cannot install $dest"
run $as_root ln -sfn "$dest" "$link" || die "cannot link $link"
# An earlier bootstrap.sh installed a copy in /usr/local/sbin, which comes
# first on root's PATH and would run instead of this one.
old=/usr/local/sbin/errand-box
if [ -e "$old" ] && [ "$(readlink "$old" 2>/dev/null)" != "$dest" ]; then
  run $as_root ln -sfn "$dest" "$old" || die "cannot replace $old"
fi
if [ $dry = 1 ]; then
  say "would install $dest and link $link"
else
  say "installed $dest; $link links to it"
fi
if [ $# -gt 0 ]; then
  say "running: errand-box setup $*"
  run $as_root "$dest" setup "$@"
  exit $?
fi
say "next: errand-box setup --config errand-box.toml --env errand-box.env --creds creds.env"
exit 0
