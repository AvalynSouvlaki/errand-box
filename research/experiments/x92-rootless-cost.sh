#!/bin/sh
# Question: does Incus run by an unprivileged user (incusd as root of that
# user's namespace) cost what root Incus and runc cost, on the same work?
# Engines, all up at once, one workload on each in turn per round:
#   runc    docker run bench:trixie, workspace bind-mounted from the host
#   incus   root incusd 6.0.6, image feat, btrfs pool, workspace disk device
#   rincus  incusd 6.0.6 as root of rl's user namespace, image feat, dir pool
# Dirs: /workspace (host directory) and /scratch (tmpfs in each sandbox).
# Run as root. rl's namespace must be up (/srv/rl/start.sh) with instance r1.
# Exit 0 measured, 1 a workload failed, 2 could not run.
set -u
runs=${RUNS:-5}
wl=${WL:-/srv/rl/workload.sh}
loads=${LOADS:-"spawn files git cpu compile"}
rl_incus() { su - rl -c "INCUS_DIR=/srv/rl/state/lib incus $*" </dev/null; }
root=/var/tmp/x92; rm -rf $root; mkdir -p $root/runc $root/incus /srv/rl/ws
chown 1000000:1000000 $root/incus
chown 2000999999:2000999999 /srv/rl/ws
docker rm -f x92 >/dev/null 2>&1
docker run -d --name x92 -v "$wl:/run/workload.sh:ro" -v $root/runc:/workspace --tmpfs /scratch:exec bench:trixie sleep infinity >/dev/null || exit 2
incus delete -f x92 </dev/null >/dev/null 2>&1
incus launch -q feat x92 -s bt </dev/null >/dev/null || exit 2
incus config device add x92 ws disk source=$root/incus path=/workspace </dev/null >/dev/null || exit 2
incus file push -q "$wl" x92/run/workload.sh </dev/null || exit 2
incus exec x92 -- sh -c 'mkdir -p /scratch && mount -t tmpfs -o exec tmpfs /scratch' </dev/null || exit 2
rl_incus config device add r1 ws disk source=/srv/rl/ws path=/workspace >/dev/null 2>&1
rl_incus file push -q "$wl" r1/run/workload.sh || exit 2
rl_incus exec r1 -- sh -c "'mkdir -p /scratch && mountpoint -q /scratch || mount -t tmpfs -o exec tmpfs /scratch'" || exit 2
sleep 2
run_one() { # engine dir load
  case $1 in
    runc)   docker exec -e WL_DIR="/$2" x92 sh /run/workload.sh "$3" ;;
    incus)  incus exec x92 --env WL_DIR="/$2" -- sh /run/workload.sh "$3" </dev/null ;;
    rincus) rl_incus exec r1 --env WL_DIR=/$2 -- sh /run/workload.sh $3 ;;
  esac 2>/dev/null | sed -n 's/^inner_ms=//p'
}
raw=$root/raw.txt; : > $raw
for dir in workspace scratch; do
  for load in $loads; do
    for _ in $(seq "$runs"); do
      for e in runc incus rincus; do
        v=$(run_one $e $dir $load); echo "$e $load@$dir ${v:--}" >> $raw
      done
    done
  done
done
failed=0; grep -q ' -$' $raw && failed=1
median() { sort -n | awk '{a[NR]=$1} END {print (NR%2 ? a[(NR+1)/2] : int((a[NR/2]+a[NR/2+1])/2))}'; }
printf '%-18s %16s %16s %16s   (median ms of %s interleaved rounds)\n' workload@dir runc incus rincus "$runs"
for dir in workspace scratch; do
  for load in $loads; do
    printf '%-18s' "$load@$dir"; base=""
    for e in runc incus rincus; do
      vals=$(awk -v e=$e -v k="$load@$dir" '$1==e && $2==k {print $3}' $raw)
      if echo "$vals" | grep -q -- -; then printf ' %16s' -; continue; fi
      m=$(echo "$vals" | median); [ -z "$base" ] && base=$m
      printf ' %9s (%sx)' "$m" "$(awk -v a=$m -v b=$base 'BEGIN {printf "%.1f", a/b}')"
    done; echo
  done
done
cp $raw /srv/rl/x92-raw.txt
docker rm -f x92 >/dev/null; incus delete -f x92 </dev/null >/dev/null
echo "conditions: host=$(uname -srm) cpus=$(nproc) cgroup=v1 (rl: delegated subtree /rl) kvm=no incus=6.0.6 root-pool=btrfs rootless-pool=dir runs=$runs date=$(date -u +%FT%TZ)"
exit $failed
