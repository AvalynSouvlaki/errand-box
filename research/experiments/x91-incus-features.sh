#!/bin/sh
# Reproduce the Incus claims of next-session.md step 3 on this host:
#  - systemd is PID 1; Docker runs and builds; podman runs (--cgroups=disabled
#    on cgroup v1); FUSE mounts a local dir and network storage (rclone :http:)
#  - an existing, stopped instance starts in under 1 s
#  - a golden copy on btrfs gives a new instance in about 0.4 s
#  - btrfs enforces root.size; memory and cpu limits apply; lxcfs shows them
# Every incus client call has stdin closed (section 9.6 hang).
# Run as root with incusd, lxcfs and the relay on incusbr0 up, a btrfs pool
# named bt, and the incus image "bench". Exit 0 all pass, 1 a fail, 2 no run.
set -u
pool=${POOL:-bt}
ca=${PROXY_CA:-}
br=$(incus network get incusbr0 ipv4.address </dev/null | cut -d/ -f1)
relay="http://$br:${RELAY_PORT:-33337}"
fail=0
ok() { echo "PASS $*"; }
no() { echo "FAIL $*"; fail=1; }
ms() { echo $(( ($(date +%s%N) - $1) / 1000000 )); }
median() { sort -n | awk '{a[NR]=$1} END {print (NR%2 ? a[(NR+1)/2] : int((a[NR/2]+a[NR/2+1])/2))}'; }
i() { incus "$@" </dev/null; }
x() { n=$1; shift; incus exec "$n" -- "$@" </dev/null; }

[ -n "$br" ] || { echo "no incusbr0"; exit 2; }
i image info bench >/dev/null 2>&1 || { echo "no image bench"; exit 2; }

# 1. A featured instance, built once: nesting on, the tools installed.
if ! i image info feat >/dev/null 2>&1; then
  i delete -f featbuild >/dev/null 2>&1
  t0=$(date +%s%N)
  i launch -q bench featbuild -s "$pool" -c security.nesting=true || exit 2
  echo "launch bench on $pool (first): $(ms "$t0") ms"
  sleep 3
  incus file push -q "$ca" featbuild/usr/local/share/ca-certificates/proxy-ca.crt </dev/null || exit 2
  x featbuild sh -c "
    set -e
    for f in /etc/apt/sources.list /etc/apt/sources.list.d/*; do [ -f \"\$f\" ] && sed -i 's#http://deb.debian.org#https://deb.debian.org#g' \"\$f\"; done
    printf 'Acquire::https::Proxy \"%s\";\nAcquire::https::CaInfo \"/usr/local/share/ca-certificates/proxy-ca.crt\";\n' '$relay' > /etc/apt/apt.conf.d/99proxy
    apt-get -qq update
    DEBIAN_FRONTEND=noninteractive apt-get -qq install -y --no-install-recommends \
      ca-certificates docker.io docker-cli iptables nftables podman crun uidmap fuse-overlayfs passt \
      fuse3 rclone procps >/dev/null
    update-ca-certificates >/dev/null
    mkdir -p /etc/containers/containers.conf.d
    printf '[containers]\ncgroups = \"disabled\"\n[engine]\ncgroup_manager = \"cgroupfs\"\nevents_logger = \"file\"\n' > /etc/containers/containers.conf.d/50-cgroup-v1.conf
  " || { echo "featbuild provisioning failed"; exit 2; }
  i stop featbuild && i publish -q featbuild --alias feat >/dev/null || exit 2
  i delete -f featbuild >/dev/null
fi

# 2. Features inside one instance.
n=feat1; i delete -f $n >/dev/null 2>&1
i launch -q feat $n -s "$pool" -c security.nesting=true || exit 2
for _ in $(seq 30); do x $n systemctl is-system-running 2>/dev/null | grep -qE 'running|degraded' && break; sleep 1; done
p1=$(x $n readlink /proc/1/exe)
case $p1 in */systemd) ok "systemd is PID 1 ($p1, state $(x $n systemctl is-system-running 2>/dev/null))" ;; *) no "PID 1 is $p1" ;; esac
x $n systemctl is-active docker >/dev/null 2>&1 && ok "docker started by systemd: $(x $n docker version -f '{{.Server.Version}}' 2>/dev/null)" || no "docker not active"
docker image inspect debian:trixie-slim >/dev/null 2>&1 || docker pull -q debian:trixie-slim >/dev/null
docker save debian:trixie-slim | incus exec $n -- docker load -q >/dev/null 2>&1 && ok "docker load inside" || no "docker load inside"
v=$(x $n docker run --rm debian:trixie-slim cat /etc/debian_version 2>&1 | tail -1)
case $v in 13.*) ok "docker run inside: debian $v" ;; *) no "docker run inside: $v" ;; esac
x $n sh -c 'mkdir -p /b && cd /b && printf "int main(void){return 0;}\n" > m.c &&
  printf "FROM debian:trixie-slim\nCOPY m.c /m.c\nRUN true\nCMD [\"true\"]\n" > Dockerfile' &&
  x $n env DOCKER_BUILDKIT=0 docker build -q -t t1 /b >/dev/null 2>&1 && ok "docker build inside (legacy builder)" || no "docker build inside"
docker save debian:trixie-slim | incus exec $n -- podman load -q >/dev/null 2>&1
v=$(x $n podman run --rm --network=none debian:trixie-slim cat /etc/debian_version 2>&1 | tail -1)
case $v in 13.*) ok "podman run inside (containers.conf cgroups=disabled): debian $v, driver $(x $n podman info --format '{{.Store.GraphDriverName}}' 2>/dev/null)" ;; *) no "podman run inside: $v" ;; esac
x $n sh -c 'mkdir -p /srv/l /mnt/l && echo hello > /srv/l/f && rclone mount /srv/l /mnt/l --daemon --daemon-timeout 20s 2>/tmp/r1 && cat /mnt/l/f' 2>/dev/null | grep -q hello &&
  ok "fuse: rclone mount of a local dir" || no "fuse: rclone local"
x $n fusermount3 -u /mnt/l >/dev/null 2>&1
x $n sh -c "mkdir -p /mnt/n && HTTPS_PROXY=$relay rclone mount :http: /mnt/n --http-url https://deb.debian.org/debian/ --read-only --daemon --daemon-timeout 30s 2>/tmp/r2 && head -c 200 /mnt/n/dists/trixie/Release" 2>/dev/null | grep -q '^Origin: Debian' &&
  ok "network storage over fuse: rclone :http: through the relay" || no "network storage over fuse: $(x $n head -c 200 /tmp/r2 2>/dev/null)"
x $n fusermount3 -u /mnt/n >/dev/null 2>&1

# 3. Start an existing, stopped instance, then exec true (5 runs).
i stop $n || no "stop"
vals=""
for _ in 1 2 3 4 5; do
  t0=$(date +%s%N); i start $n && x $n true; vals="$vals $(ms "$t0")"
  i stop -f $n
done
m=$(echo $vals | tr ' ' '\n' | median)
[ "$m" -lt 1000 ] && ok "start a stopped instance + exec true: median $m ms ($vals)" || no "start stopped: median $m ms ($vals)"

# 4. Golden copy on btrfs: copy the stopped instance, start, exec true.
vals=""; c=""
for k in 1 2 3; do
  i delete -f g$k >/dev/null 2>&1
  t0=$(date +%s%N); i copy $n g$k && i start g$k && x g$k true; vals="$vals $(ms "$t0")"
  c="$c $(i config get g$k volatile.last_state.idmap | cut -c1-20)"
done
m=$(echo $vals | tr ' ' '\n' | median)
[ "$m" -lt 1000 ] && ok "golden copy + start + exec true: median $m ms ($vals)" || no "golden copy: median $m ms ($vals)"
for k in 1 2 3; do i delete -f g$k >/dev/null; done

# 5. Golden copy with security.idmap.isolated=true (not measured before).
i config set $n security.idmap.isolated=true && i start $n && x $n true && i stop -f $n
vals=""
for k in 1 2 3; do
  i delete -f gi$k >/dev/null 2>&1
  t0=$(date +%s%N); i copy $n gi$k && i start gi$k && x gi$k true; vals="$vals $(ms "$t0")"
done
m=$(echo $vals | tr ' ' '\n' | median)
echo "INFO golden copy of an isolated instance + start + exec true: median $m ms ($vals); maps: $(for k in 1 2 3; do i exec gi$k -- cat /proc/self/uid_map </dev/null | awk '{print $2}'; done | tr '\n' ' ')"
for k in 1 2 3; do i delete -f gi$k >/dev/null; done

# 6. Limits: root.size on btrfs, memory, cpu, and lxcfs.
i delete -f lim >/dev/null 2>&1
i copy $n lim && { i config device set lim root size=1GiB 2>/dev/null || i config device override lim root size=1GiB >/dev/null; } && i config set lim limits.memory=256MiB limits.cpu=1 && i start lim || no "lim start"
x lim sh -c 'dd if=/dev/zero of=/big bs=1M count=1500 status=none' 2>/tmp/dd.err; rc=$?
[ $rc -ne 0 ] && ok "btrfs root.size=1GiB enforced (dd exit $rc: $(head -c 80 /tmp/dd.err))" || no "quota not enforced"
x lim rm -f /big
mt=$(x lim awk '/MemTotal/ {print $2}' /proc/meminfo)
[ "$mt" -le 262144 ] && ok "lxcfs: guest MemTotal $mt kB with limits.memory=256MiB" || no "guest MemTotal $mt kB"
[ "$(x lim nproc)" = 1 ] && ok "limits.cpu=1: nproc 1" || no "nproc $(x lim nproc)"
x lim python3 -c 'b=[]
while True: b.append(bytearray(16<<20))' >/dev/null 2>&1; rc=$?
[ $rc -eq 137 ] && ok "memory limit: allocation loop killed (exit 137)" || no "memory loop exit $rc"
i delete -f lim >/dev/null
i delete -f $n >/dev/null
echo "conditions: host=$(uname -srm) cpus=$(nproc) cgroup=v1 kvm=$([ -e /dev/kvm ] && echo yes || echo no) incus=$(incus version </dev/null | sed -n 's/Server version: //p') pool=$pool lxcfs=$(lxcfs --version 2>/dev/null) date=$(date -u +%FT%TZ)"
exit $fail
