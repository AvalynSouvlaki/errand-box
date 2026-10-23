#!/bin/sh
# Runs inside: unshare --user --map-root-user --map-auto --mount --net --pid
# --fork --propagation private. Puts incusd's state on rl-owned directories
# and starts incusd as root of this user namespace.
set -u
base=/srv/rl/state
mkdir -p $base/lib $base/log $base/cache $base/run
# Keep the host cgroup tree before /sys is replaced.
mkdir -p $base/run/cg && mount --rbind /sys/fs/cgroup $base/run/cg
[ -n "${RL_NETNS:-}" ] && { mount -t sysfs sysfs /sys || echo "sysfs mount failed"; }
mount --rbind $base/run/cg /sys/fs/cgroup 2>/dev/null || mount -t tmpfs cgroup /sys/fs/cgroup
mount -t proc proc /proc || echo "proc mount failed"
mount -t tmpfs tmpfs /run
mkdir -p /run/incus
mount --bind $base/lib /var/lib/incus
mount --bind $base/log /var/log/incus
mount --bind $base/cache /var/cache/incus 2>/dev/null || true
mount --bind /srv/rl/state/subid /etc/subuid
mount --bind /srv/rl/state/subid /etc/subgid
[ -n "${RL_NETNS:-}" ] && ip link set lo up
echo "ns ready: pid $$ uid_map: $(tr -s ' ' < /proc/self/uid_map | tr '\n' ';')"
export INCUS_DIR=/var/lib/incus
exec /opt/incus/lib/systemd/incusd --group root --logfile /var/log/incus/incusd.log --debug
