#!/bin/sh
# As rl: rootless Incus on btrfs. Launch, golden copy, isolated copy, limits.
export INCUS_DIR=/srv/rl/state/lib
i() { incus "$@" </dev/null; }
ms() { echo $(( ($(date +%s%N) - $1) / 1000000 )); }
i delete -f r1 >/dev/null 2>&1
t0=$(date +%s%N); i launch -q feat b0 -s rbt -c security.nesting=true 2>&1 | tr "\r" "\n" | grep Error; echo "launch feat on btrfs (unpack + shift): $(ms $t0) ms, state $(i list b0 -c s -f csv)"
i stop -f b0
for k in 1 2 3; do t0=$(date +%s%N); i copy b0 c$k && i start c$k 2>/dev/null && i exec c$k -- true; echo "golden copy c$k (shared map) + start + exec true: $(ms $t0) ms"; done
for k in 1 2; do t0=$(date +%s%N); i copy b0 ci$k -c security.idmap.isolated=true && i start ci$k >/dev/null 2>&1 && i exec ci$k -- true; echo "golden copy ci$k (isolated) + start + exec true: $(ms $t0) ms, map $(i exec ci$k -- cat /proc/self/uid_map | awk '{print $2, $3}')"; done
for k in 1 2 3; do i stop -f c$k; t0=$(date +%s%N); i start c$k && i exec c$k -- true; echo "start stopped c$k + exec true: $(ms $t0) ms"; done
i stop -f c1; i config set c1 limits.memory=256MiB limits.cpu=1; i config device override c1 root size=3GiB >/dev/null 2>&1 || i config device set c1 root size=3GiB; echo "quota set rc=$?"
i start c1; echo "start with limits rc=$?"
i exec c1 -- sh -c 'echo "nproc $(nproc)"; awk "/MemTotal/ {print \"MemTotal\", \$2}" /proc/meminfo; cat /sys/fs/cgroup/memory/memory.limit_in_bytes 2>/dev/null; dd if=/dev/zero of=/big bs=1M count=4000 status=none; echo "dd 4000M exit $?"; rm -f /big'
i exec c1 -- python3 -c 'b=[]
while True: b.append(bytearray(16<<20))' >/dev/null 2>&1; echo "alloc loop exit $?"
for n in b0 c1 c2 c3 ci1 ci2; do i delete -f $n; done
