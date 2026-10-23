#!/bin/sh
# Run as rl: join the delegated v1 cgroups, then start the rootless namespace.
for h in /sys/fs/cgroup/*/rl; do echo $$ > "$h/cgroup.procs" 2>/dev/null || echo "could not join $h"; done
cd /srv/rl && exec unshare --user --map-root-user --map-auto --mount ${RL_NETNS:+--net} --pid --fork --propagation private /srv/rl/rincus-ns.sh
