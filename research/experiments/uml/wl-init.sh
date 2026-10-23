#!/bin/sh
# PID 1 inside the guest: run each workload RUNS times in /scratch (tmpfs)
# and /workspace (the guest's ext4 disk), print one line per run, then exit.
mount -t proc proc /proc; mount -t sysfs sys /sys; mount -t devtmpfs dev /dev 2>/dev/null
mkdir -p /scratch /workspace; mount -t tmpfs -o exec tmpfs /scratch
export PATH=/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin HOME=/root
echo "guest: $(uname -r) cpus=$(nproc) mem=$(awk '/MemTotal/ {print $2}' /proc/meminfo)kB"
for dir in workspace scratch; do for load in spawn files git cpu compile; do for r in $(seq ${RUNS:-3}); do
  v=$(WL_DIR=/$dir sh /workload.sh $load 2>&1 | sed -n 's/^inner_ms=//p'); echo "RESULT $load@$dir ${v:--}"
done; done; done
echo DONE
sync
