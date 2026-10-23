# Design

errand-box puts each errand session in its own Incus system container. This
page says how the parts fit. [research/engines.md](research/engines.md) says
why Incus; [bailey-contract.md](bailey-contract.md) says what errand
expects; [edge-cases.md](edge-cases.md) lists the host conditions each part
handles.

## The parts

| part | what it is | runs as |
| --- | --- | --- |
| `errand-box` | One Rust program (`src/`). The operator's commands, the `bailey` errand calls, and two internal helpers. | root for `setup`, `up`, `image`, `maint`; the errand account as `bailey` |
| incusd | Incus 6.0 LTS on cgroup v1 hosts, the stable line elsewhere, from the Zabbly repository where it has a build | root |
| incus-user | Part of Incus: gives each member of group `incus` one restricted project | root |
| lxcfs | Gives each guest a /proc that matches its limits | root |
| errand | The chat agent daemon (`errand run`), unchanged, from its release | the errand account |
| kage | The coding agent. errand needs a host copy on its PATH; the guest image has its own | errand account (host copy); agent user (guest copy) |
| the guest image | Debian 13 with systemd, Docker, podman, FUSE tools, build tools and kage (`image/provision.sh`) | in Incus |
| a supervisor | systemd units where systemd is PID 1, else s6 (`s6-svscan`) | root |

## A session, start to end

errand does not know Incus. It runs the program it calls `bailey`, which
setup links to `errand-box` on errand's PATH. errand-box answers each call
as follows.

1. **`bailey doctor`** at errand's start. errand-box prints the three lines
   errand reads (`landlock: ABI`, `user namespaces: yes`, `cgroup
   delegation: memory, pids, cpu`) and checks that Incus answers and the
   golden instance exists. It exits 1 if not, and errand refuses to start.
2. **`bailey trust <policy>`**. errand writes the session's policy into the
   session's state directory, which the session itself can write.
   errand-box records the policy's SHA-256 in its own state, outside any
   directory a session can reach.
3. **`bailey run ... -- true`**: errand's start-up probe, and its check
   before each session. errand-box parses the policy (an unknown key is an
   error) and, for the check, creates or updates the session's instance
   without starting it.
4. **`bailey run --egress-proxy 169.254.169.1:P ... -- kage rpc ...`**: the
   session.
   - It refuses a policy whose digest differs from the trusted one.
   - It names the instance after the session (`eb-<id>`) and copies it
     from the golden instance the first time. With Incus 6.0.1 and later a
     copy needs no rootfs rewrite, so a new instance takes about 0.5 s. A
     stopped instance made from an older image is made again (E-31).
   - It sets the instance's devices from the policy: the project at
     `/workspace`, the session state at `/state`, and two proxy devices to
     errand's broker on host 127.0.0.1:P. One listens on port P inside, for
     the agent; the other on port 3128, for Docker and apt.
   - It starts the instance and writes the agent's environment to
     `/run/errand-box/env.sh` in the guest through the Incus API, so no
     value appears on a host command line.
   - It runs `incus exec` in `/workspace`, with errand's pipes passed
     through: the agent's protocol goes straight from errand to kage and
     back. The launcher starts as the guest's root and drops to the agent
     (uid 1000) with the agent's groups, which `incus exec --user` would
     not give (E-29).
   - When the agent ends, it stops the instance, then exits with the
     agent's code.
5. **`bailey untrust <policy>`** after the session.

The instance is kept, stopped, after a session. A revived session starts it
again in about 0.3 s, with what the agent installed still there. `maint`
deletes instances unused for `box.keep_days`, or whose session errand
deleted.

## What a session can reach

| thing | how it is limited |
| --- | --- |
| The host's files | Only the two directories errand grants (and any extra grant in errand's own config). Incus refuses any disk device outside the errand account's home: that is a rule of the restricted project, enforced by incusd, not by errand-box. |
| The network | The instance has no network device. Its only way out is the two proxy devices to errand's broker, which applies errand's host allowlist and ports. Measured: a direct TCP connection fails with errno 101. |
| Host user ids | The guest's root is an unprivileged host id from a range of its own (`security.idmap.isolated`). Only guest uid 1000, the agent, maps to a real host account: the errand account, so files the agent writes in `/workspace` belong to errand. |
| CPU, memory, disk, processes | Incus limits per instance: `box.memory`, `box.cpus`, and `box.disk` as a btrfs quota. lxcfs makes /proc report them. |
| Credentials | The provider key stays with errand's broker; the guest gets a nonce. The GitHub token does reach the guest, because errand puts it in the launch environment. |
| Incus itself | The errand account is in group `incus`, not `incus-admin`. incus-user gives it one restricted project: no privileged containers, no raw LXC keys, disks only under its home, `raw.idmap` only for its own uid and gid. The project allows proxy devices for the broker link, and Incus cannot limit where one connects, so this does not hold against the errand account itself (E-61, T-020). |

## Why the session's processes cannot outlive it

errand stops a session with SIGTERM to the launcher's process group, then
SIGKILL after its grace period (10 s by default). Three things make sure
the instance stops whatever happens:

1. On SIGTERM the `incus exec` client passes the signal into the guest; the
   shim then stops the instance (a clean stop for `box.stop_seconds`, then a
   forced one) and only then exits.
2. A watchdog process, started in a session of its own, holds a pipe to the
   shim. When the pipe closes, even after a SIGKILL, it stops the instance if
   the shim's lease is still on it.
3. `maint` runs every `maintenance.reap_seconds` as root and stops any
   running session instance whose lease names a dead process. The project
   profile sets `boot.autostart=false`, so a host restart does not revive a
   session either.

## Keeping the host up

A host with no init (edge case E-01) gets s6 from its distribution's
package. `errand-box up` writes one s6 service directory per daemon, starts
`s6-svscan` if it is not running, and brings each service up. A host with
systemd keeps the units the Incus packages ship and gets units for errand
and `maint` only. Either way, `up` is the whole recovery after a restart:
device modes, kernel settings, the proxy setting, and every daemon.

Under s6, each daemon logs to `/var/log/errand-box/<daemon>/current`,
through `s6-log`, in at most ten files of 1 MB. `up` looks at each daemon
twice, 2.5 s apart, and reports one that restarts as failed, with the last
line it logged. It counts a supervisor that exited and was not reaped (a
zombie) as gone (E-33).

No code signals a daemon by name. Every guest's LXC monitor process is also
called `incusd`, so a kill by name stops every guest (E-12).
`scripts/check-no-kill-by-name.sh` enforces this in the gate.

## The guest image

`errand-box image build` launches `images:debian/13` on a bridge used only
for builds, runs `image/provision.sh` in it, and publishes the result as
`errand-box/debian-13`. The script installs Docker, podman, FUSE tools,
build tools, Python, Node.js and kage, adds the agent user (uid 1000, in
group docker, with passwordless sudo), and installs the launcher and a boot
step. The boot step adds 169.254.169.1 to `lo` and, on a cgroup v1 host,
sets podman to run without cgroups. The agent has subordinate ids 1001 to
65535 for rootless podman, whose network is slirp4netns (E-30). Session
instances never get a network device; only the build instance does.

The image records a digest of what made it: the three files of `image/`,
the kage version and the settings `provision.sh` reads. Setup rebuilds the
image when the digest differs, so a new errand-box never runs with an old
launcher (E-31).

Setup copies the image into errand's project by export and import (E-26),
and makes a golden instance from it that is never started: copies of a
never-started instance need no rootfs rewrite.

## Versions

`pins.toml` names every download with its SHA-256. `latest` in
`errand-box.toml` means the newest pinned version. errand-box refuses bytes
whose digest differs and deletes them. The freshness workflow opens a pull
request when an upstream release appears. The Incus line follows the host:
`stable` on cgroup v2, `lts-6.0` on cgroup v1 (E-04).

## What it does not do

- It does not make a guest a VM. The guests share the host's kernel; a
  kernel exploit inside a guest is a host compromise. On a host with KVM,
  Incus can run VMs instead; errand-box does not offer that yet.
- It does not change errand. Where errand's own behaviour limits the
  sandbox, this page and [edge-cases.md](edge-cases.md) say so: errand's
  broker connects to its targets directly, so a host whose only egress is
  an HTTP proxy gives guests no egress through errand (E-22).
- Rootless mode (`engine.mode = rootless`) is measured and specified in
  [research/rootless.md](research/rootless.md), and the shim already speaks
  to such a daemon, but `setup` does not install it yet.
