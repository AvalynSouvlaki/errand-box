#!/bin/sh
# host.sh - set errand-box up on another kind of host, and test it there.
#
# The live tests (tests/live/live.sh) prove errand-box on the host they run
# on. This script makes other hosts: an Incus system container of a given
# image, nested (systemd as PID 1 on Ubuntu and Debian, OpenRC and musl on
# Alpine). It installs errand-box in it through bootstrap.sh, runs setup
# twice, runs the live tests inside, restarts the container, and checks that
# errand-box comes back.
#
# Usage: sh tests/hosts/host.sh <name> <image> [step...]
#   name   a short word; the container is ebt-<name>
#   image  an image of the images: remote, e.g. ubuntu/24.04, debian/13,
#          alpine/3.22
#   steps  create bootstrap setup live restart destroy (default: every step
#          but destroy, in that order)
#
# Run as root on a host where Incus runs (errand-box setup did that). The
# container is in the default project on the pool `default`. A nested
# storage pool on a loop file needs privileges a container does not have,
# so the nested errand-box uses a `dir` pool and reports it as degraded
# (E-05).
#
# Environment:
#   EB_HOSTS_PROXY  host:port of an HTTP proxy on this host's loopback that
#                   the container uses for all egress (default: the host and
#                   port of HTTPS_PROXY). It appears in the container at
#                   127.0.0.1:3128, through a proxy device.
#   EB_HOSTS_CA     a CA file the container must trust for that proxy.
#   EB_HOSTS_BIN    the errand-box binary to install (default: the static
#                   build of this checkout for this machine).
# Output: one PASS or FAIL line per check, and the logs under
#   tests/hosts/out/<name>/ (ignored by git).
# Exit codes: 0 every check passed, 1 a check failed, 2 could not run.

# The steps are called as "s_$name", which shellcheck cannot follow.
# shellcheck disable=SC2317
set -u

[ "$(id -u)" = 0 ] || { echo "host.sh: run as root" >&2; exit 2; }
[ $# -ge 2 ] || { sed -n '2,36p' "$0"; exit 2; }
name=$1
image=$2
shift 2
steps=${*:-create bootstrap setup live restart}
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
c=ebt-$name
out=$here/out/$name
mkdir -p "$out"
arch=$(uname -m)
bin=${EB_HOSTS_BIN:-$root/target/$arch-unknown-linux-musl/release/errand-box}
up=${EB_HOSTS_PROXY:-$(printf '%s' "${HTTPS_PROXY:-${https_proxy:-}}" | sed 's#^[a-z]*://##; s#/.*$##; s#^.*@##')}
failed=0
pass() { printf 'PASS %s %s: %s\n' "$name" "$1" "$2"; }
fail() { printf 'FAIL %s %s: %s\n' "$name" "$1" "$2"; failed=1; }
check() { if [ "$2" = 0 ]; then pass "$1" "$3"; else fail "$1" "$3"; fi; }
inc() { incus "$@" </dev/null; }
# Run a command in the container with the proxy settings a host would have.
in_c() {
  incus exec "$c" --env HTTPS_PROXY=http://127.0.0.1:3128 --env HTTP_PROXY=http://127.0.0.1:3128 \
    --env https_proxy=http://127.0.0.1:3128 --env http_proxy=http://127.0.0.1:3128 \
    --env NO_PROXY=127.0.0.1,localhost --env no_proxy=127.0.0.1,localhost -- "$@"
}
family() {
  case $image in
    alpine/*) echo apk ;;
    *) echo apt ;;
  esac
}

# Point the container's 127.0.0.1:3128 at the host's proxy. The host's
# proxy port moves across restarts (E-13), so this runs at every step.
link_proxy() {
  [ -n "$up" ] || { echo "host.sh: no proxy: set EB_HOSTS_PROXY or HTTPS_PROXY" >&2; return 1; }
  inc config device remove "$c" eb-proxy >/dev/null 2>&1
  inc config device add "$c" eb-proxy proxy listen=tcp:127.0.0.1:3128 "connect=tcp:$up" bind=instance >/dev/null
}

wait_ready() { # the container's init is up and it resolves nothing itself
  i=0
  while [ "$i" -lt 120 ]; do
    in_c sh -c 'test -e /run/systemd/system && systemctl is-system-running 2>/dev/null | grep -qE "running|degraded" || test ! -e /run/systemd/system' >/dev/null 2>&1 && return 0
    sleep 1
    i=$((i + 1))
  done
  return 1
}

s_create() {
  inc delete -f "$c" >/dev/null 2>&1
  # A uid range large enough for the nested Incus to give its own guests
  # ranges; every cgroup v1 hierarchy mounted, because a nested LXC cannot
  # start a guest without the cpu and cpuacct controllers, which Incus leaves
  # out when the host mounts them apart; FUSE and tun for lxcfs, guests and
  # podman; no network device:
  # all egress goes through the proxy device, as on a host whose only way
  # out is a proxy.
  if ! inc init "images:$image" "$c" -s default \
    -c security.nesting=true -c security.idmap.isolated=true -c security.idmap.size=100000000 \
    -c security.syscalls.intercept.mknod=true -c security.syscalls.intercept.setxattr=true \
    -c raw.lxc='lxc.mount.auto = cgroup:rw:force' \
    >"$out/create.log" 2>&1; then
    fail create "incus init images:$image: $(tail -n 1 "$out/create.log")"
    return
  fi
  inc config device add "$c" eth0 none >/dev/null
  inc config device add "$c" fuse unix-char source=/dev/fuse path=/dev/fuse mode=0666 >/dev/null
  inc config device add "$c" tun unix-char source=/dev/net/tun path=/dev/net/tun mode=0666 >/dev/null
  link_proxy
  inc start "$c" >>"$out/create.log" 2>&1
  wait_ready
  check create $? "$c from images:$image is up ($(in_c sh -c 'cat /proc/1/comm'))"
  if [ -n "${EB_HOSTS_CA:-}" ]; then
    inc file push -p "$EB_HOSTS_CA" "$c/usr/local/share/ca-certificates/eb-proxy.crt" >/dev/null
  fi
  # Test tools only: errand-box itself installs everything it needs.
  case $(family) in
    apk)
      in_c sh -c 'apk add -q ca-certificates >/dev/null 2>&1; update-ca-certificates >/dev/null 2>&1; apk add -q python3 socat curl coreutils util-linux bash >/dev/null' >"$out/tools.log" 2>&1 ;;
    apt)
      # The proxy accepts CONNECT only, so apt must use https, as on a host
      # whose only egress is such a proxy.
      # shellcheck disable=SC2016 # expanded in the container
      in_c sh -c 'for f in /etc/apt/sources.list /etc/apt/sources.list.d/*; do [ -f "$f" ] && sed -i "s#http://#https://#g" "$f"; done; update-ca-certificates >/dev/null 2>&1; apt-get -qq update && DEBIAN_FRONTEND=noninteractive apt-get -qq install -y python3 socat curl >/dev/null' >"$out/tools.log" 2>&1 ;;
  esac
  check create $? "test tools installed (python3, socat, curl)"
}

s_bootstrap() {
  link_proxy
  [ -x "$bin" ] || { fail bootstrap "no binary at $bin; run sh scripts/ci-local.sh --step musl"; return; }
  # Serve a release the way GitHub does: the asset and SHA256SUMS. The
  # container reaches the server through a proxy device.
  rel=$out/release
  rm -rf "$rel"
  mkdir -p "$rel/download/v0.0.0-test"
  cp "$bin" "$rel/download/v0.0.0-test/errand-box-$arch-linux-musl"
  (cd "$rel/download/v0.0.0-test" && sha256sum "errand-box-$arch-linux-musl" > SHA256SUMS)
  port=$(python3 -I -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
  python3 -I -m http.server --bind 127.0.0.1 --directory "$rel" "$port" >"$out/http.log" 2>&1 &
  hp=$!
  sleep 0.5
  inc config device remove "$c" eb-release >/dev/null 2>&1
  inc config device add "$c" eb-release proxy listen=tcp:127.0.0.1:8099 "connect=tcp:127.0.0.1:$port" bind=instance >/dev/null
  inc file push "$root/bootstrap.sh" "$c/root/bootstrap.sh" >/dev/null
  in_c sh /root/bootstrap.sh --base-url http://127.0.0.1:8099/download/v0.0.0-test >"$out/bootstrap.log" 2>&1
  rc=$?
  kill "$hp" 2>/dev/null
  inc config device remove "$c" eb-release >/dev/null 2>&1
  check bootstrap "$rc" "bootstrap.sh fetched, checked and installed the release binary (exit $rc): $(grep -m 1 'digest' "$out/bootstrap.log")"
  v=$(in_c errand-box version 2>&1)
  [ -n "$v" ]
  check bootstrap $? "errand-box is on PATH: $(printf '%s' "$v" | head -n 1)"
}

write_inputs() {
  ca=""
  [ -n "${EB_HOSTS_CA:-}" ] && ca=/usr/local/share/ca-certificates/eb-proxy.crt
  cat > "$out/errand-box.toml" <<EOF
# Written by tests/hosts/host.sh for a nested test host.
[engine]
pool_driver = "dir"

[network]
proxy_ca = "$ca"
EOF
  cat > "$out/errand-box.env" <<'EOF'
ERRAND_CHANNEL_ID=100000000000000001
ERRAND_ALLOWED_USER_IDS=100000000000000002
ERRAND_PROVIDER=gateway
ERRAND_PROVIDER_BASE_URL=https://gateway.invalid/v1
ERRAND_PROVIDER_KIND=openai
ERRAND_PROVIDER_CREDENTIAL_NAME=PROVIDER_KEY
ERRAND_MODEL=test-model
EOF
  printf 'ERRAND_CHAT_TOKEN=not-a-token\nERRAND_PROVIDER_CREDENTIAL=not-a-key\n' > "$out/creds.env"
  for f in errand-box.toml errand-box.env creds.env; do
    inc file push "$out/$f" "$c/root/$f" >/dev/null
  done
}

s_setup() {
  link_proxy
  write_inputs
  t0=$(date +%s)
  in_c errand-box setup --config /root/errand-box.toml --env /root/errand-box.env --creds /root/creds.env >"$out/setup1.log" 2>&1
  rc=$?
  # The test creds are not real, so errand (the daemon) crash-loops; that is
  # the one service allowed to fail (T-015 needs real credentials). Every
  # other FAILED row is a real defect.
  other_fail "$out/setup1.log"
  check setup "$?" "first setup exit $rc in $(($(date +%s) - t0)) s: $(tail -n 1 "$out/setup1.log")"
  grep -E 'FAILED' "$out/setup1.log" | sed 's/^/  /'
  in_c errand-box setup --config /root/errand-box.toml --env /root/errand-box.env --creds /root/creds.env >"$out/setup2.log" 2>&1
  # Idempotent but for errand's own crash: nothing else changed or failed.
  other_fail "$out/setup2.log"
  of=$?
  case $(tail -n 1 "$out/setup2.log") in
    0\ changed*) r=$of ;;
    *\ changed*) r=$([ "$(sed -n 's/^\([0-9]*\) changed.*/\1/p' "$out/setup2.log" | tail -n1)" = 0 ] && echo "$of" || echo 1) ;;
    *) r=1 ;;
  esac
  check setup "$r" "second setup changes nothing but errand: $(tail -n 1 "$out/setup2.log")"
}

# Exit 0 when the only FAILED rows name the errand daemon (its dummy creds),
# 1 when any other row failed.
other_fail() {
  ! grep -E '  FAILED ' "$1" | grep -qvE '^(errand|errand-box-errand|bailey doctor as errand) '
}

s_live() {
  link_proxy
  for f in live.sh acp.py; do inc file push "$root/tests/live/$f" "$c/root/$f" >/dev/null; done
  ca=""
  [ -n "${EB_HOSTS_CA:-}" ] && ca=/usr/local/share/ca-certificates/eb-proxy.crt
  in_c env EB_LIVE_PROXY=127.0.0.1:3128 EB_LIVE_CA="$ca" sh /root/live.sh >"$out/live.log" 2>&1
  rc=$?
  check live "$rc" "tests/live/live.sh: $(grep -c '^PASS' "$out/live.log") PASS, $(grep -c '^FAIL' "$out/live.log") FAIL, $(grep -c '^SKIP' "$out/live.log") SKIP"
  grep -E '^(FAIL|SKIP)' "$out/live.log" | sed 's/^/  /'
}

s_restart() {
  inc restart "$c" >"$out/restart.log" 2>&1 || inc restart -f "$c" >>"$out/restart.log" 2>&1
  link_proxy
  wait_ready
  if in_c test -e /run/systemd/system; then
    # systemd brings the units back by itself; `up` is not needed.
    i=0
    while [ "$i" -lt 90 ] && ! in_c sh -c 'errand-box __as errand -- /usr/local/lib/errand-box/shim/bailey doctor' >/dev/null 2>&1; do
      sleep 1
      i=$((i + 1))
    done
    in_c sh -c 'errand-box __as errand -- /usr/local/lib/errand-box/shim/bailey doctor' >"$out/restart-doctor.log" 2>&1
    check restart $? "after a restart, systemd alone brings errand's sandbox back (bailey doctor as errand, ${i} s)"
    in_c systemctl is-active errand-box-errand.service >"$out/restart-errand.log" 2>&1
    check restart $? "errand-box-errand.service is $(cat "$out/restart-errand.log")"
  fi
  in_c errand-box up >"$out/up.log" 2>&1
  rc=$?
  check restart "$rc" "errand-box up after the restart: $(tail -n 1 "$out/up.log")"
  grep -E 'FAILED' "$out/up.log" | sed 's/^/  /'
  in_c env EB_LIVE_PROXY=127.0.0.1:3128 sh /root/live.sh probe session >"$out/live-after-restart.log" 2>&1
  check restart $? "probe and a session after the restart: $(grep -c '^PASS' "$out/live-after-restart.log") PASS, $(grep -c '^FAIL' "$out/live-after-restart.log") FAIL"
}

s_destroy() {
  inc delete -f "$c" >/dev/null 2>&1
  inc info "$c" >/dev/null 2>&1 && r=1 || r=0
  check destroy "$r" "$c deleted"
}

for s in $steps; do
  case $s in
    create | bootstrap | setup | live | restart | destroy) "s_$s" ;;
    *) echo "host.sh: unknown step $s" >&2; exit 2 ;;
  esac
done
exit "$failed"
