#!/bin/sh
# pins.sh - read and move the pins of pins.toml.
#
# The defect this exists to stop: a new upstream release that errand-box
# never picks up, or picks up with a digest nobody checked. errand-box runs
# only bytes whose sha256 pins.toml records; this script is the one way a
# version gets in.
#
# Usage:
#   sh scripts/pins.sh pinned <tool>            the pinned `latest`
#   sh scripts/pins.sh upstream <tool>          the newest release tag upstream
#   sh scripts/pins.sh bump <tool> <version>    add <version> and make it latest
#
# `bump` takes the URL of each architecture from the pinned latest with the
# version replaced, downloads it, checks it against the `.sha256` file next to
# it when upstream publishes one, and writes the digest. It changes nothing
# when a download or a check fails.
#
# For a GitHub release, `bump` also compares each download with the digest
# GitHub records for the asset, when the API gives one.
#
# Environment: PINS_FILE (default pins.toml at the repository root);
# GH_TOKEN for the GitHub API (optional; raises the rate limit); GITHUB_API
# (default https://api.github.com), for a host that reaches the API through
# a mirror.
# Needs: curl, awk, sed, and sha256sum or shasum.
# Exit codes: 0 done, 1 a download or a check failed, 2 could not run.
set -u

root=$(cd "$(dirname "$0")/.." && pwd)
pins=${PINS_FILE:-$root/pins.toml}
die() { printf 'pins: %s\n' "$1" >&2; exit "${2:-1}"; }
[ -r "$pins" ] || die "cannot read $pins" 2
command -v curl >/dev/null 2>&1 || die "curl is missing" 2

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# The value of `latest` in [<tool>].
pinned() {
  awk -v t="[$1]" '$0 == t { s = 1; next } /^\[/ { s = 0 } s && $1 == "latest" { gsub(/"/, "", $3); print $3; exit }' "$pins"
}

# The url of [<tool>.versions."<version>".<arch>].
url_of() {
  awk -v t="[$1.versions.\"$2\".$3]" '$0 == t { s = 1; next } /^\[/ { s = 0 } s && $1 == "url" { gsub(/"/, "", $3); print $3; exit }' "$pins"
}

# owner/repo of a GitHub release URL.
repo_of() {
  printf '%s\n' "$1" | sed -n 's#^https://github.com/\([^/]*/[^/]*\)/releases/.*#\1#p'
}

api=${GITHUB_API:-https://api.github.com}
api_get() {
  if [ -n "${GH_TOKEN:-}" ]; then
    curl -fsSL --retry 3 -H "Authorization: Bearer $GH_TOKEN" "$api/$1"
  else
    curl -fsSL --retry 3 "$api/$1"
  fi
}

# The sha256 GitHub records for asset $3 of release $2 of repo $1, or nothing.
api_digest() {
  api_get "repos/$1/releases/tags/$2" 2>/dev/null | tr '{}' ',,' | tr ',' '\n' |
    awk -v n="\"name\":\"$3\"" '$0 == n { f = 1; next } /^"name":/ { f = 0 }
      f && /^"digest":"sha256:/ { sub(/^"digest":"sha256:/, ""); sub(/"$/, ""); print; exit }'
}

tool=${2:-}
case ${1:-} in
  pinned)
    [ -n "$tool" ] || die "usage: pins.sh pinned <tool>" 2
    v=$(pinned "$tool")
    [ -n "$v" ] || die "pins.toml has no $tool" 2
    printf '%s\n' "$v"
    ;;
  upstream)
    [ -n "$tool" ] || die "usage: pins.sh upstream <tool>" 2
    v=$(pinned "$tool")
    [ -n "$v" ] || die "pins.toml has no $tool" 2
    repo=$(repo_of "$(url_of "$tool" "$v" x86_64)")
    [ -n "$repo" ] || die "$tool is not a GitHub release" 2
    tag=$(api_get "repos/$repo/releases/latest" | grep -o '"tag_name": *"[^"]*"' | head -n 1 | sed 's/.*"\([^"]*\)"$/\1/')
    [ -n "$tag" ] || die "no latest release for $repo"
    printf '%s\n' "$tag"
    ;;
  bump)
    new=${3:-}
    if [ -z "$tool" ] || [ -z "$new" ]; then die "usage: pins.sh bump <tool> <version>" 2; fi
    case $new in *[!A-Za-z0-9._-]*) die "bad version: $new" 2 ;; esac
    old=$(pinned "$tool")
    [ -n "$old" ] || die "pins.toml has no $tool" 2
    [ "$old" = "$new" ] && { printf 'pins: %s is already %s\n' "$tool" "$new"; exit 0; }
    if grep -qF "[$tool.versions.\"$new\"." "$pins"; then
      die "$tool $new is pinned already but not latest; move latest by hand" 2
    fi
    tmp=$(mktemp -d) || die "cannot make a temporary directory" 2
    trap 'rm -rf "$tmp"' EXIT INT TERM
    block=""
    for arch in x86_64 aarch64; do
      ou=$(url_of "$tool" "$old" "$arch")
      [ -n "$ou" ] || die "$tool $old has no $arch url" 2
      # v0.3.1 -> v0.4.0, and the bare 0.3.1 -> 0.4.0 inside file names.
      nu=$(printf '%s\n' "$ou" | sed "s#$old#$new#g; s#${old#v}#${new#v}#g")
      curl -fsSL --retry 3 -o "$tmp/a" "$nu" || die "download failed: $nu"
      sum=$(sha256_of "$tmp/a")
      how=""
      if curl -fsSL --retry 3 -o "$tmp/a.sha256" "$nu.sha256" 2>/dev/null; then
        want=$(cut -d' ' -f1 < "$tmp/a.sha256")
        [ "$want" = "$sum" ] || die "digest mismatch for $nu: upstream says $want, the download is $sum"
        how="matches upstream's .sha256"
      fi
      repo=$(repo_of "$nu")
      if [ -n "$repo" ]; then
        gd=$(api_digest "$repo" "$new" "${nu##*/}")
        if [ -n "$gd" ]; then
          [ "$gd" = "$sum" ] || die "digest mismatch for $nu: GitHub records $gd, the download is $sum"
          how="${how:+$how; }matches GitHub's asset digest"
        fi
      fi
      [ -n "$how" ] || how="upstream publishes no digest; recorded on first use"
      printf 'pins: %s %s %s sha256 %s (%s)\n' "$tool" "$new" "$arch" "$sum" "$how"
      entry="[$tool.versions.\"$new\".$arch]
url = \"$nu\"
sha256 = \"$sum\"
"
      block="$block${block:+
}$entry"
    done
    # The new versions go after the tool's last block; latest moves.
    awk -v tool="$tool" -v new="$new" -v block="$block" '
      # Before the next table: the block, then the blank line that
      # separated the two. At the end of the file: a blank line, then it.
      function flush() { if (pending) { printf "%s\n", block; pending = 0 } }
      /^\[/ {
        name = $0; sub(/^\[/, "", name); sub(/[].].*$/, "", name)
        if (in_tool && name != tool) { flush(); in_tool = 0 }
        if (name == tool) { in_tool = 1; pending = 1 }
      }
      in_tool && $1 == "latest" { print "latest = \"" new "\""; next }
      { print }
      END { if (pending) printf "\n%s", block }
    ' "$pins" > "$tmp/pins" || die "cannot rewrite $pins" 2
    cat "$tmp/pins" > "$pins"
    printf 'pins: %s latest %s -> %s\n' "$tool" "$old" "$new"
    ;;
  *) die "usage: pins.sh pinned|upstream <tool> | bump <tool> <version>" 2 ;;
esac
