#!/bin/sh
# The definition B runs of docs/research/rootless.md section 2: the same
# workloads in User-mode Linux (ptrace and seccomp modes) and in QEMU with
# TCG, as an ordinary user with no subordinate ids, no user namespace and no
# /dev/kvm. These are the command lines that were run, kept so the numbers
# can be produced again.
#
# Inputs, in the current directory:
#   linux-6.18.58/   kernel source; its digest checked against kernel.org's
#                    sha256sums.asc
#   rootfs.img       a 4 GiB ext4 file of a Debian 13 image with python3,
#                    git, gcc and make, holding /wl-init.sh (wl-init.sh here)
#                    and /workload.sh (../workload.sh)
# Building needs flex, bison, bc and libelf-dev. The runs need nothing but
# the kernels. Output: one `RESULT <load>@<dir> <ms>` line per run, collected
# in ../../results/uml-tcg.txt.
#
# Usage: sh run.sh build | ptrace | seccomp | tcg
set -eu
step=${1:?usage: run.sh build | ptrace | seccomp | tcg}
case $step in
  build)
    # UML.
    (cd linux-6.18.58 && make -s ARCH=um defconfig &&
      ./scripts/config -e DEVTMPFS -e DEVTMPFS_MOUNT -e TMPFS -e TMPFS_POSIX_ACL -e EXT4_FS \
        -e BLK_DEV_UBD -e HOSTFS -e PROC_FS -e SYSFS -e UNIX -e INET -e FUTEX -e EPOLL -e SIGNALFD \
        -e TIMERFD -e EVENTFD -e INOTIFY_USER -e FHANDLE -e CGROUPS -e NAMESPACES -e SECCOMP \
        -e SECCOMP_FILTER &&
      make -s ARCH=um olddefconfig && make -s ARCH=um -j4 linux)
    cp linux-6.18.58/linux uml-linux
    # x86_64 for QEMU.
    (cd linux-6.18.58 && make -s mrproper && make -s defconfig && make -s kvm_guest.config &&
      ./scripts/config -e DEVTMPFS -e DEVTMPFS_MOUNT -e EXT4_FS -e VIRTIO_BLK -e VIRTIO_PCI -e TMPFS &&
      make -s olddefconfig && make -s -j4 bzImage)
    cp linux-6.18.58/arch/x86/boot/bzImage bzImage
    ;;
  ptrace | seccomp)
    # ptrace mode is the only UML mode that is a boundary; see rootless.md.
    mode=off
    [ "$step" = seccomp ] && mode=on
    cp rootfs.img "rootfs-$step.img"
    timeout 1500 ./uml-linux "ubd0=rootfs-$step.img" root=/dev/ubda rw mem=2G "seccomp=$mode" \
      init=/wl-init.sh RUNS=3 con=null con0=null,fd:1 </dev/null > "$step.out" 2>&1 || true
    grep -E '^(guest|RESULT|DONE)' "$step.out"
    ;;
  tcg)
    cp rootfs.img rootfs-tcg.img
    timeout 2400 qemu-system-x86_64 -machine q35 -accel tcg,thread=multi -cpu max -smp 4 -m 2G \
      -kernel bzImage -append "root=/dev/vda rw console=ttyS0 init=/wl-init.sh RUNS=3 panic=-1" \
      -drive file=rootfs-tcg.img,if=virtio,format=raw -nographic -no-reboot </dev/null > tcg.out 2>&1 || true
    tr -d '\r' < tcg.out | grep -E '^(guest|RESULT|DONE)'
    ;;
  *) echo "usage: run.sh build | ptrace | seccomp | tcg" >&2; exit 2 ;;
esac
