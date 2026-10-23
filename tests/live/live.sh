#!/bin/sh
# live.sh - the live tests of errand-box: the `live:` steps that
# docs/edge-cases.md names and docs/tests.md section 3 describes.
#
# Run as root on a host where `errand-box setup` has finished. Each test starts
# real session instances through the installed bailey shim, as the errand
# account, with the policy shape errand writes. It prints one line per check:
#   PASS <test>: <what was seen>
#   FAIL <test>: <what was seen>
#
# Usage: sh tests/live/live.sh [test...]
#   Tests: probe session acp inside quota signals reap maint up errand.
#   With no argument it runs all of them in that order. `up` stops every
#   supervised daemon (as a host restart does) and starts them again.
#
# Environment:
#   EB_LIVE_PROXY  host:port of an HTTP CONNECT proxy for `inside`, which
#                  pulls container images. Default: the host and port of
#                  HTTPS_PROXY. A relay on the session's broker port stands
#                  in for errand's broker.
#   EB_LIVE_CA     a CA file the guest must trust for that proxy (optional).
#
# Needs: python3, socat and curl on the host.
# Exit: 0 every check passed, 1 a check failed, 2 could not run.

# The tests are called as "t_$name", which shellcheck cannot follow.
# shellcheck disable=SC2317
set -u

[ "$(id -u)" = 0 ] || { echo "live.sh: run as root" >&2; exit 2; }
for t in python3 socat curl incus; do
  command -v "$t" >/dev/null 2>&1 || { echo "live.sh: $t is missing" >&2; exit 2; }
done
rt=/etc/errand-box/runtime.toml
[ -r "$rt" ] || { echo "live.sh: $rt is missing; run errand-box setup first" >&2; exit 2; }
val() { sed -n "s/^$1 = \"\\(.*\\)\"\$/\\1/p" "$rt"; }
project=$(val admin_project)
golden=$(val golden)
state_root=$(val state_root)
home=${state_root%/state}
here=$(cd "$(dirname "$0")" && pwd)
eb=/usr/local/lib/errand-box/bin/errand-box
bailey=/usr/local/lib/errand-box/shim/bailey
scan=/var/lib/errand-box/s6
id=live-test
inst=eb-$id
st=$state_root/$id
proj=$home/projects/$id
failed=0
bg=""

pass() { printf 'PASS %s: %s\n' "$1" "$2"; }
fail() { printf 'FAIL %s: %s\n' "$1" "$2"; failed=1; }
# A check that does not apply to this host: printed, never a failure.
skip() { printf 'SKIP %s: %s\n' "$1" "$2"; }
check() { # test, condition result (0 = pass), detail
  if [ "$2" = 0 ]; then pass "$1" "$3"; else fail "$1" "$3"; fi
}
as() { "$eb" __as errand -- "$@"; }
inc() { incus --project "$project" "$@" </dev/null; }
status() { inc list "$inst" -c s -f csv 2>/dev/null; }
lease() { inc config get "$inst" user.errand-box.lease 2>/dev/null; }
now_ms() { date +%s%3N; }
free_port() {
  python3 -I -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])'
}
wait_status() { # want, tenths of a second
  i=0
  while [ "$i" -lt "$2" ]; do
    [ "$(status)" = "$1" ] && return 0
    sleep 0.1
    i=$((i + 1))
  done
  return 1
}
stop_bg() {
  for p in $bg; do kill "$p" 2>/dev/null; done
  bg=""
}
trap stop_bg EXIT INT TERM

# The policy errand writes for a session, with its broker on `port`.
policy() {
  port=$1
  mkdir -p "$st/home" "$proj"
  [ -f "$proj/README" ] || echo "hello from the host" > "$proj/README"
  cat > "$st/policy.toml" <<P
# Generated per session by errand. Do not edit.
[filesystem]
reset = true
read = ["/usr", "/lib", "/etc/ssl", { path = "$state_root/resolv.conf", at = "/etc/resolv.conf" }, { path = "$proj", at = "/workspace" }, { path = "$st", at = "/state" }, "/usr/local/lib/errand-box/bin"]
write = [{ path = "$proj", at = "/workspace" }, { path = "$st", at = "/state" }]
execute = ["/usr", "/state/home/bin", "/usr/local/lib/errand-box/bin", { path = "$proj", at = "/workspace" }]
[[device]]
path = "/dev/null"
access = "rw"

[env]
pass = ["PROVIDER_KEY", "XDG_DATA_HOME"]
set = { PATH = "/state/home/bin:/usr/local/bin:/usr/bin:/bin", HOME = "/state/home", HTTPS_PROXY = "http://169.254.169.1:$port", HTTP_PROXY = "http://169.254.169.1:$port", NO_PROXY = "169.254.169.1" }

[resources]
file_max = "1g"
tmp_size = "512m"
shm_size = "256m"

[network]
egress_allow = [{ host = "*", port = 443 }]
P
  chown -R errand:errand "$st" "$proj"
  chmod 700 "$st"
  chmod 600 "$st/policy.toml"
  as "$bailey" trust "$st/policy.toml" >/dev/null
}

session() { # port, command...
  port=$1
  shift
  as "$bailey" run --isolate --egress-proxy "169.254.169.1:$port" --config "$st/policy.toml" --profile ai-agent -- "$@"
}

field() { printf '%s\n' "$1" | sed -n "s/^$2=//p" | head -n 1; }

t_probe() {
  d=$(as "$bailey" doctor 2>&1)
  rc=$?
  check probe "$rc" "bailey doctor exit $rc: $(printf '%s' "$d" | tr '\n' ';' | cut -c1-160)"
  policy 1
  t0=$(now_ms)
  as "$bailey" run --isolate --config "$st/policy.toml" --profile ai-agent --quiet -- true </dev/null
  rc=$?
  check probe "$rc" "per-session check (run -- true) exit $rc in $(($(now_ms) - t0)) ms"
}

t_session() {
  port=$(free_port)
  policy "$port"
  python3 -I -c "
import http.server, socketserver
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200); self.end_headers(); self.wfile.write(b'broker-ok')
    def log_message(self, *a): pass
socketserver.TCPServer(('127.0.0.1', $port), H).serve_forever()" &
  bg="$bg $!"
  sleep 0.5
  rm -f "$proj/by-agent.txt"
  t0=$(now_ms)
  # shellcheck disable=SC2016 # expanded inside the guest
  out=$(as env PROVIDER_KEY=live-nonce "$bailey" run --isolate --egress-proxy "169.254.169.1:$port" --config "$st/policy.toml" --profile ai-agent -- sh -c '
    echo "uid=$(id -u)"
    echo "groups=$(id -Gn | tr " " ,)"
    echo "readme=$(cat README)"
    echo from-agent > by-agent.txt
    # Bridges Docker and podman make inside the guest reach nothing outside.
    echo "links=$(ls /sys/class/net | grep -Ev "^(docker[0-9]|veth|br-|podman|cni)" | tr "\n" " ")"
    echo "route=$(ip -4 route show default | head -n 1)"
    echo "broker=$(curl -s --noproxy "*" -m 5 "http://169.254.169.1:$1/")"
    echo "direct=$(timeout 3 bash -c "echo > /dev/tcp/1.1.1.1/443" 2>/dev/null && echo open || echo blocked)"
    echo "fsize=$(ulimit -f)"
    echo "envfile=$(ls -A /run/errand-box | wc -l)"
    echo "key=$PROVIDER_KEY"
    echo "pid1=$(cat /proc/1/comm)"
  ' sh "$port" </dev/null 2>&1)
  rc=$?
  ms=$(($(now_ms) - t0))
  stop_bg
  check session "$rc" "session exit $rc in $ms ms"
  [ "$(field "$out" uid)" = 1000 ]; check session $? "agent uid $(field "$out" uid)"
  case ,$(field "$out" groups), in *,docker,*) r=0 ;; *) r=1 ;; esac
  check session "$r" "supplementary groups $(field "$out" groups) include docker (E-29)"
  [ "$(field "$out" readme)" = "hello from the host" ]; check session $? "workspace readable"
  [ "$(stat -c %U "$proj/by-agent.txt" 2>/dev/null)" = errand ]; check session $? "a file the agent wrote is owned by errand on the host"
  [ "$(field "$out" links)" = "lo " ]; check session $? "network devices other than guest-internal bridges: $(field "$out" links)"
  [ -z "$(field "$out" route)" ]; check session $? "no default route: '$(field "$out" route)'"
  [ "$(field "$out" broker)" = broker-ok ]; check session $? "broker reached through the proxy device: $(field "$out" broker)"
  [ "$(field "$out" direct)" = blocked ]; check session $? "direct egress: $(field "$out" direct)"
  [ "$(field "$out" fsize)" != unlimited ]; check session $? "file size limit: $(field "$out" fsize)"
  [ "$(field "$out" envfile)" = 0 ]; check session $? "environment file deleted"
  [ "$(field "$out" key)" = live-nonce ]; check session $? "passed variable arrives"
  [ "$(field "$out" pid1)" = systemd ]; check session $? "PID 1 is $(field "$out" pid1)"
  wait_status STOPPED 100; check session $? "instance $(status) after the session"
  [ -z "$(lease)" ]; check session $? "lease cleared"
}

t_acp() {
  port=$(free_port)
  policy "$port"
  out=$(python3 -I "$here/acp.py" "$eb" __as errand -- env XDG_DATA_HOME=/state/kage "$bailey" run --isolate --egress-proxy "169.254.169.1:$port" --config "$st/policy.toml" --profile ai-agent -- kage rpc 2>&1)
  rc=$?
  check acp "$rc" "$(printf '%s' "$out" | tr '\n' ';' | cut -c1-200)"
}

t_inside() {
  up=${EB_LIVE_PROXY:-$(printf '%s' "${HTTPS_PROXY:-${https_proxy:-}}" | sed 's#^[a-z]*://##; s#/.*$##; s#^.*@##')}
  if [ -z "$up" ]; then
    fail inside "no proxy to pull images through: set EB_LIVE_PROXY or HTTPS_PROXY"
    return
  fi
  port=$(free_port)
  policy "$port"
  socat "TCP-LISTEN:$port,bind=127.0.0.1,fork,reuseaddr" "TCP:$up" </dev/null >/dev/null 2>&1 &
  bg="$bg $!"
  rm -f "$proj/.eb-live-ca.crt"
  if [ -n "${EB_LIVE_CA:-}" ]; then
    cp "$EB_LIVE_CA" "$proj/.eb-live-ca.crt"
    chown errand:errand "$proj/.eb-live-ca.crt"
  fi
  # shellcheck disable=SC2016 # expanded inside the guest
  out=$(session "$port" sh -c '
    if [ -f .eb-live-ca.crt ]; then
      sudo sh -c "cp .eb-live-ca.crt /usr/local/share/ca-certificates/eb-live.crt && update-ca-certificates >/dev/null 2>&1 && systemctl restart docker"
    fi
    echo "docker=$(docker run --rm debian:trixie-slim cat /etc/debian_version 2>&1 | tail -n 1)"
    echo "build=$(printf "FROM debian:trixie-slim\nRUN echo built > /b\nCMD [\"cat\", \"/b\"]\n" | docker build -q -t eb-live - >/dev/null 2>&1 && docker run --rm eb-live 2>&1 | tail -n 1)"
    docker rmi -f eb-live >/dev/null 2>&1
    echo "podman_root=$(sudo -E podman run --rm docker.io/library/debian:trixie-slim cat /etc/debian_version 2>&1 | tail -n 1)"
    echo "podman_agent=$(podman run --rm docker.io/library/debian:trixie-slim cat /etc/debian_version 2>&1 | tail -n 1)"
    mkdir -p /tmp/l /tmp/m && echo fuse-ok > /tmp/l/f
    rclone mount /tmp/l /tmp/m --daemon --daemon-timeout 20s >/dev/null 2>&1
    echo "fuse=$(cat /tmp/m/f 2>&1) $(grep " /tmp/m " /proc/mounts | cut -d" " -f3)"
    fusermount3 -u /tmp/m 2>/dev/null
    echo "cpus=$(nproc)"
    echo "mem=$(free -m | awk "/^Mem:/ {print \$2}")"
  ' </dev/null 2>&1)
  rc=$?
  stop_bg
  rm -f "$proj/.eb-live-ca.crt"
  check inside "$rc" "session exit $rc"
  case $(field "$out" docker) in 13.*) r=0 ;; *) r=1 ;; esac
  check inside "$r" "docker run: $(field "$out" docker)"
  [ "$(field "$out" build)" = built ]; check inside $? "docker build and run: $(field "$out" build)"
  case $(field "$out" podman_root) in 13.*) r=0 ;; *) r=1 ;; esac
  check inside "$r" "sudo podman run: $(field "$out" podman_root)"
  case $(field "$out" podman_agent) in 13.*) r=0 ;; *) r=1 ;; esac
  check inside "$r" "rootless podman as the agent (E-30): $(field "$out" podman_agent)"
  [ "$(field "$out" fuse)" = "fuse-ok fuse.rclone" ]; check inside $? "FUSE mount as the agent: $(field "$out" fuse)"
  want_cpu=$(inc profile get default limits.cpu)
  [ "$(field "$out" cpus)" = "$want_cpu" ]; check inside $? "CPUs seen: $(field "$out" cpus), limit $want_cpu"
  host_mem=$(awk '/^MemTotal:/ {print int($2 / 1024)}' /proc/meminfo)
  mem=$(field "$out" mem)
  r=$(awk -v m="${mem:-0}" -v h="$host_mem" 'BEGIN { print (m > h * 0.7 && m < h * 0.85) ? 0 : 1 }')
  check inside "$r" "memory seen: $mem MiB of $host_mem MiB on the host (limits.memory $(inc profile get default limits.memory))"
}

t_quota() {
  pool=$(inc profile device get default root pool 2>/dev/null)
  driver=$(incus storage list -f csv -c nD </dev/null 2>/dev/null | sed -n "s/^$pool,//p")
  if [ "$driver" = dir ]; then
    skip quota "the pool $pool is a dir pool, which does not enforce a quota (E-05)"
    return
  fi
  q="eb-live-quota"
  inc delete -f "$q" >/dev/null 2>&1
  if ! inc copy "$golden" "$q" >/dev/null 2>&1 || ! inc config device override "$q" root size=5GiB >/dev/null 2>&1 || ! inc start "$q" >/dev/null 2>&1; then
    fail quota "cannot start a 5 GiB copy of $golden"
    inc delete -f "$q" >/dev/null 2>&1
    return
  fi
  small=$(inc exec "$q" -- sh -c 'fallocate -l 1G /var/tmp/a 2>&1; echo "rc=$?"; rm -f /var/tmp/a')
  big=$(inc exec "$q" -- sh -c 'fallocate -l 6G /var/tmp/b 2>&1; echo "rc=$?"; rm -f /var/tmp/b')
  inc delete -f "$q" >/dev/null 2>&1
  case $small in *rc=0*) r=0 ;; *) r=1 ;; esac
  check quota "$r" "1 GiB under a 5 GiB quota: $(printf '%s' "$small" | tr '\n' ' ')"
  case $big in *rc=0*) r=1 ;; *) r=0 ;; esac
  check quota "$r" "6 GiB over a 5 GiB quota is refused: $(printf '%s' "$big" | tr '\n' ' ')"
}

signal_test() { # TERM or KILL
  sig=$1
  port=$(free_port)
  policy "$port"
  setsid "$eb" __as errand -- "$bailey" run --isolate --egress-proxy "169.254.169.1:$port" --config "$st/policy.toml" --profile ai-agent -- sleep 300 </dev/null >/dev/null 2>&1 &
  pid=$!
  if ! wait_status RUNNING 300; then
    fail signals "SIG$sig: the instance did not start"
    kill -s KILL -- "-$pid" 2>/dev/null
    return
  fi
  sleep 2
  t0=$(now_ms)
  kill -s "$sig" -- "-$pid"
  wait_status STOPPED 300
  r=$?
  check signals "$r" "SIG$sig to the launcher's process group: instance $(status) after $(($(now_ms) - t0)) ms"
  sleep 1
  [ -z "$(lease)" ]; check signals $? "SIG$sig: lease cleared"
  kill -0 "$pid" 2>/dev/null && r=1 || r=0
  check signals "$r" "SIG$sig: launcher gone"
}

t_signals() {
  signal_test TERM
  signal_test KILL
}

t_reap() {
  port=$(free_port)
  policy "$port"
  setsid "$eb" __as errand -- "$bailey" run --isolate --egress-proxy "169.254.169.1:$port" --config "$st/policy.toml" --profile ai-agent -- sleep 300 </dev/null >/dev/null 2>&1 &
  pid=$!
  wait_status RUNNING 300 || { fail reap "the instance did not start"; return; }
  sleep 2
  # The bracket keeps pgrep from matching its own command line.
  wd=$(pgrep -f "[_]_watchdog $inst ")
  # shellcheck disable=SC2086 # one pid per word
  [ -n "$wd" ] && kill -s KILL $wd
  kill -s KILL -- "-$pid"
  sleep 2
  [ "$(status)" = RUNNING ]; check reap $? "launcher and watchdog killed: instance still $(status), as after a crash"
  "$eb" reap >/dev/null 2>&1
  wait_status STOPPED 300; check reap $? "errand-box reap: instance $(status)"
}

t_maint() {
  out=$("$eb" maint --once 2>&1)
  rc=$?
  check maint "$rc" "maint --once exit $rc: $(printf '%s' "$out" | tr '\n' ';' | cut -c1-200)"
}

t_up() {
  if [ ! -d "$scan" ]; then
    pass up "no s6 scan directory: this host runs systemd, which restores the daemons itself"
    return
  fi
  ctl=$(command -v s6-svscanctl || echo /usr/bin/s6-svscanctl)
  "$ctl" -t "$scan"
  i=0
  while [ "$i" -lt 600 ] && incus query /1.0 </dev/null >/dev/null 2>&1; do sleep 0.1; i=$((i + 1)); done
  incus query /1.0 </dev/null >/dev/null 2>&1 && r=1 || r=0
  check up "$r" "daemons stopped through s6-svscanctl, as a restart loses them"
  chmod 0600 /dev/fuse /dev/net/tun 2>/dev/null
  t0=$(now_ms)
  out=$("$eb" up 2>&1)
  rc=$?
  check up "$rc" "errand-box up exit $rc in $(($(now_ms) - t0)) ms"
  incus query /1.0 </dev/null >/dev/null 2>&1; check up $? "incusd answers"
  [ "$(stat -c %a /dev/fuse)" = 666 ]; check up $? "/dev/fuse mode $(stat -c %a /dev/fuse) (E-03)"
  sv=$(command -v s6-svstat || echo /usr/bin/s6-svstat)
  for s in incusd incus-user lxcfs errand errand-box-maint; do
    case $("$sv" "$scan/$s" 2>&1) in up*) r=0 ;; *) r=1 ;; esac
    check up "$r" "service $s: $("$sv" "$scan/$s" 2>&1 | cut -c1-60)"
  done
  out=$("$eb" up 2>&1)
  rc=$?
  case $(printf '%s\n' "$out" | tail -n 1) in "0 changed"*) r=$rc ;; *) r=1 ;; esac
  check up "$r" "a second up changes nothing: $(printf '%s' "$out" | tail -n 1)"
  t_probe
}

t_errand() {
  if [ -d "$scan" ]; then
    sv=$(command -v s6-svstat || echo /usr/bin/s6-svstat)
    state=$("$sv" "$scan/errand" 2>&1)
    log=$(cat /var/log/errand-box/errand/current 2>/dev/null)
  else
    state=$(systemctl is-active errand-box-errand.service 2>&1)
    log=$(journalctl -u errand-box-errand.service --no-pager -n 300 2>/dev/null)
  fi
  case $state in up* | active*) r=0 ;; *) r=1 ;; esac
  check errand "$r" "service errand: $(printf '%s' "$state" | cut -c1-60)"
  case $log in *ConfigError*) r=1 ;; *) r=0 ;; esac
  check errand "$r" "errand accepts the config setup wrote"
  case $log in *"this backend enforces every configured guarantee on this host"*) r=0 ;; *) r=1 ;; esac
  check errand "$r" "errand's own probe of the bailey shim passed: the backend enforces every configured guarantee"
}

tests=${*:-probe session acp inside quota signals reap maint up errand}
for t in $tests; do
  case $t in
    probe | session | acp | inside | quota | signals | reap | maint | up | errand) "t_$t" ;;
    *) echo "live.sh: unknown test $t" >&2; exit 2 ;;
  esac
done
inc stop -f "$inst" >/dev/null 2>&1
exit $failed
