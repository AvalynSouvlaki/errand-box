# Incus backend for errand: specification

**Status:** proposal to errand's author; not implemented in errand. errand-box
implements the same behaviour today, behind errand's `bailey` backend.
**Target:** errand v0.3.x, `src/sandbox/`. **Tracked as:** T-019 in
[TODO/sandbox.md](../TODO/sandbox.md).

This page specifies a third errand sandbox backend, `incus`, that runs each
session in its own Incus system container. Every behaviour below runs today
in errand-box and passed its live tests on one host; the measurements are in
appendix A. The E numbers refer to [edge-cases.md](edge-cases.md), where
each condition has its measurement and its test.

## 1. Scope

The backend gives a session a whole Linux system: systemd as PID 1, Docker,
podman and FUSE inside, per-session memory, CPU and process limits, and a
disk quota on the guest's own disk. It keeps the session's interface to
errand as the other backends do: the project at `/workspace`, the state at
`/state`, ACP on the launcher's stdin and stdout, and errand's broker as the
only way out.

Out of scope:

- installing and configuring Incus on the host (section 8 lists what the
  backend expects to find);
- virtual machines;
- an incusd that runs without root (section 10, D3).

## 2. Architecture

errand talks to incusd through the incus-user socket, as the unprivileged
errand account. incusd runs one instance per session, copied from a golden
instance that is never started. The instance has no network device; two
proxy devices carry its traffic to errand's broker on host loopback.

| part | runs as | role |
| --- | --- | --- |
| errand, `incus` backend | the errand account, in group `incus` | probe, launch, stop and sweep, through the API |
| incus-user | root | gives the errand account one restricted project, `user-<uid>` |
| incusd | root | runs the instances |
| golden instance | (stopped) | the source of every session instance; made once per image |
| session instance | an unprivileged uid range of its own | systemd, the launcher, kage, and what the agent runs |
| errand's broker | the errand account | the only egress of a session |

## 3. Configuration

The backend reads errand's existing sandbox settings (section 7) and these
new keys. Names are proposals.

| key | default | meaning |
| --- | --- | --- |
| `sandbox.backend` | | `"incus"` selects this backend |
| `sandbox.image` | `errand/debian-13` | the image alias the golden instance is made from |
| `sandbox.incus.golden` | `errand-golden` | the golden instance |
| `sandbox.incus.guestUid` | `1000` | the agent's uid and gid in the guest |
| `sandbox.incus.rootSize` | `20GiB` | the quota of each instance's root disk; at least 4 GiB (E-21) |
| `sandbox.incus.keepDays` | `14` | a stopped instance unused this long is deleted |

The project is the one incus-user assigns, `user-<uid of the errand account>`.
The client configuration (`INCUS_CONF`) is a directory errand owns outside
every session's state directory, because a session can write its state
directory.

## 4. The instance

### 4.1 Name and keys

The instance name is `errand-<session id>` when that is a valid Incus name
(at most 63 characters; letters, digits and hyphens; not starting with a
digit or a hyphen). Otherwise it is `errand-` followed by the first 40
characters of the id with every other character replaced by a hyphen, a
hyphen, and the first 10 hex digits of the id's SHA-256.

| config key | value | written |
| --- | --- | --- |
| `user.errand.system` | `true` | at the copy |
| `user.errand.session` | the session id | at the copy |
| `user.errand.image` | the fingerprint of the image the golden instance came from | at the copy (inherited from the golden instance) |
| `user.errand.last-used` | Unix seconds | at each launch |

### 4.2 Settings

Each instance carries:

| key | value | reason |
| --- | --- | --- |
| `security.nesting` | `true` | Docker and podman inside |
| `security.idmap.isolated` | `true` | a uid range per instance; with Incus 6.0.1 or later the root disk is an idmapped mount, so a copy needs no rewrite (E-19, E-20) |
| `raw.idmap` | `uid <errand uid> <guestUid>` and `gid <errand gid> <guestUid>` | files the agent writes in `/workspace` belong to the errand account |
| `boot.autostart` | `false` | incusd restarts running instances after a host restart; a session must run only under errand (E-28) |
| `limits.memory`, `limits.cpu`, `limits.processes` | from section 7 | per-session limits |
| root disk `size` | `sandbox.incus.rootSize` | the guest's own disk |

The instance has no `nic` device. The project's default profile has its
`eth0` removed.

### 4.3 Devices

| device | type | settings | when |
| --- | --- | --- | --- |
| `errand-workspace` | `disk` | `source` = project path, `path` = `/workspace` | always |
| `errand-state` | `disk` | `source` = state directory, `path` = `/state` | always |
| `errand-extra-<n>` | `disk` | from `sandbox.policyExtra`; `readonly=true` for read grants | when set |
| `errand-broker` | `proxy` | `listen=tcp:0.0.0.0:<P>`, `connect=tcp:127.0.0.1:<P>`, `bind=instance` | network not `none` |
| `errand-sysproxy` | `proxy` | `listen=tcp:0.0.0.0:3128`, `connect=tcp:127.0.0.1:<P>`, `bind=instance` | network not `none` |

`<P>` is the broker's port for the session. The agent reaches the broker at
`169.254.169.1:<P>`; apt and the guest's Docker daemon reach it at
`169.254.169.1:3128`. The launch reconciles the devices every time: it
removes an `errand-` device that is not wanted and adds one that is missing,
because the port and the paths can change between launches.

## 5. The guest image

The image is a contract between the backend and whoever builds it.
errand-box's [image/provision.sh](../image/provision.sh) builds one from
`images:debian/13`. The image must provide:

1. systemd as init;
2. an account `agent` with uid and gid `guestUid`, in group `docker`, with
   passwordless sudo;
3. subordinate ids `1001` to `65535` for that account, and `slirp4netns` as
   podman's rootless network, because the guest has no default route for
   pasta (E-30);
4. a boot unit, before Docker and podman, that adds `169.254.169.1/32` to
   `lo`, and on a cgroup v1 host sets podman's `cgroups = "disabled"` (E-04);
5. apt and the Docker daemon set to the proxy `http://169.254.169.1:3128`;
6. kage at `/usr/local/bin/kage`;
7. a tmpfiles entry for `/run/errand`, mode 0700, owned by the agent;
8. the launcher of section 6.3, at `/usr/local/libexec/errand/launch`.

## 6. Lifecycle

The backend implements errand's `Backend` operations. The handle is a new
`SandboxHandle::Incus` variant with the fields of `PodmanStop`.

### 6.1 `probe`

1. `GET /1.0` on the socket. No answer: `SandboxUnavailableError`.
2. The project: restricted, and `restricted.devices.proxy=allow` when the
   network is not `none`. Missing: `SandboxUnavailableError` that names the
   key.
3. The golden instance: present and stopped. Missing:
   `SandboxUnavailableError`.
4. The server version: below 6.0.1 is a note, "each new instance rewrites
   its root file system" (2.1 to 2.7 s, E-20).
5. The root pool's driver: `dir` is a gap, "the guest's disk quota is not
   enforced" (E-05).
6. Notes for the report: per-session memory, CPU and process limits; the
   disk quota; no network device; the broker as the only egress.

### 6.2 `launch`

1. Take an exclusive lock on the session's instance.
2. If the instance exists, is stopped, and its `user.errand.image` differs
   from the golden instance's, delete it. The rootfs holds nothing errand
   needs; the workspace and the state are on the host (E-31).
3. If the instance does not exist, copy the golden instance to it with the
   keys of section 4.1, as an instance-only copy.
4. Set the limits of section 4.2 and reconcile the devices of section 4.3.
   Set `user.errand.last-used`.
5. Write the agent's configuration into the state directory, as the podman
   backend does.
6. Start the instance.
7. Write the session environment to `/run/errand/env.sh` in the guest
   through the file API (`POST /1.0/instances/<name>/files`), owner
   `guestUid`, mode 0600. One shell-quoted `export` per variable, then
   `ERRAND_FSIZE=<bytes>` for `sandbox.fileMax`. No value goes on a host
   command line.
8. Spawn `incus exec <name> -T --cwd /workspace --
   /usr/local/libexec/errand/launch <guestUid> <guestUid> <agent command>`
   as the agent process, with stdin and stdout piped for ACP, as the podman
   backend spawns `podman run`. Do not pass `--user`: it gives the process
   no supplementary groups (E-29).
9. Return the handle.

### 6.3 The launcher

The launcher starts as the guest's root and:

1. refuses to run without `<uid> <gid> <command...>`, or without
   `/run/errand/env.sh`;
2. drops to `<uid>:<gid>` with the account's supplementary groups
   (`setpriv --reuid --regid --init-groups`);
3. starts a shell with an empty environment, sources the file, deletes it,
   and removes `ERRAND_FSIZE` from the environment;
4. waits up to 5 s for `169.254.169.1` on `lo`;
5. execs the command, under `prlimit --fsize=<ERRAND_FSIZE>` when it is set.

It writes only to stderr: stdout carries ACP.

### 6.4 `stop`

As `PodmanStop::stop`, with the instance in place of the container:

1. Return at once if the handle already stopped.
2. `PUT /1.0/instances/<name>/state` with `action=stop` and `timeout` =
   `gracePeriodMs` in seconds, at least 1.
3. If that fails, the same with `force=true`, and log a warning.
4. SIGKILL the `incus exec` process.
5. Return whether the forced stop was needed.

The instance stays, stopped, for the session's next launch: a stopped
instance starts in about 0.3 s, with what the agent installed still there.

### 6.5 `list_orphans` and `remove_orphans`

errand calls these at start, before it accepts a message.

- `list_orphans`: the instances in the project with
  `user.errand.system=true` that are running. No session is live at start,
  so each one is left from a run that did not stop it: a crash, or a
  SIGKILL of errand (E-27).
- `remove_orphans`: a forced stop of each. Then delete each stopped
  instance whose session errand no longer has, or whose
  `user.errand.last-used` is older than `sandbox.incus.keepDays`.

### 6.6 Calls to incusd

- Use the REST API on the socket for everything except the session's exec.
  It needs no stdin, and it puts no value on a command line.
- Where the `incus` client runs (the exec), close stdin on every call that
  is not the exec. The client reads a stdin that is not a terminal and
  blocks until it closes (E-11).
- Never signal a process named `incusd`. Every instance's monitor process
  has that name too (E-12).

## 7. errand's sandbox settings

| errand setting | the backend |
| --- | --- |
| `sandbox.memory` | `limits.memory` |
| `sandbox.cpus` | `limits.cpu` for whole CPUs; `limits.cpu.allowance` (`<n>ms/100ms`) for a fraction, not tested |
| `sandbox.pids` | `limits.processes` |
| `sandbox.fileMax` | `ERRAND_FSIZE`, applied by the launcher as `RLIMIT_FSIZE` |
| `sandbox.tmpSize`, `shmSize` | a tmpfs at `/tmp` of that size, if errand keeps the bound; the guest has its own `/dev/shm`. Not tested |
| `sandbox.diskTmp` | a disk device from the state directory at `/tmp`, as in the podman backend; the image must then mask `tmp.mount`. Not tested |
| `sandbox.disk` | measured by errand, as now: it covers the project and the state, which are host directories |
| `sandbox.network = "none"` | no proxy devices: no way out |
| `sandbox.network`, `egress` | the proxy devices of section 4.3 |
| `sandbox.egressPorts`, `hideHostAddress` | no effect: there is no network device |
| `sandbox.gracePeriodMs` | the stop timeout of section 6.4 |
| `sandbox.env`, `pathExtra` | the environment file |
| `sandbox.policyExtra` | `errand-extra-<n>` disk devices |
| `sandbox.requireFullEnforcement` | as now, against the gaps of section 6.1 |

## 8. Host prerequisites

The backend expects these on the host. Each needs root, so they stay outside
errand. errand-box's `errand-box setup` and `errand-box up` do all of them
([operations.md](operations.md)).

| prerequisite | why |
| --- | --- |
| Incus 6.0.1 or later; on a cgroup v1 host the 6.0 LTS line | 6.0.0 rewrites each new root file system (E-20); Incus 7.0 refuses cgroup v1 (E-04) |
| a btrfs (or ZFS, or LVM) storage pool | a `dir` pool does not enforce `size` (E-05) |
| lxcfs that works on the host | the guest sees its limits in `/proc` (E-06, E-08) |
| incus-user running; the errand account in group `incus`, not `incus-admin` | the restricted project |
| `restricted.devices.proxy=allow` on that project | the broker devices |
| the project's default profile: no `eth0`, the root disk on the pool | no network device in any instance |
| root allowed to map errand's uid and gid (`/etc/subuid`, `/etc/subgid`) | `raw.idmap` |
| the guest image (section 5) and the golden instance | the copy in section 6.2 |
| on a host with no init: a supervisor for incusd, incus-user, lxcfs and errand, and a step at each boot that restores device modes and kernel settings | nothing else starts them (E-01, E-02, E-03) |
| periodic `fstrim` of a pool on a loop file | freed space returns to the host disk (E-25) |

## 9. Security properties

- A guest's root is an unprivileged host uid from a range of its own. Only
  `guestUid` maps to a real host account, the errand account.
- The restricted project allows no privileged instance and no raw LXC
  settings, disk devices only under the errand account's home, and
  `raw.idmap` only for the account's own uid and gid. incusd enforces it,
  not errand.
- With no network device, a direct connection fails (errno 101); the
  broker applies errand's allowlist to everything else.
- No session value is written to a host command line.
- The guests share the host's kernel. A kernel exploit in a guest is a
  host compromise. This is the same boundary as the podman backend's, with
  a larger kernel surface inside the guest (systemd, Docker).

## 10. Decisions for errand's author

| | decision | recommendation |
| --- | --- | --- |
| D1 | Who performs the host prerequisites | an installer outside errand, such as `errand-box setup`, so that errand never needs root |
| D2 | One instance per session, kept between launches, or one per launch | per session: a revived session starts in about 0.3 s and keeps what the agent installed |
| D3 | An incusd without root | later. It runs at the same speed but loses the disk quota (E-24), and its proxy devices cannot reach host loopback, so the broker needs a unix-socket relay (E-23) |
| D4 | The client for the exec | the `incus` client as a child process, so that errand owns its pipes as it does `podman run`; the REST API for the rest |

## 11. Acceptance

The backend is done when these pass on a live host, through errand. Each is
a check of errand-box's [tests/live/live.sh](../tests/live/live.sh), which
can be ported.

- the agent runs as `guestUid`, with the `docker` group;
- a file the agent writes in `/workspace` belongs to the errand account;
- no network device other than the guest's own bridges, no default route,
  the broker reachable, a direct connection refused;
- the file size limit applies; the environment file is gone; PID 1 is
  systemd;
- Docker run and build, `sudo podman`, rootless podman and a FUSE mount by
  the agent;
- the guest sees its CPU and memory limits; a write past the root quota
  fails;
- `stop` ends the session within the grace period; a SIGKILL of errand
  leaves an instance that the next start's sweep stops;
- after a host restart, no session instance runs until errand launches it.

## Appendix A: measurements

One host: Ubuntu 24.04 in a hosted VM, kernel 6.18, x86_64, 4 vCPUs, 16 GB,
cgroup v1, no init, no KVM; Incus 6.0.6, errand v0.3.1, kage v0.5.0.
Sources: [research/engines.md](research/engines.md) and
[tests/live/2026-10-23.txt](../tests/live/2026-10-23.txt).

| measure | result |
| --- | --- |
| work in a guest against runc (spawn, files, git, cpu, compile) | 0.9 to 1.1x; gVisor 1.2 to 9.0x |
| start a stopped instance and run `true` | 285 ms median (245 to 307) |
| copy the golden instance, start, run `true` | 440, 479, 544 ms; 2,114 to 2,654 ms on Incus 6.0.0 |
| a short session, start to clean stop | 4.3 s |
| kage answers `initialize` and `session/new` | 3.3 s after launch |
| stop after SIGTERM to the launcher | 1.0 s |
| stop after SIGKILL of the launcher, by a watchdog | 1.1 s |

## Appendix B: errand references (v0.3.1)

- `src/sandbox/mod.rs`: `Backend`, `SandboxHandle`, `PodmanStop::stop`
- `src/sandbox/backend.rs`: `SandboxLaunch`, `CapabilityReport`,
  `SYSTEM_LABEL`, `SESSION_LABEL`, `sandbox_name`
- `src/sandbox/podman.rs`: `launch`, `list_orphans`, `remove_orphans`
- `src/session/manager.rs`: `sweep_orphans`
- `src/config/schema.rs` lines 397 to 452: `SandboxConfig`
