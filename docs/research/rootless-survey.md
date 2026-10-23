# Rootless sandbox engines for the no-KVM host: a survey, 2026-10-23

A delegated survey, kept as its author wrote it. It READ sources and
measured nothing; [rootless.md](rootless.md) holds the measurements and the
ranking that superseded its untested recipes. Its section 13 plan was carried
out there.

The question: is there a way to get what Incus 6.0 gives on this host
(Docker and podman inside, FUSE and network storage over FUSE, systemd as
PID 1, runc speed, enforced limits, a new instance in about 0.4 s, persistent
instances) without a root daemon? Two definitions:

- **A**: no root at run time. One-time root setup is allowed. No root daemon,
  no setuid helper other than newuidmap and newgidmap.
- **B**: no root ever. The user installs and runs everything from $HOME.

The host: Linux 6.18 x86_64, 4 vCPUs, 16 GB, no /dev/kvm, cgroup v1 without
delegation, PID 1 is not systemd.

## How this was done

- curl through search.web.pkgforge.dev, read.web.pkgforge.dev and
  forge.web.pkgforge.dev (GitHub REST). Shallow clones into
  `scratchpad/survey-src/`. Nothing was installed, built or run, except the
  read-only probes in section 1.
- **READ** means the statement was found at the URL or at the source lines
  cited. Every URL marked READ was read on **2026-10-23**. **INFERRED** means
  a conclusion from READ facts or general knowledge that nobody checked.
  **MEASURED** means a probe ran on this host on 2026-10-23.
- Source citations use `owner/repo@<12-char commit>:path:lines`. Full pins:

| tree | commit | what it is |
| --- | --- | --- |
| containers/libkrun | a1e5398dab8e | main, 2.0.0-dev, 2026-10-20 (latest release v1.19.7, 2026-10-22) |
| google/crosvm | 96e84c3f680e | main, 2026-10-21 |
| cloud-hypervisor/cloud-hypervisor | 5b346485defc | main, 2026-10-22 (latest release v54.0, 2026-10-14) |
| firecracker-microvm/firecracker | 6e52c0315ba8 | main, 2026-10-22 (latest release v1.17.0, 2026-09-10) |
| qemu/qemu | 4f4225874d4d | master, VERSION 11.1.50 |
| virtio-fs/virtiofsd (gitlab) | 9c1a575eb4d8 | 1.14.0 |
| passt (passt.top) | 8ba41320c3a2 | 2026-10-14 |
| lxc/incus (stable-6.0) | 7678ccc46b2d | 2026-10-16 |
| lxc/incus (main) | 04e89b422ba5 | 2026-10-22 (docs only) |
| lxc/lxc (stable-6.0) | 5c5afa6e5e11 | 6.0.6 |
| systemd/systemd | c676df83262c | main, "263 in spe" |
| torvalds/linux | 734bef548a96 | 7.3 plus the net-next 7.4 merge, 2026-10-22 |
| lima-vm/lima | c30ccbe435ad | main, 2026-10-22 |

## 0. The answer in short

- **There is no supported rootless Incus.** Upstream says incusd stays root
  (lxc/incus#923). But incusd has first-class code for running as root
  *inside a user namespace* (its nesting mode), and it takes its id range
  from `/proc/self/uid_map` when it cannot use /etc/subuid. So a normal user
  can plausibly start stock incusd 6.0 under rootlesskit or `unshare` with
  pasta for egress. No published attempt was found. It is the first thing
  to test (section 4, candidate A1).
- **On cgroup v1 without delegation, no rootless container engine can run
  systemd as PID 1 or enforce limits.** That needs a root step at every boot
  that makes and chowns cgroup subtrees, which is what pam_cgfs or systemd's
  `Delegate=` do. On this init-less host that step belongs in the existing
  root boot script. So for A, "no root at run time" means "root at boot".
- **Every microVM in the proposed matrix needs KVM, except QEMU.** Without
  KVM the only rootless kernel boundaries are User-Mode Linux and QEMU TCG.
  Both keep every Incus feature *inside* the guest and enforce memory, CPU
  and disk limits without cgroups. UML's secure mode has one vCPU, and its
  SMP (Linux 6.19) works only in the seccomp mode that upstream calls
  insecure. TCG costs about 8-12x on boot workloads; the "10-50x" figure that
  ruled it out is an unsourced comment.
- systemd-nspawn's unprivileged mode depends on two root daemons
  (systemd-nsresourced, systemd-mountfsd), BPF-LSM and a systemd host. It
  fails definition A by its own rule and does not run on this host.

## 1. Host facts used here (MEASURED, read-only)

| probe | result |
| --- | --- |
| `/proc/self/cgroup`, `/proc/self/mounts` | hybrid layout: v1 controllers cpu, cpuacct, cpuset, memory, devices, freezer, blkio, pids and `name=systemd`, plus cgroup2 at `/sys/fs/cgroup/unified` (ownership not probed; engines-shortlist.md records no delegation) |
| `/dev/kvm` | absent |
| `/dev/fuse`, `/dev/net/tun` | `crw-rw-rw-` (set by 75-host-up.sh; they reset to 0600 at boot, engines-shortlist.md 9.1) |
| `newuidmap`, `newgidmap`, `pasta`, `slirp4netns` | present in /usr/bin |
| `/proc/sys/user/max_user_namespaces` | 64280 |

## 2. The matrix, corrected

Two columns are added: whether KVM is needed, and snapshot or fast clone.
Numbers in brackets point to the source list under the table.

| Hypervisor | Language | Rootless Storage | Rootless Network | Sandboxing Model | Needs KVM (added) | Snapshot / fast clone (added) |
| --- | --- | --- | --- | --- | --- | --- |
| libkrun | **Correct.** Rust. | **Partial.** Built-in virtio-fs (host dir) is right, and virtio-blk raw or qcow2 exists too [1]. Unprivileged, ownership is *not* preserved: guest uid 0 and the VMM's own uid are accepted, any other uid gets EPERM on create, and chown is a real `fchownat` that fails [2]. Preserved only when the VMM runs as root in a userns with a subuid map, as under crun's krun handler [3]. | **Correct**, plus gvproxy [4]. TSI: TCP and UDP over INET/INET6, AF_UNIX by absolute path; no raw sockets, no ICMP ping sockets, no netlink; needs libkrunfw's kernel [4]. A guest `listen()` becomes a host `listen()` in the VMM's netns [5]. | **Correct, with a caveat.** libkrun applies no seccomp or namespaces of its own; the embedder must [6]. crun's krun handler sets up namespaces and cgroups, then calls libkrun [3]. | Yes on Linux. | **None.** Pause and resume return FeatureDisabled on Linux [7]. Clone only by copying the disk (qcow2 backing file or reflink; INFERRED). |
| crosvm | **Correct.** Rust. | **Correct**, plus block (raw, qcow2 is a default feature, composite) [8]. The fs device runs in a minijail with a user namespace; its default uid map is `0 <euid> 1`, so a plain user maps one guest uid [9]. | **Wrong for Linux.** The slirp feature is "Currently only supported on Windows" and the module is `#![cfg(windows)]` [10]. On Linux: a TAP by name (the docs make a persistent TAP with `ip tuntap add mode tap user $USER`), a TAP fd, or a vhost-user net backend [11]. | **Correct** on Linux: one minijail per device, with user, PID, mount and net namespaces and seccomp [12]. Works as a plain user (userns-based; INFERRED from [9]). The main process has no seccomp policy (docs/research/engines-2026-10.md). | Yes. | **Effectively none.** Snapshots are "100% not supported" [13]. qcow2 backing files for disk clones (INFERRED). |
| Cloud Hypervisor | **Correct.** Rust. | **Correct**, plus virtio-blk raw, qcow2 and VHD(X), and vhost-user-blk. virtio-fs needs `--memory shared=on`; virtiofsd runs unprivileged (Q1 below) [14]. | **Partial.** `fd=` takes TAP or macvtap fds, not a passt or slirp socket [15]. passt attaches through `vhost_user=true,socket=...` with shared memory; a crash with more than one vCPU was fixed by PR #7906, merged by 2026-03-27 (so v52.0 and later) [16]. A pre-made, already-up TAP the user owns works without CAP_NET_ADMIN, if no `host_mac` or `mtu` option asks CH to change it; an `ip` option on an existing TAP is skipped [17]. slirp4netns has no direct path. | **Correct**, plus opt-in Landlock [18]. No jailer. | Yes (or MSHV). | **Yes.** Snapshot and restore; restore modes eager, on-demand (userfaultfd) and, since v54.0, copy-on-write that shares the page cache across clones [19]. vhost-user devices block on-demand restore; shared memory forces the eager copy [19]. |
| Firecracker | **Correct.** Rust. | **Partial.** virtio-block over raw files, plus virtio-pmem (file-backed) and vhost-user-block (developer preview) [20]. No virtio-fs, no qcow2 in process. | **Partial.** TAP only, by `host_dev_name`; no socket or vhost-user net backend, so no passt path [21]. A plain user can open a persistent TAP it owns [22]; the Go SDK's tests do exactly that [23]. | **Correct.** The default seccomp filters apply without the jailer [24]; the jailer runs as root [25]. Unjailed, there is no chroot, namespace or cgroup unless the caller adds them. | Yes. | **Yes.** Full and diff snapshots; clones need a netns each for the same TAP name, and care for uniqueness [26]. |
| QEMU microvm | **Correct.** C. | **Partial.** Any block format, including qcow2 backing files for instant clones; virtio-fs via vhost-user with a shared memory backend. microvm is virtio-mmio only, up to eight devices [27]. | **Partial.** libslirp has been an external, optional library since 7.2 [28]. passt works via `-netdev stream`, and `-netdev passt` (with `vhost-user=on`, which needs shared memory) [29]. | **Correct.** `-sandbox on` is a deny list over a default of allow (engines-2026-10.md). | **No.** TCG runs without KVM, with one thread per vCPU for x86 guests on x86 hosts [30]. The heading "all need KVM on Linux" is wrong for QEMU. | **Yes.** savevm/loadvm, migration to a file (mapped-ram), and "fast snapshot load" from a file through userfaultfd in master docs [31]. |

Sources for the matrix:

1. READ `containers/libkrun@a1e5398dab8e:docs/vm-lifecycle.md:42-48`.
2. READ `containers/libkrun@a1e5398dab8e:src/devices/src/virtio/fs/linux/passthrough.rs:447-460` (own uid kept only without CAP_SETUID), `:779-808` (`set_creds`: other uids return EPERM), `:1352-1381` (`fchownat`). The xattr `user.containers.override_stat` is used only in the macOS and Windows passthroughs (`.../fs/macos/passthrough.rs:37`, `.../fs/windows/passthrough.rs:58`).
3. READ https://github.com/containers/crun/blob/main/krun.1.md ("crun: sets up the container namespaces (user, pid, mount, ipc, uts, cgroup) ... then hands off execution to libkrun").
4. READ `containers/libkrun@a1e5398dab8e:docs/networking.md:9-18,44-48,52-54`.
5. READ `containers/libkrun@a1e5398dab8e:src/devices/src/virtio/vsock/tsi_stream/unix.rs:88-120`.
6. READ `containers/libkrun@a1e5398dab8e:docs/security-model.md:9-15,32-36`. A search of `src/` finds no seccomp use.
7. READ `containers/libkrun@a1e5398dab8e:docs/vm-lifecycle.md:66`.
8. READ `google/crosvm@96e84c3f680e:Cargo.toml:236`, `docs/book/src/devices/fs.md:1-19`.
9. READ `google/crosvm@96e84c3f680e:jail/src/helpers.rs:187-220`, `src/crosvm/sys/linux/config.rs:115-119`.
10. READ `google/crosvm@96e84c3f680e:Cargo.toml:405-406`, `net_util/src/slirp.rs:9`.
11. READ `google/crosvm@96e84c3f680e:docs/book/src/devices/net.md:5-37`, `docs/book/src/devices/vhost_user.md:3`.
12. READ `google/crosvm@96e84c3f680e:docs/book/src/appendix/minijail.md:3-22`.
13. READ `google/crosvm@96e84c3f680e:docs/book/src/architecture/snapshotting.md:3-4`.
14. READ `cloud-hypervisor/cloud-hypervisor@5b346485defc:docs/fs.md:15-39`.
15. READ `cloud-hypervisor/cloud-hypervisor@5b346485defc:vmm/src/config.rs:1882-1891`, `docs/macvtap-bridge.md:23-29`.
16. READ https://github.com/cloud-hypervisor/cloud-hypervisor/issues/7766 (opened 2026-02-27, closed 2026-03-27; the reporter confirms PR #7906 fixes it); releases v52.0 2026-05-14, v53.0, v54.0 2026-10-14 (GitHub releases API).
17. READ `cloud-hypervisor/cloud-hypervisor@5b346485defc:net_util/src/open_tap.rs:63-101` (opens an existing TAP by name, configures IP only if asked) and `net_util/src/tap.rs:457-476` (`enable()` returns early if the TAP is already up). `docs/building.md:48-49` uses `setcap cap_net_admin` only so that CH can *create* TAPs.
18. READ `cloud-hypervisor/cloud-hypervisor@5b346485defc:docs/landlock.md:1-8`.
19. READ `cloud-hypervisor/cloud-hypervisor@5b346485defc:docs/snapshot_restore.md:1-9,128-146,156-170`, `release-notes.md:609-611` (v54.0).
20. READ `firecracker-microvm/firecracker@6e52c0315ba8:docs/device-api.md:17`, `docs/pmem.md:1-12`, `docs/api_requests/block-vhost-user.md:1-10`.
21. READ `firecracker-microvm/firecracker@6e52c0315ba8:src/firecracker/swagger/firecracker.yaml:1690-1702`, `src/vmm/src/devices/virtio/net/tap.rs:124-147`.
22. READ `torvalds/linux@734bef548a96:drivers/net/tun.c:515-527` (`tun_permitted`: owner or group match, else CAP_NET_ADMIN in the netns's userns).
23. READ https://github.com/firecracker-microvm/firecracker-go-sdk/blob/main/HACKING.md ("A tap device owned by your userid ... `sudo ip tuntap add fc-test-tap0 mode tap user $UID`"; only the jailer tests need sudo).
24. READ `firecracker-microvm/firecracker@6e52c0315ba8:docs/seccomp.md:3-4`, `docs/getting-started.md:25-83` (a plain user needs rw on /dev/kvm, by ACL or the kvm group).
25. READ `firecracker-microvm/firecracker@6e52c0315ba8:docs/jailer.md:295-296`.
26. READ `firecracker-microvm/firecracker@6e52c0315ba8:docs/snapshotting/network-for-clones.md:14-20`, `docs/snapshotting/snapshot-support.md:551-559`.
27. READ `qemu/qemu@4f4225874d4d:docs/system/i386/microvm.rst:25-27,63` (no KVM requirement; kvmclock only "if using KVM").
28. READ `qemu/qemu@4f4225874d4d:meson_options.txt:247-248` (`slirp` is an `auto` feature). The 7.2 removal of the slirp submodule is from a search snippet of https://wiki.qemu.org/ChangeLog/7.2 and the 7.2 patch series (https://lists.libreplanet.org/archive/html/qemu-devel/2022-08/msg03650.html); wiki.qemu.org answered the reader with a bot wall, so the page itself was not read.
29. READ `qemu/qemu@4f4225874d4d:docs/system/devices/net.rst:86-175`. READ (GitHub API): commit da703b06a52b "net/passt: Implement vhost-user backend support", authored 2025-07-09. The 10.1 soft freeze of 2025-07-15 is from a search snippet of https://wiki.qemu.org/Planning/10.1. That the passt netdev shipped in 10.1 is INFERRED.
30. READ `qemu/qemu@4f4225874d4d:docs/devel/multi-thread-tcg.rst:28-33`, `target/i386/tcg/tcg-cpu.c:163` (`.mttcg_supported = true`).
31. READ `qemu/qemu@4f4225874d4d:docs/devel/migration/mapped-ram.rst:1-8`, `docs/devel/migration/fast-snapshot-load.rst:1-17`. Which release first carries fast snapshot load was not checked.

## 3. Q1, the sub-questions

**Is crosvm's slirp Linux or Windows only?** Windows only (matrix [10]).

**Can Firecracker run without the jailer as a plain user in the kvm group?**
Yes. Its getting-started guide only asks for rw on /dev/kvm (ACL or kvm
group), and its default seccomp filters still apply (matrix [24]). What is
lost unjailed: chroot, mount/PID/net namespaces, the uid drop and the cgroup
placement. Those are the caller's job then.

**Can it use a TAP the user owns?** Yes. The kernel lets the owner (or group)
of a persistent TAP attach to it without CAP_NET_ADMIN (matrix [22]), and
Firecracker only opens `/dev/net/tun` and calls TUNSETIFF, TUNSETOFFLOAD and
TUNSETVNETHDRSZ (matrix [21]). The Go SDK's test suite runs this way (matrix
[23]). Root must create the TAP once per boot (`ip tuntap add tap0 mode tap
user <u>`) and give it routing or NAT.

**A passt or vhost-user path for Firecracker?** No. Its only vhost-user
device is block, and the net device takes a TAP name only (matrix [20],
[21]). A rootless route without any root step: run Firecracker inside a
pasta-made user and network namespace, create the TAP there (the user has
CAP_NET_ADMIN in that namespace), and route or NAT it to pasta's interface
with nftables inside the namespace. INFERRED; untested. A 2026 process
sandbox (cwage/agentpen) states that nft rules inside a pasta userns work
without host privilege (READ https://github.com/cwage/agentpen README).

**Does Cloud Hypervisor support vhost-user-net with passt?** Yes, with
`memory shared=on`, after the fix in PR #7906 (matrix [16]). passt's own man
page still says "the only supported hypervisor is qemu" (READ
`passt@8ba41320c3a2:passt.1:53-55`), so CH is not a tested combination
upstream at passt.

**libkrun virtio-fs and guest uid/gid when unprivileged.** See matrix [2]:
without CAP_SETUID/CAP_SETGID, creating a file as any guest uid other than 0
or the VMM's own uid fails with EPERM, and chown fails. Run under a userns
with a subuid map (podman unshare, `unshare --map-auto`, or crun's krun
handler), the per-request `setresuid` works and ownership is kept.

**What does TSI not support?** Raw sockets, ICMP ping sockets and netlink;
AF_UNIX only by absolute path (matrix [4]). It does support listening sockets
and UDP. Listening is done by the VMM on the host side, in the VMM's netns,
with an optional port map (matrix [5]). So two guests that listen on the same
port collide unless each VMM has its own netns. INFERRED: Docker inside a
TSI-only guest cannot build bridges or NAT, so Docker inside needs virtio-net
(passt or gvproxy).

**Does virtiofsd run unprivileged?** Yes. It "must be run as the root user or
as a 'fake' root inside a user namespace" (READ
`virtio-fs/virtiofsd@9c1a575eb4d8:README.md:51-63`). As a plain user:

- `--sandbox namespace` (the default) makes its own mount, PID and net
  namespaces; with `--uid-map`/`--gid-map` it calls newuidmap with the
  user's subuid range, else it maps the current uid 1:1 (READ
  `README.md:248-270,321-356`).
- `--sandbox chroot` under `podman unshare`, `unshare -r --map-auto` or
  `lxc-usernsexec` (READ `README.md:534-597`).
- `--translate-uid`/`--translate-gid` map guest ids to the single host id
  with no user namespace at all (READ `README.md:358-404`).
- Limits: no device nodes in the share, no file handles, so many open fds,
  and no raising RLIMIT_NOFILE (READ `README.md:597-605`).

Note: the Cloud Hypervisor docs still show `sudo setcap cap_sys_admin+epi`
for virtiofsd (READ `cloud-hypervisor@5b346485defc:docs/fs.md:22`). That is
not required by virtiofsd itself.

**Which have snapshot/restore or fast clone?** Firecracker (snapshots, clones
with per-clone netns), Cloud Hypervisor (snapshots; v54.0 copy-on-write
restore), QEMU (savevm, migration file, mapped-ram, qcow2 backing files).
crosvm: experimental and unsupported. libkrun: none. Disk-level clones
(qcow2 backing files, reflink copies) work for all but Firecracker's raw-only
block device, where a reflink copy on btrfs or XFS is the route (INFERRED).

## 4. Q2, rootless Incus

**Upstream position.** "Incus remains running as fully privileged root";
for unprivileged use, upstream points to LXC (READ
https://github.com/lxc/incus/issues/923, stgraber, 2024-06-07). In 2019 the
same maintainer wrote of LXD: "LXD can absolutely work nested inside an
unprivileged container ... LXD effectively runs as a root inside a user
namespace, so as a globally unprivileged user" (READ
https://lwn.net/Articles/796861/).

**What the 6.0 code does inside a user namespace (READ, incus stable-6.0
@7678ccc46b2d):**

- It detects a user namespace from the first line of `/proc/self/uid_map`
  (`internal/linux/userns.go:9-31`) and has 26 code paths keyed on it, for
  seccomp, AppArmor, devices, btrfs and nftables.
- **Where the id range comes from** (`internal/server/sys/os.go:267-340`,
  `shared/idmap/set_load.go:170-176`):
  1. If /etc/subuid, /etc/subgid, newuidmap and newgidmap all exist, it
     uses the entries for **root** in /etc/subuid and /etc/subgid. If root
     has none, it logs an error and allows privileged containers only.
  2. Otherwise it reads `/proc/self/uid_map` and splits off a range of at
     least 65536 ids starting at namespace id 1,000,000, else at 65,536.
  3. Otherwise only privileged containers work.
- **Consequence:** inside a user-made namespace on this host, /etc/subuid
  and newuidmap are visible, so step 1 would pick host ids that the
  namespace cannot map. Hiding newuidmap inside the namespace's private
  mount namespace (bind `/dev/null` over it) makes step 2 apply. LXC needs
  the same: it uses newuidmap whenever one is on PATH and privileged
  (READ `lxc/lxc@5c5afa6e5e11:src/lxc/idmap_utils.c:163-202`), and without
  it, as root in the namespace, it writes the maps itself. INFERRED.
- **Storage inside a user namespace:** only `dir`, and `btrfs` when the
  source path is on btrfs (`internal/linux/storage.go:33-41`). The docs'
  feature table says dir, btrfs and ZFS (with `zfs.delegate`) are usable
  inside a container (READ `lxc/incus@04e89b422ba5:doc/reference/storage_drivers.md:35`),
  and nested btrfs "does not inherit the Btrfs quotas from the parent"
  (`doc/reference/storage_btrfs.md:27-29`). The btrfs pool's default mount
  option is `user_subvol_rm_allowed` (`doc/config_options.txt:5385-5386`).
  The kernel lets anyone create a subvolume, snapshot only subvolumes they
  own, and delete one only with that mount option (READ
  `torvalds/linux@734bef548a96:fs/btrfs/ioctl.c:1203-1207,2383-2399`).
- **Too few ids:** `incus admin init` detects the namespace and offers to
  set `security.privileged=true`, making nested containers share the
  parent's allocation (READ incus-6.0 `cmd/incus/admin_init_interactive.go:540-558`).
  stgraber: "'privileged' containers running inside of an unprivileged
  container are still effectively unprivileged containers" (READ
  https://discuss.linuxcontainers.org/raw/23146/2).
- **Paths:** `INCUS_DIR` moves var, cache, log and run under one directory
  (READ incus-6.0 `internal/util/paths.go:8-61`), so the whole daemon state
  can live in $HOME.

**Networking.** INFERRED: incusbr0, dnsmasq and the nftables NAT live in
the namespace's own netns, where the namespace root has CAP_NET_ADMIN.
Egress goes out through pasta or slirp4netns attached to that netns
(rootlesskit `--net=pasta` or `--net=slirp4netns`; READ
https://github.com/rootless-containers/rootlesskit README, v3.3.0 of
2026-10-14). pasta maps the gateway address to the host's loopback by
default, which is how guests would reach this host's CONNECT proxy
(INFERRED from the passt man page). On this host, nftables inside an Incus
container's userns already worked for podman's bridge (engines-shortlist.md
section 8).

**cgroups.** INFERRED: on this host's non-delegated v1, the namespace sees
every hierarchy read-only. LXC 6.0 skips unwritable legacy hierarchies and
starts the container with an "unwritable cgroup layout" (READ
`lxc/lxc@5c5afa6e5e11:src/lxc/cgroups/cgfsng.c:3784-3792,4026-4027,4072-4076`).
So containers start, with no limits, and systemd as PID 1 inside them fails
on a read-only cgroupfs ("Failed to create /init.scope control group:
Read-only file system ... Exiting PID 1", READ
https://github.com/moby/moby/issues/42275). The fix is a root step at each
boot that creates cgroup subtrees in `name=systemd` and `unified` (and, for
ceilings, in memory, pids and cpu, with the limits set on the root-owned
parent) and chowns them to the user, then starts the user's launcher inside
them. LXC chowns the per-container part to the container's root itself
(READ `cgfsng.c:1950-2031`).

**lxcfs.** "A properly configured container with both cgroup namespaces and
fuse setup can run lxcfs just fine" (READ
https://github.com/lxc/lxcfs/issues/254, stgraber, 2020-03-03). FUSE mounts
are allowed in a user namespace (READ `torvalds/linux@734bef548a96:fs/fuse/inode.c:2008`).
So lxcfs can run as the namespace's root (INFERRED).

**Community evidence.** A search of lxc/incus and canonical/lxd issues for
"rootless" found nothing on this topic (GitHub search API). No blog or
forum post was found that starts incusd under rootlesskit or `unshare` as a
user. The documented path is Incus nested in an unprivileged container,
which upstream and users run routinely (READ
https://discuss.linuxcontainers.org/t/incus-container-inside-incus/23146,
Incus 6.0.3, 2025).

**Verdict.** Unsupported, untested, but the code paths it would need are the
nesting paths that upstream maintains. It keeps the Incus API, images,
`incus copy` golden copies on btrfs, profiles, nesting, FUSE and runc speed.
It loses limits and systemd as PID 1 unless root delegates cgroups at boot,
disk quotas always, idmapped mounts (INFERRED: they need privilege over the
host filesystem's user namespace, so Incus falls back to the chown rewrite,
which a golden copy avoids, engines-shortlist.md 10.3), and anything that
needs init-namespace root (kernel modules, host sysctls, device passthrough).

## 5. Q3, unprivileged LXC 6.0 run by a normal user

- **Requirements** (READ https://linuxcontainers.org/lxc/getting-started/):
  libpam-cgfs, shadow's newuidmap and newgidmap, a subuid/subgid range,
  `/etc/lxc/lxc-usernet` for veth quota, `~/.config/lxc/default.conf` with
  `lxc.idmap` lines, and on cgroup v2 `systemd-run --user --scope -p
  "Delegate=yes"` around every `lxc-*` command. Use the `download`
  template.
- **lxc-user-nic is setuid root** (READ
  `lxc/lxc@5c5afa6e5e11:doc/lxc-user-nic.sgml.in:54-56`). The non-setuid
  option: `lxc.net.0.type = empty`, which "will create only the loopback
  interface" (READ `doc/lxc.container.conf.sgml.in:433-435`), plus pasta
  attached to the container's PID. LXC exports `LXC_PID` before running the
  `start-host` hook (READ `src/lxc/start.c:1767,1919`;
  `doc/lxc.container.conf.sgml.in:2585-2591`), and pasta "associates to an
  existing user and network namespace" when given a PID (READ
  `passt@8ba41320c3a2:passt.1:64-71`). INFERRED: a one-line hook
  `exec pasta --config-net "$LXC_PID"` gives egress with no setuid helper.
  No published example of this was found; podman does the same thing for
  its own containers.
- **cgroup v1 vs v2.** On v1, LXC 6.0 runs without writable hierarchies
  (section 4) but then has no limits, and a systemd guest fails. pam_cgfs
  (or a root boot step that does the same) gives a writable per-user
  subtree. On v2, the systemd-run scope gives it. LXC 7.0 dropped v1
  (runtime-survey.md), so this host stays on LXC 6.0 LTS.
- **systemd guest:** works when the cgroup subtree is writable (INFERRED
  from LXC's chown logic, section 4); fails on read-only cgroups.
- **Docker inside:** `lxc.include = /usr/share/lxc/config/nesting.conf`
  (READ `config/templates/nesting.conf.in:1-8`: it adds proc and sys mounts
  for nested use). INFERRED: dockerd as root in the container's userns then
  behaves like rootless Docker, whose `--cpus`, `--memory` and
  `--pids-limit` "are ignored ... on cgroup v1 mode" (READ
  https://docs.docker.com/engine/security/rootless/troubleshoot/). Not
  tested in the read-only-cgroup case.
- **podman inside:** `--cgroups=disabled` worked inside an unprivileged
  Incus container on this host (engines-shortlist.md section 8). Same
  expectation here (INFERRED).
- **Storage backends.** `lxc-copy -s` makes "small copy-on-write copies" on
  btrfs, lvm, overlay and zfs, and "overlay snapshots of directory backed
  containers are exempted" from the same-type rule (READ
  `doc/lxc-copy.sgml.in:93-102`). Overlayfs mounts are allowed in a user
  namespace (READ `torvalds/linux@734bef548a96:fs/overlayfs/super.c:1596`).
  For a user: dir, overlay snapshots, and btrfs on a btrfs path (subject to
  the snapshot and delete rules above); lvm and zfs need root. INFERRED.
- **Quotas:** none for a user. btrfs qgroups and loop or LVM backends need
  root (INFERRED).

## 6. Q4, systemd-nspawn unprivileged (nsresourced and mountfsd)

- **v256 (2024-06-11):** systemd-nsresourced (assigns a transient 64K
  range to a user namespace, and delegates mounts, cgroups and network
  interfaces to it) and systemd-mountfsd (mounts DDI images; trust needs
  Verity or interactive polkit). "systemd-nspawn may now operate without
  privileges, if a suitable DDI is provided via --image=" (READ
  `systemd/systemd@c676df83262c:NEWS:6368-6395`). The same release made
  systemd refuse to boot on cgroup v1 unless
  `SYSTEMD_CGROUP_ENABLE_LEGACY_FORCE=1` (READ `NEWS:5446-5452`).
- **v258 (2025-09-17):** directory trees owned by the new "foreign" UID
  range work unprivileged; nsresourced can allocate a TAP for unprivileged
  VMMs; vmspawn gets unprivileged networking (READ `NEWS:3813-3866,3898`).
  cgroup v1 support removed entirely (READ `NEWS:2988-2990`).
- **v260 (2026-03-17):** up to 16 extra delegated 64K ranges for nested user
  namespaces, and `PrivateUsers=managed` (READ `NEWS:2005-2008,2203-2240`).
- **Limits of the mode:** only `--image=`, or `--directory=` owned by the
  foreign range; only `--private-network` and `--network-veth` (READ
  `man/systemd-nspawn.xml:125-143`). nsresourced enforces a BPF-LSM policy
  (READ `man/systemd-nsresourced.service.xml`, Description). "Bpf-lsm is a
  mandatory dep for this to work" (READ
  https://github.com/systemd/systemd/issues/35387, poettering, 2025-05-05).
  The same open issue shows it failing for users through 258.7 on Fedora
  with SELinux enforcing (comment of 2026-06-13).
- **Requirements:** a systemd host running the two services, polkit, BPF-LSM
  enabled at boot, cgroup v2 for 258 and later. Docker, podman and FUSE
  inside were not found documented for this mode.
- **Verdict:** nsresourced and mountfsd are root daemons, so this fails
  definition A by its own rule, and fails definition B. It cannot run on
  this host: no systemd, cgroup v1, and BPF-LSM is compiled in but not
  enabled (incus-gaps-survey.md, gap e). First usable release: v256 for
  signed images, v258 for directory trees.

## 7. Q5, User-Mode Linux in 2025-2026

- **Seccomp-based process mode:** merged in Linux 6.16. The UML pull
  request says it "still isn't enabled by default due to security concerns
  [but] can still be used e.g. for tests" (READ
  https://www.spinics.net/lists/kernel/msg5710692.html, 2025-06-05). The
  boot option is `seccomp=on|auto|off`, default off, and its help text says
  the filter "is not (yet) restrictive enough to prevent userspace from
  reading and writing all physical memory ... This is insecure and should
  only be used with a trusted userspace" (READ
  `torvalds/linux@734bef548a96:arch/um/os-Linux/start_up.c:423-460`).
- **SMP:** merged in 6.19, "limited ... in that threads within a single
  process cannot run concurrently" (READ https://lwn.net/Articles/1049424/).
  `CONFIG_SMP` defaults to n (READ `arch/um/Kconfig:95-116`). And:
  `fatal("SMP is not supported with PTRACE userspace.")` (READ
  `arch/um/os-Linux/start_up.c:485-486`). **So the secure ptrace mode is
  single-vCPU; SMP needs the mode upstream calls insecure.**
- **Numbers.** lmbench, from the nommu-UML author, 2025-11 (READ
  https://lists.openwall.net/linux-kernel/2025/11/12/457):

  | test | native | um (ptrace) | um-mmu, seccomp | um-nommu, seccomp |
  | --- | --- | --- | --- | --- |
  | syscall (us) | 0.16 | 35.0 | 53.2 | 2.6 |
  | stat (us) | 1.89 | 43.8 | 49.6 | 3.2 |
  | open/close (us) | 3.30 | 77.5 | 68.9 | 6.3 |
  | fork+execve (us) | 511 | 2834 | 2461 | 140 |
  | fork+sh (us) | 1110 | 7360 | 4619 | 439 |

  nommu UML needs NOMMU-aware binaries ("There is no stock x86_64 NOMMU
  distribution"; READ `Documentation/virt/uml/nommu-uml.rst:41-46`), so it
  cannot run Debian. Compile: a 2007 kernel build took 8m38s native, 11m12s
  under KVM and 14m30s under UML skas0 (1.68x), single job (READ
  https://blogs.igalia.com/berto/2007/06/27/comparing-virtualization-software-performance-qemu-vs-uml-vs-kvm/).
  A 2025 SMP prototype built a kernel in 6m02s with 8 vCPUs and
  `seccomp=on`, with no native baseline (READ
  the linux-um list archive, the UML SMP series posted on 2025-07-13).
  INFERRED for this host: CPU-bound compile at 1.5-3x of runc on one vCPU,
  process-heavy work (git, cargo, configure) at 5x or more. Not measured.
- **Rootless networking:** the legacy transports, including the built-in
  slirp, were removed in 6.16 (READ the 6.16 pull above, "remove obsolete
  network transports"). The vector driver remains with raw, hybrid, tap,
  gre, l2tpv3, bess, fd and vde transports (READ
  `arch/um/drivers/vector_user.c:43-50,746-760`). Without privilege: tap on
  a persistent TAP the user owns, l2tpv3 in UDP mode, bess, and
  `vde,vnl=slirp://` (READ `Documentation/virt/uml/user_mode_linux_howto_v2.rst:227-242,316-330,520-578`).
  `virtio_uml` attaches vhost-user devices with
  `virtio_uml.device=<socket>:<virtio_id>` (READ
  `torvalds/linux@734bef548a96:arch/um/drivers/virtio_uml.c:12-19`); passt
  over vhost-user into UML is INFERRED possible and untested.
- **Storage:** `ubd0=cow_file,backing_file` keeps changes in a private COW
  file; "The COW file need not exist" (READ
  `user_mode_linux_howto_v2.rst:880-910`). That is an instant golden copy.
  ubd supports TRIM. hostfs mounts host paths with the UML process's rights;
  "Always confine hostfs" (READ `:970-975`), or build without CONFIG_HOSTFS.
- **Inside:** Docker in UML is shown by weber-software/diuid (README, last
  push 2022-01-07; READ via GitHub API). UML selects
  `HAVE_ARCH_SECCOMP_FILTER` (READ `arch/um/Kconfig:24`), so guest seccomp
  profiles work. systemd inside: INFERRED (UML is a full kernel with its own
  cgroup v2, independent of the host's v1).
- **Debian's package:** trixie has `user-mode-linux` 6.12um1+b21; it depends
  only on libc6 and suggests vde2 and slirp (READ
  https://packages.debian.org/trixie/user-mode-linux). The binary is
  `/usr/bin/linux.uml`, and its modules are under
  `/usr/lib/uml/modules/6.12.101/` (READ
  https://packages.debian.org/trixie/amd64/user-mode-linux/filelist). The
  modules must be copied into the guest image for Docker's overlay,
  bridge and netfilter needs (INFERRED). The packaging VCS has an
  unreleased 7.2um2 (READ https://tracker.debian.org/pkg/user-mode-linux).
  SMP needs a 6.19+ build with `CONFIG_SMP=y`; whether Debian enables it
  was not checked.
- The in-tree HOWTO still says "UML is strictly uniprocessor at present"
  (`:86-88`, and again at `:1042`); it predates 6.19.

## 8. Q6, rootless podman with a systemd guest, podman and Docker inside, FUSE

- **cgroup v1:**
  - Limits: "Resource limits are not supported and ignored on cgroups V1
    rootless systems" (READ `containers/podman@v5.8.3:pkg/specgen/generate/validate_linux.go:33-36`).
  - systemd as PID 1: "Since systemd will not run without being able to
    write to cgroups, it will never work in a rootless cgroup V1
    environment" (READ https://bugzilla.redhat.com/show_bug.cgi?id=1827315,
    Daniel Walsh, 2020-04-24). The reporter had it working with podman
    1.6.2, so this is policy plus systemd behaviour, not a kernel limit. In
    systemd mode on v1, podman mounts a tmpfs on `/sys/fs/cgroup/systemd`
    (READ https://docs.podman.io/en/latest/markdown/podman-run.1.html,
    `--systemd`). Not tested here with 4.9.3.
  - podman inside: works with `--cgroups=disabled` (measured inside Incus,
    engines-shortlist.md 8).
  - Docker inside: needs `--privileged` (all capabilities in the userns).
    INFERRED: runs without limits, like rootless Docker on v1.
  - FUSE: `--device /dev/fuse`, after `chmod 666 /dev/fuse` on this host.
  - Podman 6 removed v1 (runtime-survey.md); stay on 4.9 or 5.x here.
- **cgroup v2 with systemd delegation:** /sys/fs/cgroup is mounted writable
  in systemd mode (READ podman-run doc above), so systemd works; rootless
  podman in rootless podman works with `--device /dev/fuse` and a subuid
  range inside smaller than the outer one (READ
  https://www.redhat.com/en/blog/podman-inside-container). For rootless
  Docker-in-Docker, "Rootless cgroup v2 requires systemd, which is missing
  in the official dind" (READ https://github.com/moby/moby/issues/42910,
  AkihiroSuda, 2021-10-06).
- On this host, the earlier session measured Sysbox and Incus, not rootless
  podman with a systemd image; that combination is untested here.

## 9. Q7, QEMU TCG cost for compile-heavy work

The "10-50x slower" claim comes from Microsoft quicksand's docs, where it is
a comment in a code block, `# Software emulation (10-50x slower):`, with no
measurement behind it (READ
https://microsoft.github.io/quicksand/under-the-hood/08-performance.html).
The same wording appears in mikn/rules_qemu's README (search snippet only).

Real numbers found:

| source | workload | result |
| --- | --- | --- |
| Linaro, Dec 2024, QEMU 9.2 (READ https://www.linaro.org/blog/qemu-a-tale-of-performance-analysis/) | Android boot under TCG vs KVM with 4 CPUs | 8x slower for an x86-64 guest, 12x for aarch64; more than 4 guest CPUs did not help |
| Igalia 2007 (READ, section 7 link) | kernel build, 1 job, QEMU 0.8.2 (before TCG) | 2h13m vs 8m38s native: 15.4x |
| docker/buildx#2810, 2024-11 (READ GitHub API) | C++ compile, arm64 image under qemu-user on x86 | 8x (1 h vs 8 h) |
| multiarch/qemu-user-static#176 (search snippet) | cross-compile under qemu-user | 2-5x vs the native target board |
| qemu-devel, 2020-05 (READ https://lists.nongnu.org/archive/html/qemu-devel/2020-05/msg04898.html) | kernel build in an aarch64 guest on x86 under MTTCG | speedup over one vCPU plateaus near 5x |

No same-architecture, system-mode TCG compile benchmark from 2023 or later
was found. INFERRED for this host: x86-64 guest, MTTCG on 4 vCPUs, compile
at 6-12x of runc; process creation worse, because every guest page-table
change goes through the soft MMU. A 10-minute measurement would settle it
(section 13).

## 10. Q8, other routes from 2025-2026 not in the earlier files

| route | what it is | verdict here |
| --- | --- | --- |
| Lima on Linux (v2.2.1, 2026-10-03) | Rootless QEMU manager; without /dev/kvm it warns and uses TCG (READ `lima-vm/lima@c30ccbe435ad:pkg/driver/qemu/qemu.go:1258-1282`); `limactl clone` copies with copy-on-write when the filesystem supports it (READ `pkg/instance/clone.go:37,90`); `limactl snapshot create/apply/list/delete` exist (READ https://lima-vm.io/docs/reference/limactl_snapshot/) | The best manager for the TCG route (A5, B2) |
| libvirt session mode (`qemu:///session`) | A per-user daemon. "In unprivileged mode (qemu:///session), mapping user/group IDs is available (since libvirt version 10.0.0)" for virtiofs (READ https://libvirt.org/kbase/virtiofs/). passt as a backend, including vhost-user, is from search snippets of the 2025 libvirt patch series (the libvirt list's patch series of 2025-02-15 on passt, as indexed by patchew.org), not opened | Same engine as A5, heavier; fine on KVM hosts |
| gVisor rootless modes | Three methods: `runsc --rootless` (single uid, `create` not supported, no netstack, no save/restore, cgroup errors ignored), caller-made userns (podman/docker rootless, needs newuidmap), and a native rootless mode without network namespacing (READ https://gvisor.dev/docs/user_guide/rootless/). App compatibility: Docker-in-Docker 29.6.1 "Partial", Podman 4.9.3 "Partial", "Rootless mode is supported with caveats" (READ https://gvisor.dev/application-compatibility/) | A6 and B3; costs measured at 1.1-8.2x of runc on this host |
| systemd-vmspawn unprivileged | QEMU wrapper; unprivileged networking through nsresourced's TAP API (v258); `--kvm=no` for TCG | Needs systemd and its root daemons; no |
| Apptainer instances | `instance start --boot` is "(root only)" (READ https://apptainer.org/docs/user/main/cli/apptainer_instance_start.html) | No systemd as PID 1 without root |
| cwage/agentpen (2026-04) | bubblewrap plus pasta plus nftables inside the user namespace, rootless (README) | A process sandbox, not a system container; useful only as evidence that nft inside a pasta userns works |
| crun + libkrun (`krun` handler) | OCI runtime that boots each container in a libkrun VM (matrix [3]) | Needs KVM |
| Incus inside a rootless VM | Stock Incus as root *inside* a user-run UML or QEMU guest | Keeps all Incus features with no root daemon on the host; pays the VM's cost on everything. On a KVM host this is near-native and is the cleanest "rootless Incus" (INFERRED) |

## 11. Ranked list, definition A (no root at run time)

Ranking criterion: the Incus feature set first, speed second, then
maturity. "Limits without root" means enforced memory, CPU and disk limits
with no root step at all; a per-boot root delegation is noted where it
helps. "On this host" means no KVM, cgroup v1, no init.

| rank | candidate | docker in | podman in | FUSE | systemd PID 1 | cost vs runc | limits without root | fast clone | on this host | status |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| A1 | Incus 6.0 in a user namespace (rootlesskit or unshare, pasta) | yes, nesting; no limits | yes, `--cgroups=disabled` | yes | only with per-boot cgroup delegation | ~1.0x (INFERRED) | no; with delegation, memory/pids/cpu ceilings per user subtree; no disk quota | yes, `incus copy` on btrfs (INFERRED ~0.4 s) | yes | untested; nesting code paths exist |
| A2 | LXC 6.0 as the user, `type = empty` plus pasta hook | yes, nesting.conf; no limits | yes, `--cgroups=disabled` | yes, bind /dev/fuse | only with per-boot delegation | ~1.0x | as A1 | yes, `lxc-copy -s` (overlay or btrfs) | yes, LXC 6.0 LTS keeps v1 | documented mode; the pasta hook is not |
| A3 | rootless podman 4.9/5.x with crun and pasta | yes, `--privileged`; limits ignored | yes | yes, `--device /dev/fuse` | likely no on v1 | ~1.0x | no | image commit only | yes, 4.9.3 is installed | supported for apps, not for system containers on v1 |
| A4 | User-Mode Linux, ptrace mode, owned TAP | yes | yes | yes, the guest's own FUSE | yes | 1.5-3x CPU-bound, 5x+ process-heavy (INFERRED from section 7); 1 vCPU | yes, by construction: `mem=`, image size, 1 CPU | yes, ubd COW file, instant | yes | old, upstream, maintained; strongest boundary here |
| A5 | QEMU TCG (Lima or plain), passt or slirp | yes | yes | yes | yes | 8-12x (section 9) | yes, by construction | yes, qcow2 backing or Lima clone; RAM state via migration file | yes | mature, slow |
| A6 | gVisor under rootless podman (`runsc`) | partial, 6 concessions (measured as root) | no (measured) | no (measured) | not tested | 1.1-8.2x (measured) | no | no | yes | covered in runtime-survey.md |

Not ranked for A:
- systemd-nspawn unprivileged: two root daemons, plus systemd, BPF-LSM and
  cgroup v2 (section 6).
- Packaged Incus, Sysbox, incus-user: root daemons.
- Apptainer `--boot`: root only.
- Firecracker, Cloud Hypervisor, crosvm, libkrun, QEMU with KVM: no
  /dev/kvm. **If KVM appears** (PVM's `nested_pvm=1`, or another host),
  they move to the top: a user-run microVM with Docker, podman, FUSE and
  systemd inside at near-native speed, and limits by construction.
  Firecracker unjailed with an owned TAP fits A; Cloud Hypervisor 52+ or
  QEMU with passt fit B.

### A1. Incus 6.0 in a user namespace (untested recipe)

```sh
# --- root, once per boot (no init here: put it in 75-host-up.sh) ---
U=agent; UH=/home/$U
grep -q "^$U:" /etc/subuid || usermod --add-subuids 2000000-3999999 --add-subgids 2000000-3999999 "$U"
chmod 0666 /dev/fuse /dev/net/tun
# btrfs for golden copies; the user may delete subvolumes it owns
[ -f /var/lib/agent/bt.img ] || { mkdir -p /var/lib/agent; truncate -s 30G /var/lib/agent/bt.img; mkfs.btrfs -q /var/lib/agent/bt.img; }
mkdir -p /srv/agent; mountpoint -q /srv/agent || mount -o loop,user_subvol_rm_allowed,discard=async /var/lib/agent/bt.img /srv/agent
chown "$U": /srv/agent
# cgroup delegation, the pam_cgfs / Delegate=yes equivalent; ceilings stay on the root-owned parent
for c in systemd unified memory pids cpu cpuacct; do mkdir -p /sys/fs/cgroup/$c/agent/u; chown -R "$U": /sys/fs/cgroup/$c/agent/u; done
echo 12G  > /sys/fs/cgroup/memory/agent/memory.limit_in_bytes
echo 8192 > /sys/fs/cgroup/pids/agent/pids.max
# start the user's launcher inside the delegated cgroups (root must move it: cgroup2 checks the common ancestor)
sh -c 'for c in systemd unified memory pids cpu cpuacct; do echo $$ > /sys/fs/cgroup/$c/agent/u/cgroup.procs; done
       exec runuser -u '"$U"' -- setsid '"$UH"'/bin/incus-rootless.sh' >"$UH/incus-rootless.log" 2>&1 &
```

The launcher, `$UH/bin/incus-rootless.sh` (mode 0755, owned by the user):

```sh
#!/bin/sh
export INCUS_DIR="$HOME/.local/share/incus"; mkdir -p "$INCUS_DIR"
exec rootlesskit --net=pasta --copy-up=/etc --copy-up=/run --pidns --state-dir="$INCUS_DIR/rk" \
  sh -c 'mount --bind /dev/null /usr/bin/newuidmap && mount --bind /dev/null /usr/bin/newgidmap &&
         exec /usr/libexec/incus/incusd --logfile "$INCUS_DIR/incusd.log"'
```

```sh
# --- as the user, from outside the namespace; the socket is a file under INCUS_DIR ---
export INCUS_DIR="$HOME/.local/share/incus"
cat <<'EOF' | incus admin init --preseed
storage_pools:
- {name: bt, driver: btrfs, config: {source: /srv/agent/incus-pool}}
networks:
- {name: incusbr0, type: bridge, config: {ipv4.address: 10.77.0.1/24, ipv4.nat: "true", ipv6.address: none}}
profiles:
- name: default
  devices:
    root: {type: disk, path: /, pool: bt}
    eth0: {type: nic, network: incusbr0, name: eth0}
    fuse: {type: unix-char, path: /dev/fuse}
EOF
incus launch images:debian/13 golden -c security.nesting=true </dev/null
incus exec golden -- systemctl is-system-running --wait   # needs the delegated cgroups
incus stop golden
time incus copy golden s1 </dev/null && time incus start s1 </dev/null
incus exec s1 -- sh -c 'cat /proc/self/uid_map; ls -l /dev/fuse'
```

What to check: the incusd log line that prints the selected idmap; whether
`Split` found a range at namespace id 1,000,000 (it needs a map of about
1,065,536 ids; with 2,000,000 subuids it should); whether incusbr0 and the
NAT come up; docker and podman inside; the copy and start times.
Fallback: `incus profile set default security.privileged=true`.

Variant A1b, closer to what upstream tests: run a Debian 13 unprivileged LXC
container as the user (A2 below), with an idmap of at least 1,065,536 ids
and `nesting.conf`, then `apt-get install incus` (Debian 13 ships 6.0.4)
inside it and use it as a nested Incus host.

### A2. LXC 6.0 as the user

```sh
# --- root, once: packages; per boot: the subuid, /dev/fuse and cgroup steps of A1 ---
apt-get install lxc uidmap passt          # LXC 6.0.x; no lxc-user-nic setup, no lxc-usernet entry
# definition A allows no setuid helper but newuidmap/newgidmap: drop lxc-user-nic's setuid bit
find /usr/lib /usr/libexec -name lxc-user-nic -perm -4000 -exec chmod u-s {} +
```

```sh
# --- as the user ---
mkdir -p ~/.config/lxc ~/bin /srv/agent/lxc
read -r SU SN <<EOF
$(awk -F: -v u="$(id -un)" '$1==u{print $2, $3}' /etc/subuid)
EOF
read -r GU GN <<EOF
$(awk -F: -v u="$(id -un)" '$1==u{print $2, $3}' /etc/subgid)
EOF
echo "lxc.lxcpath = /srv/agent/lxc" > ~/.config/lxc/lxc.conf
cat > ~/.config/lxc/default.conf <<EOF
lxc.idmap = u 0 $SU 65536
lxc.idmap = g 0 $GU 65536
lxc.net.0.type = empty
lxc.hook.version = 1
lxc.hook.start-host = $HOME/bin/lxc-pasta
lxc.include = /usr/share/lxc/config/nesting.conf
lxc.mount.entry = /dev/fuse dev/fuse none bind,create=file,optional 0 0
EOF
printf '#!/bin/sh\nexec pasta --config-net --quiet "$LXC_PID"\n' > ~/bin/lxc-pasta; chmod +x ~/bin/lxc-pasta
lxc-create -n golden -B btrfs -t download -- -d debian -r trixie -a amd64
lxc-start -n golden && lxc-attach -n golden -- systemctl is-system-running --wait
lxc-attach -n golden -- sh -c 'apt-get update && apt-get install -y docker.io podman fuse3'
lxc-stop -n golden
time lxc-copy -n golden -N s1 -s && time lxc-start -n s1
```

### A3. Rootless podman with a systemd image

```sh
# as the user; podman 4.9.3 is already on this host
cat > Containerfile <<'EOF'
FROM docker.io/library/debian:trixie
RUN apt-get update && apt-get install -y systemd-sysv dbus docker.io podman fuse3 iptables && apt-get clean
CMD ["/sbin/init"]
EOF
podman build -t sysimg .
podman run -d --name s1 --systemd=always --privileged --device /dev/fuse --network pasta sysimg
podman exec s1 systemctl is-system-running --wait      # expected to fail on v1; this is the test
podman exec s1 podman --cgroups=disabled run --rm docker.io/library/debian:trixie true
```

### A4. User-Mode Linux, ptrace mode

```sh
# --- root, once per boot: an owned TAP and a relay to this host's CONNECT proxy ---
ip tuntap add dev uml0 mode tap user agent && ip addr add 10.78.0.1/24 dev uml0 && ip link set uml0 up
P=${HTTPS_PROXY##*:}; P=${P%/}
socat TCP-LISTEN:"$P",bind=10.78.0.1,fork TCP:127.0.0.1:"$P" &
```

```sh
# --- as the user ---
mkdir -p ~/uml && cd ~/uml
apt-get download user-mode-linux && dpkg-deb -x user-mode-linux_*.deb pkg   # trixie: 6.12um1, 1 vCPU
# root filesystem with real ownership, built inside a subuid user namespace
mmdebstrap --mode=unshare --variant=important \
  --include=systemd-sysv,dbus,openssh-server,docker.io,podman,fuse3,iptables trixie golden.tar
unshare --map-auto --map-root-user sh -c 'mkdir r && tar -C r -xpf golden.tar &&
  mkdir -p r/lib/modules && cp -a pkg/usr/lib/uml/modules/* r/lib/modules/ &&
  mke2fs -q -t ext4 -d r golden.img 30G'
chmod a-w golden.img                                    # any change to the backing file invalidates every COW file
# one instance = one COW file; it is created on first boot
UML=./pkg/usr/bin/linux.uml                             # depends only on libc6
"$UML" mem=4G ubd0=s1.cow,golden.img root=/dev/ubda rw hostfs=/nonexistent \
  vec0:transport=tap,ifname=uml0 con=null con0=fd:0,fd:1 init=/lib/systemd/systemd
# inside: ip addr add 10.78.0.2/24 dev vec0; ip route add default via 10.78.0.1;
#         export HTTPS_PROXY=http://10.78.0.1:$P
```

Measure: `hyperfine` of the bench workloads (spawn, files, git, cpu,
compile) from engines-shortlist.md 5.2 inside the guest, against runc.

### A5. QEMU TCG

```sh
# --- root, once: packages (or use B2's $HOME install) ---
apt-get install qemu-system-x86 qemu-utils cloud-image-utils passt
# --- as the user ---
curl -LO https://cloud.debian.org/images/cloud/trixie/latest/debian-13-genericcloud-amd64.qcow2
printf '#cloud-config\npassword: agent\nchpasswd: {expire: false}\npackages: [docker.io, podman, fuse3]\n' > user-data
cloud-localds seed.img user-data
qemu-img create -f qcow2 -F qcow2 -b debian-13-genericcloud-amd64.qcow2 golden.qcow2 30G
qemu-system-x86_64 -machine q35 -accel tcg -cpu max -smp 4 -m 6G -nographic \
  -drive file=golden.qcow2,if=virtio,discard=unmap -drive file=seed.img,if=virtio,format=raw \
  -nic passt,model=virtio            # QEMU 10.1+; older: -nic user,model=virtio-net-pci
# after provisioning and poweroff: a new instance is one overlay
qemu-img create -f qcow2 -F qcow2 -b golden.qcow2 s1.qcow2
# or with Lima: limactl create --name=golden --vm-type=qemu --cpus=4 --memory=6 template:debian-13
#               limactl start golden; limactl stop golden; limactl clone golden s1
```

### A6. gVisor under rootless podman

```sh
# runsc release binary in ~/bin; see runtime-survey.md for the measured costs
podman --runtime ~/bin/runsc --runtime-flag platform=systrap run --rm docker.io/library/debian:trixie true
```

## 12. Ranked list, definition B (no root ever)

Without newuidmap there is one uid in a user namespace. Every container
route then breaks a Debian userland that chowns to other ids (apt's `_apt`,
systemd units with `User=`, image layers with other owners). A guest kernel
with its own disk image does not care: inside the image, ownership is the
guest's business. So the kernel-in-userspace routes rise to the top.

| rank | candidate | docker in | podman in | FUSE | systemd PID 1 | cost vs runc | limits without root | fast clone | on this host | status |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| B1 | UML, ptrace mode, from $HOME | yes | yes | yes, guest's own | yes | as A4; 1 vCPU | yes, by construction | yes, ubd COW | yes; needs no user namespace at all | as A4 |
| B2 | QEMU TCG (plain or Lima) from $HOME, passt or slirp | yes | yes | yes | yes | 8-12x | yes | yes | yes | as A5 |
| B3 | gVisor `runsc --rootless do` | no or partial (no netstack) | no | no (measured) | not tested | 1.1-8.2x (measured as root) | no | no | yes | a process sandbox over the host root, not a system container |
| B4 | Containers with a single-uid map (`unshare -r`, rootlesskit without subuid, Incus "privileged only") | breaks on chown | breaks | yes with 0666 /dev/fuse (a root step) | no | ~1.0x | no | partial | yes | not the Incus feature set |
| B5 | PRoot or udocker | no | no | no | no | ptrace path translation, slow | no | no | yes | not examined in depth |

Where unprivileged user namespaces are disabled (for example Ubuntu's
AppArmor restriction), B3 and B4 stop working; B1 and B2 do not need user
namespaces (INFERRED).

### B1. UML from $HOME

```sh
mkdir -p ~/uml && cd ~/uml
apt-get download user-mode-linux && dpkg-deb -x user-mode-linux_*.deb pkg   # apt-get download needs no root
# root filesystem without subuids: fakeroot keeps ownership until mke2fs writes it into the image
mmdebstrap --mode=fakeroot --variant=important \
  --include=systemd-sysv,dbus,openssh-server,docker.io,podman,fuse3,iptables trixie golden.tar
fakeroot sh -c 'mkdir r && tar -C r -xpf golden.tar &&
  mkdir -p r/lib/modules && cp -a pkg/usr/lib/uml/modules/* r/lib/modules/ &&
  mke2fs -q -t ext4 -d r golden.img 30G'
UML=./pkg/usr/bin/linux.uml
# network option 1: VDE with its slirp plugin (needs a UML built with VDE support and libvdeplug in $HOME)
"$UML" mem=4G ubd0=s1.cow,golden.img root=/dev/ubda rw hostfs=/nonexistent \
  vec0:transport=vde,vnl=slirp:// con=null con0=fd:0,fd:1 init=/lib/systemd/systemd
# network option 2: a TAP inside a pasta-made namespace, NAT by nft there (needs unprivileged userns)
pasta --config-net -- sh -c '
  ip tuntap add uml0 mode tap && ip addr add 10.78.0.1/24 dev uml0 && ip link set uml0 up &&
  sysctl -qw net.ipv4.ip_forward=1 &&
  nft add table ip nat && nft add chain ip nat post "{ type nat hook postrouting priority 100; }" &&
  nft add rule ip nat post oifname != uml0 masquerade &&
  exec '"$UML"' mem=4G ubd0=s1.cow,golden.img root=/dev/ubda rw hostfs=/nonexistent vec0:transport=tap,ifname=uml0 con=null con0=fd:0,fd:1'
```

`mmdebstrap --mode=fakeroot` and the fakeroot ownership carry-over into
`mke2fs -d` are INFERRED; if they fail, build the image once on a host with
subuids (A4) and copy it.

### B2. QEMU TCG from $HOME

```sh
mkdir -p ~/qemu/debs && cd ~/qemu/debs
apt-get download qemu-system-x86 qemu-system-common qemu-system-data qemu-utils seabios ipxe-qemu libslirp0 passt
# add any library that `ldd` reports missing after extraction, the same way
for d in *.deb; do dpkg-deb -x "$d" ~/qemu/root; done
export PATH=~/qemu/root/usr/bin:$PATH LD_LIBRARY_PATH=~/qemu/root/usr/lib/x86_64-linux-gnu
qemu-system-x86_64 -L ~/qemu/root/usr/share/qemu -L ~/qemu/root/usr/share/seabios ...   # then as in A5
```

## 13. What to measure next, in order

1. **A1** on this host: one hour. The incusd idmap log line, `incus launch`,
   systemd inside (with and without the per-boot delegation), Docker and
   podman inside, `incus copy` time. If it works, it replaces root incusd
   with a root boot script and nothing else.
2. **A2** as the fallback with the same probes, and the pasta hook.
3. **TCG and UML costs** with the experiment-81 workloads, interleaved with
   runc: one Debian guest each, 4 vCPUs for TCG and 1 for UML. This turns
   section 7's and section 9's estimates into numbers.
4. Whether `incusd` in a namespace needs anything this recipe lacks
   (`/run` copy-up, dnsmasq's user, nftables built into this kernel).

## 14. Where this disagrees with the earlier files

- **runtime-survey.md, "QEMU without KVM ... its own documentation says
  10-50x slower".** The figure is an unsourced code comment in quicksand's
  docs (section 9). Primary data says 8x (x86-64 Android boot) to 12x
  (aarch64), and 15x for a 2007 pre-TCG QEMU on a kernel build. TCG is still
  far slower than runc, but it should not be ruled out for definition B,
  where it is one of two routes that keep the whole feature set.
- **runtime-survey.md read UML "docs only" and did not rank it.** UML is the
  only no-KVM kernel boundary that runs as a plain user with no setuid
  helper and no user namespace. Its weak points are specific: one vCPU in
  the secure mode, 5x on fork+exec, 200x on raw syscalls.
- **incus-gaps-survey.md, gap (a), "Unprivileged LXC 6.0.x ... needs
  pam_cgfs or cgroups pre-created by root."** LXC 6.0 starts unprivileged
  containers with no writable cgroups; it skips those hierarchies (section
  4). What needs pam_cgfs is limits and a systemd guest.
- **incus-gaps-survey.md, gap (a), "treat incusd as a root component."**
  True for the packaged daemon, but incusd's supported nesting mode is
  "root inside a user namespace", and its id-range fallback reads
  `/proc/self/uid_map`. A user-made namespace is a plausible host for it
  (section 4). Untested.
- **engines-shortlist.md section 13, "Against it: a root daemon (incusd)".**
  Same point. A1 would remove the root daemon and keep the root boot
  script that this host needs anyway.
- **The proposed matrix:** crosvm's slirp is Windows-only; Firecracker has no
  socket or vhost-user net path; Cloud Hypervisor's `fd=` is for TAP fds,
  not passt; libkrun's virtio-fs does not preserve ownership unprivileged;
  QEMU does not need KVM (section 2).
- **docs/engines.md** says the Firecracker jailer needs root; agreed (matrix
  [25]). Note that the unjailed VMM still runs its own seccomp filters, so
  "rootless Firecracker" is not "unsandboxed Firecracker", only unjailed.
- **Cloud Hypervisor's own docs** grant virtiofsd `cap_sys_admin`; virtiofsd
  documents a fully unprivileged mode (section 3).

## 15. What could not be reached or was not done

- No experiment ran: no install, build, container or VM start, per the task.
  Every recipe in sections 11 and 12 is untested.
- No published attempt at incusd under rootlesskit or `unshare` was found.
- No modern same-architecture TCG compile benchmark was found.
- Whether Debian builds `linux.uml` with VDE support and with
  `CONFIG_SMP` was not checked (the package only suggests vde2).
- passt over vhost-user into UML or crosvm was not found documented.
- wiki.qemu.org (ChangeLog/7.2, ChangeLog/10.1, Planning/10.1) answered the
  reader with a bot wall, so those facts rest on search snippets and on the
  commit date (matrix [28], [29]).
- sources.debian.org returned no content; the Debian UML facts come from
  packages.debian.org and tracker.debian.org instead.
