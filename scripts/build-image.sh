#!/bin/sh
# build-image.sh - build the errand-box guest image with distrobuilder.
#
# The second route to the guest image, beside `errand-box image build`. It
# needs no running Incus, so a CI runner can build the image once and a host
# can import it (`errand-box image import OUT_DIR`) instead of building it.
# Both routes run image/provision.sh unchanged.
#
# Usage: sh scripts/build-image.sh OUT_DIR
#        sh scripts/build-image.sh --print-inputs
#
# --print-inputs prints the inputs digest and builds nothing; a unit test
# checks it against errand-box's own (src/image.rs inputs_digest).
#
# Environment:
#   HTTPS_PROXY          a proxy for apt inside the build (optional)
#   PROXY_CA             the CA file of that proxy, if it re-signs TLS
#   EB_REGISTRY_MIRROR   Docker Hub mirror for guests (default mirror.gcr.io)
#   EB_GUEST_PROXY_PORT  where guest services reach the broker (default 3128)
#   KAGE_VERSION         a version pinned in pins.toml (default: latest)
#   DISTROBUILDER        the distrobuilder program (default: on PATH)
# Needs: root, distrobuilder 3.x, debootstrap, debian-archive-keyring,
# squashfs-tools, curl, sha256sum.
# Output: OUT_DIR/incus.tar.xz, OUT_DIR/rootfs.squashfs, OUT_DIR/SHA256SUMS,
# and OUT_DIR/inputs, the digest `errand-box image import` records so that
# setup sees the image as current (E-31).
# Exit codes: 0 built, 1 the build failed, 2 could not run.
set -u
root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-}
die() { printf 'build-image: %s\n' "$1" >&2; exit "${2:-1}"; }
[ -n "$out" ] || die "usage: build-image.sh OUT_DIR" 2

# The digest of everything the image is built from, as src/image.rs
# inputs_digest computes it: each part followed by a NUL byte.
inputs_digest() { # kage-version
  {
    for f in provision.sh launch guest-init; do cat "$root/image/$f"; printf '\000'; done
    printf '%s\000' "$1" "${EB_REGISTRY_MIRROR-https://mirror.gcr.io}" "${EB_GUEST_PROXY_PORT:-3128}"
  } | sha256sum | cut -d' ' -f1
}
if [ "$out" = --print-inputs ]; then
  v=${KAGE_VERSION:-$(PINS_FILE="$root/pins.toml" sh "$root/scripts/pins.sh" pinned kage)} || exit 2
  inputs_digest "$v"
  exit 0
fi
[ "$(id -u)" = 0 ] || die "run as root: debootstrap needs it" 2
db=${DISTROBUILDER:-$(command -v distrobuilder || true)}
if [ -z "$db" ] || [ ! -x "$db" ]; then die "distrobuilder is missing; see docs/operations.md" 2; fi
for t in debootstrap mksquashfs curl sha256sum; do
  command -v "$t" >/dev/null 2>&1 || die "$t is missing" 2
done
[ -f /usr/share/keyrings/debian-archive-keyring.gpg ] || die "debian-archive-keyring is missing" 2
case $(uname -m) in
  x86_64 | amd64) arch=x86_64 ;;
  aarch64 | arm64) arch=aarch64 ;;
  *) die "no image for $(uname -m)" 2 ;;
esac

# kage, from pins.toml, digest checked.
v=${KAGE_VERSION:-$(PINS_FILE="$root/pins.toml" sh "$root/scripts/pins.sh" pinned kage)} || exit 2
field() {
  awk -v t="[kage.versions.\"$v\".$arch]" -v k="$1" '$0 == t { s = 1; next } /^\[/ { s = 0 } s && $1 == k { gsub(/"/, "", $3); print $3; exit }' "$root/pins.toml"
}
url=$(field url)
want=$(field sha256)
if [ -z "$url" ] || [ -z "$want" ]; then die "pins.toml has no kage $v for $arch" 2; fi

work=$(mktemp -d) || exit 2
trap 'rm -rf "$work"' EXIT INT TERM
stage=$work/eb-build
mkdir -p "$stage"
curl -fsSL --retry 3 -o "$stage/kage.tar.xz" "$url" || die "cannot download $url"
got=$(sha256sum "$stage/kage.tar.xz" | cut -d' ' -f1)
[ "$got" = "$want" ] || die "digest mismatch for $url: got $got, pinned $want"
for f in provision.sh launch guest-init; do cp "$root/image/$f" "$stage/$f"; done
chmod 0755 "$stage/provision.sh" "$stage/launch" "$stage/guest-init"
if [ -n "${PROXY_CA:-}" ]; then
  cp "$PROXY_CA" "$stage/proxy-ca.crt" || die "cannot read PROXY_CA $PROXY_CA" 2
fi
# Single-quoted, as `errand-box image build` writes it (E-46).
for v in "${EB_GUEST_PROXY_PORT:-}" "${EB_REGISTRY_MIRROR:-}" "${HTTPS_PROXY:-}"; do
  case $v in *"'"*) die "a setting (proxy, mirror or port) has a single quote" 2 ;; esac
done
{
  printf "EB_GUEST_PROXY_PORT='%s'\n" "${EB_GUEST_PROXY_PORT:-3128}"
  printf "EB_REGISTRY_MIRROR='%s'\n" "${EB_REGISTRY_MIRROR-https://mirror.gcr.io}"
  if [ -n "${HTTPS_PROXY:-}" ]; then printf "EB_PROXY='%s'\n" "$HTTPS_PROXY"; fi
} > "$stage/env"
chmod 0600 "$stage/env"

mkdir -p "$out" || die "cannot create $out" 2
out=$(cd "$out" && pwd)
# debootstrap needs the proxy in its own environment; apt in the build reads
# it from provision.sh, through the env file above.
(cd "$work" && "$db" build-incus "$root/image/distrobuilder.yaml" "$out" \
  -o "image.architecture=$arch" -o "source.url=https://deb.debian.org/debian") || die "distrobuilder failed"
(cd "$out" && sha256sum incus.tar.xz rootfs.squashfs > SHA256SUMS) || die "cannot write SHA256SUMS"
inputs_digest "$v" > "$out/inputs" || die "cannot write $out/inputs"
printf 'build-image: %s\n' "$out"
cat "$out/SHA256SUMS"
