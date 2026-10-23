# Engine choice: the reproduction and what it changed

This page records why errand-box runs each errand session in an Incus system
container, as measured on 2026-10-23. It starts from the research on the
`ephemeral-errand-sandbox-research` branch of agent-box
(`research/errand-sandbox/`, called "the research" below), and checks its
claims on a second host before anything was built on them.
[rootless.md](rootless.md) covers the engines that need no root.
[review.md](review.md) covers the corrections to the research's citations.

## The host

Linux 6.18.44 (a Firecracker guest kernel), x86_64, 4 vCPUs, 16 GB, ext4
root. No /dev/kvm. cgroup v1, no delegation. PID 1 is the platform's own
process, not systemd. This is another instance of the same platform the
research used. The host restarted once during the build (section 5).

Versions: Docker 29.9.1, podman 4.9.3, gVisor runsc release-20261019.0 (its
sha512 matched the research's pin), Incus 6.0.0 from Ubuntu 24.04 for the
reproduction, and Incus 6.0.6 from the Zabbly `lts-6.0` repository after it.

## 1. The four claims the research asked a second host to check

| claim | result here |
| --- | --- |
| Incus costs 0.9 to 1.1x of runc on spawn, files, git, cpu and compile | holds: 0.9 to 1.1x on all ten rows |
| gVisor costs at least 3x on spawn and compile | holds: spawn 7.3x and 8.3x, compile 7.1x and 7.2x |
| Incus runs Docker, podman (with cgroups disabled on v1), FUSE and an rclone network mount, with systemd as PID 1 | holds |
| An existing, stopped instance starts in under 1 s | holds: start plus `exec true`, median 285 ms of 5 (245 to 307) |

The cost table is experiment 81 of the research, run unchanged except that
Sysbox was left out: 5 interleaved rounds, median milliseconds, factor
against runc. `/workspace` is a host directory bind-mounted in; `/scratch`
is a tmpfs.

| workload@dir | runc | runsc (gVisor) | incus 6.0.0 |
| --- | --- | --- | --- |
| spawn@workspace | 434 | 3157 (7.3x) | 407 (0.9x) |
| files@workspace | 1014 | 2025 (2.0x) | 1042 (1.0x) |
| git@workspace | 978 | 2561 (2.6x) | 956 (1.0x) |
| cpu@workspace | 327 | 377 (1.2x) | 318 (1.0x) |
| compile@workspace | 1373 | 9746 (7.1x) | 1400 (1.0x) |
| spawn@scratch | 410 | 3385 (8.3x) | 414 (1.0x) |
| files@scratch | 101 | 349 (3.5x) | 102 (1.0x) |
| git@scratch | 109 | 986 (9.0x) | 121 (1.1x) |
| cpu@scratch | 302 | 363 (1.2x) | 331 (1.1x) |
| compile@scratch | 1186 | 8576 (7.2x) | 1238 (1.0x) |

Absolute times are lower than on the research host (files@workspace 1014
against 1518 ms). The ratios agree. Raw rounds:
`research/results/81-repro-raw.txt`.

The feature checks are `research/experiments/x91-incus-features.sh`; its
output is `research/results/91-incus-features.txt`. Its quota and limits
section failed because of a defect in the script, so those were measured by
hand on 6.0.6 (section 3).

## 2. The rootfs rewrite: a defect of Incus 6.0.0, now explained

The research left one question open: why incusd rewrites the ownership of
every new instance's root filesystem ("Remapping container filesystem") when
the kernel supports idmapped mounts. With `security.idmap.isolated=true`,
which gives each guest its own uid range, that rewrite makes every new
instance cost 2 to 3 s.

The cause, found with `strace` of incusd during `incus start`:

1. Incus probes for idmapped mounts with `open_tree`, a new user namespace,
   and `mount_setattr(MOUNT_ATTR_IDMAP)`. On this host the probe returned
   `EINVAL`, so Incus fell back to the rewrite.
2. The probe's child process stopped with `si_uid=65534`: its user namespace
   had no uid map.
3. In Incus 6.0.0, `get_userns_fd` in `shared/idmap/shift_linux.go` creates
   the user namespace and returns it without writing `uid_map`, `setgroups`
   or `gid_map`. The kernel refuses an idmapped mount through a namespace
   with no map. Incus 6.0.6 writes `0 0 1` into both maps; the change is
   upstream commit ec223f75e705 of 2024-05-23, "shared/idmap: Make
   get_userns_fd configure the userns".
4. The same sequence from a root shell, with the maps written, succeeds on
   the btrfs pool, on ext4 and on tmpfs (`research/experiments/idmap-probe.c`).

Ubuntu 24.04 ships 6.0.0. Measured on the same host and pool:

| new instance, start, `exec true` | 6.0.0 (Ubuntu) | 6.0.6 (Zabbly) |
| --- | --- | --- |
| golden copy, shared uid map | 415 ms median (412 to 1128) | not measured |
| golden copy, own uid range (`idmap.isolated`) | 2114, 2386, 2654 ms (rewrite each time) | 440, 479, 544 ms (no rewrite) |
| first start of a fresh instance from an image | rewrite | 244 ms; the root mount shows `idmapped` |

So the research's "about 0.4 s per new instance" holds with per-guest uid
ranges too, but only with Incus 6.0.1 or later. errand-box installs Incus
from the Zabbly repository where it can, and reports 6.0.0 as degraded
(edge case E-20).

The research's second open question, why `shift=true` disk devices were
refused, has the same cause: on 6.0.6 they work. A write by guest root on
such a mount lands as host root, so errand-box does not use them for
errand's directories. It maps one uid instead (section 4).

## 3. Limits, measured on 6.0.6

- `root.size=3GiB` on btrfs, `limits.memory=256MiB`, `limits.cpu=1`: `nproc`
  reads 1, a Python allocation loop exits 137, and with lxcfs the guest's
  MemTotal is 262144 kB. `df /` in the guest shows the pool, not the quota.
- A quota below what the instance already uses leaves it unable to stop or
  start: "Failed clearing ownership: chown ...: disk quota exceeded". This is
  why `box.disk` has a floor (edge case E-21).
- lxcfs: the Zabbly LXC hook reads `/var/lib/incus-lxcfs`, the Ubuntu one
  `/var/lib/lxcfs` (E-07). lxcfs 6.0.6 refuses every /proc file on this host,
  because root cannot read `/proc/1/personality` here (lxcfs
  `src/utils.c:744-758`). Each guest start then fails in the mount hook with
  exit 32. Ubuntu's lxcfs 5.0.4 at the Zabbly path works (E-08).

## 4. The shape of a session, checked before any code was written

Each step below was run by hand, as the errand account, through incus-user:

- incus-user gives the account a restricted project. Disk devices may name
  paths under the account's home only, `raw.idmap` may name only its own uid
  and gid, and nesting is allowed. Proxy devices are blocked until an
  administrator allows them (`restricted.devices.proxy=allow`).
- An instance with no network device, nesting, `idmap.isolated`, and
  `raw.idmap` mapping guest uid 1000 to the errand account. Inside:
  uid_map `0 1262144 1000`, `1000 994 1`, `1001 1263145 64535`; only `lo`;
  the root mount is idmapped.
- A file the agent (uid 1000) writes in `/workspace` belongs to the errand
  account on the host.
- A proxy device that listens on `0.0.0.0:P` inside and dials host
  `127.0.0.1:P` reaches a host server through `169.254.169.1:P` once that
  address is on `lo` (HTTP 200). A direct TCP connection to the internet
  fails with errno 101.
- `incus exec -T` passes stdin and stdout through and returns the exit code.
  SIGTERM to the client reaches the process inside. SIGKILL of the client
  kills the process inside but leaves the instance running (E-27).

## 5. A restart during the build

The host restarted at about 07:40Z. As the research recorded: daemons and
device modes were lost, disk state survived. The egress proxy moved again
(port 45823 to 44687; E-13). New: incusd, once started again, restarted the
instances that had been running, with no launcher attached. errand-box sets
`boot.autostart=false` for sessions (E-28).

## 6. The choice

Incus, run by root, with errand reaching it only through incus-user. The
research's section 13 reasons hold, and two of its costs are now gone:
- per-guest uid ranges cost nothing with 6.0.1 and later (section 2);
- the "root daemon" weak point is narrowed: errand's account cannot reach
  incusd's full API, only its restricted project.

What remains against it: incusd and lxcfs run as root, the guests share the
host kernel, and on a cgroup v1 host it ends at the 6.0 LTS line (supported
to June 2029). [rootless.md](rootless.md) shows what each rootless route
gives up.
