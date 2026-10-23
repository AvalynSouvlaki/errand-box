# Without root: what each route keeps and loses

The operator asked whether Incus, or something the research did not examine,
can be as fast and as complete without root. "Without root" has two
readings, and the answer differs for each, so both are ranked:

- **A, no root at run time.** Root may prepare the host once: packages,
  subordinate id ranges, device modes, cgroup delegation. After that, no
  root daemon and no setuid helper other than newuidmap and newgidmap.
- **B, no root ever.** A user with no sudo installs and runs it from $HOME.

Everything in the tables was measured on the host of
[engines.md](engines.md), on 2026-10-23, unless a row says READ. A
delegated source survey, [rootless-survey.md](rootless-survey.md), supplies
the READ facts with their citations.

## The answer

- **A: yes. Incus itself runs without root, at runc speed.** incusd 6.0.6 ran
  as root of an ordinary user's user namespace and ran a system container
  with systemd as PID 1, Docker, podman and FUSE inside, at 0.6 to 1.1x the
  cost of runc. It gives up three things: the disk quota, the 0.5 s new
  instance, and direct access to the host's loopback. On a host without
  systemd and with cgroup v1, as here, "no root at run time" still means a
  root step at every boot, to delegate cgroups. Nobody had published this
  setup; the survey found no prior attempt.
- **B: no route comes near.** Without root there are no subordinate uid
  ranges, so no shared-kernel container can run a full system. What remains
  is a kernel of the guest's own, without KVM: User-Mode Linux or QEMU's TCG
  emulator. Both keep every feature inside the guest, and both cost 18 to
  53x of runc on compile work, measured.
- **The screenshot's microVM matrix was looked at before, for other
  reasons.** agent-box's own engine research (its
  `docs/research/engines-2026-10.md`) compared crosvm, libkrun, Cloud
  Hypervisor, Firecracker and QEMU for Windows support and jailing, not for
  rootless storage and network. All five need KVM except QEMU, so none of
  them can run on this host. The survey checked every cell: four are wrong
  and several are partial (section 4).

## 1. Definition A, measured

### Incus in a user namespace

Root, once: a user `rl` with the subordinate range
`2000000000:100000000`, a btrfs file system the user owns (mounted with
`user_subvol_rm_allowed`), and, because this host has cgroup v1 and no init,
cgroup subtrees `/sys/fs/cgroup/<controller>/rl` owned by the user, made
again at every boot. The user then starts incusd as root of its own user
namespace (`research/experiments/rootless/`).

| check | result |
| --- | --- |
| incusd starts; LXC driver operational | yes (idmapped mounts: no; storage: dir, btrfs) |
| systemd as PID 1 in the guest | yes, with the delegated cgroups. Without them: "Failed to create /init.scope control group: Permission denied" |
| the guest's root on the host | uid 2000999999, one of the user's own ids; no process runs as host root |
| Docker, podman, FUSE inside | yes, yes, yes |
| memory and CPU limits; lxcfs shows them | yes (nproc 1; allocation loop exit 137; MemTotal 262144 kB) |
| disk quota on btrfs | **no**: `btrfs qgroup limit` needs root; 4000 MB was written past a 3 GiB quota |
| new instance from a golden copy | 0.9 to 1.2 s with a shared uid map; 3.5 s with a range per guest (no idmapped mounts inside a user namespace, so the rootfs is rewritten) |
| start a stopped instance | 256 to 288 ms, as with root |
| a proxy device to host 127.0.0.1 | **no**: incusd probes features by creating a network namespace and cannot return to the host's from inside a user namespace, so it runs in an empty one. A relay through a unix socket fixes it (E-23) |

Interleaved cost against runc and root Incus, 5 rounds, median ms
(`research/experiments/x92-rootless-cost.sh`):

| workload@dir | runc | root Incus | rootless Incus |
| --- | --- | --- | --- |
| spawn@workspace | 423 | 416 (1.0x) | 434 (1.0x) |
| files@workspace | 1213 | 1263 (1.0x) | 952 (0.8x) |
| git@workspace | 1080 | 1043 (1.0x) | 680 (0.6x) |
| cpu@workspace | 363 | 335 (0.9x) | 331 (0.9x) |
| compile@workspace | 1734 | 1696 (1.0x) | 1646 (0.9x) |
| spawn@scratch | 437 | 424 (1.0x) | 426 (1.0x) |
| files@scratch | 112 | 118 (1.1x) | 113 (1.0x) |
| git@scratch | 115 | 122 (1.1x) | 125 (1.1x) |
| cpu@scratch | 322 | 310 (1.0x) | 319 (1.0x) |
| compile@scratch | 1320 | 1263 (1.0x) | 1226 (0.9x) |

The rootless workspace was a different host directory from the other two.
Why its files and git rows are faster was not investigated; read them as
"no slower", not as an advantage.

Three traps found on the way, each recorded so it is not paid for twice:
1. Incus takes its id range from `/etc/subuid`. Inside the namespace the
   host file has no line for root, so Incus chose `0 1000000 1000000000`,
   outside the namespace's map, and newuidmap failed. A private
   `/etc/subuid` bound over the host's, with a range inside the map, fixed
   it. The survey reads that Incus falls back to `/proc/self/uid_map` when
   newuidmap is missing (READ).
2. Mounting the user's state over `/var/lib/incus` left the host's own pool
   mount listed in mountinfo. The rootless incusd then believed its pool of
   the same name was mounted. Use `INCUS_DIR`; do not mount over the root
   daemon's paths.
3. Killing the `unshare` process does not stop the namespace: its PID 1,
   incusd, and the guests run on. And if incusd is PID 1 of the namespace,
   its death kills every guest. A rootless launcher wants a small init as
   PID 1.

### The other routes, from the survey (not measured here)

| route | Docker inside | podman inside | FUSE | systemd PID 1 | limits without root | fast clone |
| --- | --- | --- | --- | --- | --- | --- |
| LXC 6.0 run by the user, network by pasta | yes (READ) | yes (READ) | yes (READ) | with delegated cgroups (READ) | with delegated cgroups | `lxc-copy -s` (READ) |
| rootless podman with a systemd image | without limits | `--cgroups=disabled` | yes | likely fails on cgroup v1 (READ) | no | image layers |
| gVisor under rootless podman | partly | no | no | not tried | no | n/a |

gVisor's cost was measured: 1.2 to 9.0x of runc (engines.md section 1).

## 2. Definition B, measured

Run as an ordinary user with no subordinate ids, no user namespace and no
/dev/kvm. Guest kernel: Linux 6.18.58, built here. Root file system: the
same Debian 13 image as the other runs, as an ext4 file. Three runs each,
one engine at a time; runc's figures are the medians above.

| workload@dir | runc | UML, ptrace mode, 1 vCPU | UML, seccomp mode, 1 vCPU | QEMU TCG, 4 vCPUs |
| --- | --- | --- | --- | --- |
| spawn@workspace | 423 | 3374-4113 (8-10x) | 2110-2257 (5x) | 12098-12573 (29x) |
| files@workspace | 1213 | 4150-4240 (3.4x) | 3729-3911 (3.1x) | 5116-5717 (4.4x) |
| git@workspace | 1080 | 4563-4816 (4.3x) | 2779-2971 (2.7x) | 3863-4239 (3.7x) |
| cpu@workspace | 363 | 463-493 (1.3x) | 383-400 (1.1x) | 6776-7893 (21x) |
| compile@workspace | 1734 | 70481-72422 (41x) | 36654-38119 (21x) | 30010-32824 (18x) |
| spawn@scratch | 437 | 3690-3787 (8.5x) | 2140-2234 (5x) | 11520-12110 (27x) |
| files@scratch | 112 | 4253-4449 (39x) | 3666-3739 (33x) | 3498-3962 (33x) |
| git@scratch | 115 | 4497-4642 (40x) | 2673-2733 (23x) | 2363-2901 (22x) |
| cpu@scratch | 322 | 502-547 (1.6x) | 402-419 (1.3x) | 6626-8443 (22x) |
| compile@scratch | 1320 | 69142-71930 (53x) | 36558-37831 (28x) | 31200-31499 (24x) |

- Only UML's ptrace mode is a boundary. Its own help text, for the seccomp
  mode: "the SECCOMP filter is not (yet) restrictive enough to prevent
  userspace from reading and writing all physical memory"
  (`arch/um/os-Linux/start_up.c`, Linux 6.18.58).
- QEMU 8.2.2 with `-accel tcg,thread=multi -smp 4`. The survey traced the
  older "10 to 50x slower" figure to an unsourced comment (READ).
- A user without root also needs a prebuilt root file system:
  `mkfs.ext4 -d` copies each file's owner, so a user's own copy has every
  file owned by that user.

## 3. Ranked

**Definition A** (no root at run time; root may prepare the host):

1. **Incus in the operator's user namespace.** Runc speed, every feature of
   root Incus except the disk quota; new instances 1 to 3.5 s; broker access
   through a unix-socket relay. On a cgroup v1 host without systemd it needs
   a root step at each boot for cgroups. errand-box's shim already speaks to
   such a daemon (`broker_link = "unix"`); its setup is entry T-010 in
   [the work order](../../TODO/PROGRESS.md).
2. **LXC 6.0 run by the user.** The same runtime without the Incus API, so no
   copy-on-write golden copies through Incus, and errand-box would need a
   second engine. Not measured.
3. **Rootless podman.** No systemd on cgroup v1, no limits.
4. **gVisor.** 1.2 to 9x, no FUSE, Docker only in part.

**Definition B** (no root ever):

1. **QEMU TCG.** Every feature, 4 vCPUs, 18 to 33x on the work an agent does.
2. **UML, seccomp mode.** 21 to 28x on compile, one vCPU, and not a boundary.
3. **UML, ptrace mode.** A boundary, 41 to 53x on compile, one vCPU.
4. **runsc --rootless and PRoot** (READ): process sandboxes, no system.

On a host with KVM the picture for B changes: a user in the `kvm` group can
run Firecracker without its jailer, or QEMU with KVM, at near-native speed,
with one TAP device that root creates once (survey section 3, READ).

## 4. The proposed matrix, corrected

The survey checked each cell at the source. In short:

| VMM | verdict |
| --- | --- |
| libkrun | Storage partly wrong: unprivileged, virtio-fs does not keep guest ownership; chown fails. Network right (TSI or passt). Needs KVM. |
| crosvm | Network wrong: its slirp is Windows only; on Linux a TAP or vhost-user. Needs KVM. |
| Cloud Hypervisor | Network partly wrong: `fd=` takes TAP descriptors; passt attaches through vhost-user (fixed for more than one vCPU in v52). Needs KVM or MSHV. |
| Firecracker | Network partly wrong: TAP only, no socket backend. A plain user in the kvm group runs it without the jailer, with an owned TAP. Needs KVM. |
| QEMU microvm | "All need KVM" is wrong for it: TCG runs without KVM, measured above. |

Each cell, with its source file and line, is in
[rootless-survey.md](rootless-survey.md) section 2.
