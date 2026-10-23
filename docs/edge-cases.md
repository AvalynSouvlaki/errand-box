# Edge cases

Each row is a host condition that broke something during the research or the
build, what errand-box does about it, and the test that proves the fallback.
The code cites these by number (`E-NN`).

"Seen" names where the condition was measured: `research` for the research
branch of agent-box (`research/errand-sandbox/`), and `build` for the session
that wrote errand-box. [research/engines.md](research/engines.md) holds the
numbers. A test named `module::test` is a Rust unit test (`cargo test`); a
test named `live:` is a step of [tests.md](tests.md) section 3, which needs a
host with Incus.

| id | condition | effect | what errand-box does | test |
| --- | --- | --- | --- | --- |
| E-01 | No init on the host: PID 1 is a platform process | Nothing starts or restarts a daemon | `up` installs s6 and starts `s6-svscan`, which keeps every daemon running | `supervise::tests::s6_script_drops_privileges_and_execs`; live: up after a restart |
| E-02 | The host restarted | Device modes, kernel settings and every daemon are lost; disk state stays | `up` puts all of it back, and is safe to run again. Seen in build: a restart at about 07:40Z | live: up after a restart |
| E-03 | `/dev/fuse` and `/dev/net/tun` boot at mode 0600 | FUSE and tun inside guests fail | `up` sets mode 0666 at every run | live: up after a restart |
| E-04 | cgroup v1 | Incus 7.0 and later, podman 6, LXC 7 and systemd 258 and later refuse it; podman inside a guest cannot create cgroups | `engine.incus = auto` pins the 6.0 LTS line on a v1 host and says so; the guest's boot step sets podman to `cgroups = "disabled"` on v1 | `host::tests::channel_follows_cgroup_version`; live: podman inside |
| E-05 | An Incus `dir` storage pool | `root.size` is accepted and not enforced | The default pool is btrfs; `pool_driver = "dir"` is reported as degraded | live: quota |
| E-06 | No lxcfs | A guest sees the host's memory and CPU count in /proc | `up` runs lxcfs under the supervisor | live: lxcfs |
| E-07 | The LXC mount hook path differs by packaging | lxcfs at the wrong path is never bound into guests | `up` reads the installed hook (`/var/lib/lxcfs` or `/var/lib/incus-lxcfs`) and runs lxcfs there | `layout::tests::hook_paths` |
| E-08 | `/proc/1/personality` cannot be read, even by root | lxcfs 6 refuses every /proc file, and each guest start fails in the mount hook | `up` uses an installed lxcfs older than 6, or runs none and reports degraded | `up::tests::lxcfs_6_is_refused_where_pid1_personality_is_hidden` |
| E-09 | `df` in a guest on a btrfs pool | Shows the pool size, not the quota | Stated; the quota is still enforced | (documented) |
| E-10 | The host's egress proxy listens on 127.0.0.1 and allows CONNECT only | A build guest cannot reach it; plain-http apt gets 405 | The image build relays the proxy onto the build bridge for its duration; apt uses https | `image::tests::loopback_proxies_need_a_relay`; live: image build |
| E-11 | The Incus client with stdin open | It reads instance YAML from a non-terminal stdin and blocks until it closes (measured: 23 ms with stdin closed, blocked past 60 s with it open) | Every call closes stdin and has a deadline | `run::tests::stdin_is_closed_by_default`, `run::tests::a_deadline_kills_a_hung_program` |
| E-12 | A signal to incusd by name | Every guest's monitor is also named `incusd`, so the guests stop | Daemons are stopped only through their supervisor, which signals by pid. No code signals by name | `scripts/check-no-kill-by-name.sh` |
| E-13 | The egress proxy port moves across a restart (seen twice: 33337 to 42151, and 45823 to 44687) | A relay or daemon with the old port gets no answer | `up` reads `HTTPS_PROXY` on every run and rewrites `/etc/errand-box/proxy.env` | `up::tests::proxy_env_from_the_environment` |
| E-14 | `apt-get update` exits 0 when every fetch failed | A later install fails far from the cause | setup and the image build check `apt-cache policy` for the expected origin | live: setup, image build |
| E-15 | The proxy CA is at a platform path, and a bundle of many certificates is skipped by `update-ca-certificates` | TLS through the proxy fails in the guest | `network.proxy_ca` (or `PROXY_CA`) names one CA file; apt uses it through `Acquire::https::CaInfo`; it is removed from the image after the build | live: image build |
| E-16 | Docker Hub rate limits anonymous pulls (429) | `docker pull` in guests fails | The guest's Docker uses `image.registry_mirror` (default `mirror.gcr.io`) | live: docker inside |
| E-17 | No AppArmor in the kernel | Incus loses the AppArmor profile it adds | Reported as degraded; seccomp and isolated uid ranges remain | `errand-box doctor` |
| E-18 | Root lacks CAP_SYS_RESOURCE | Sysbox fails; Incus does not | Reported | `errand-box doctor` |
| E-19 | `security.idmap.isolated=true` | Each instance has its own uid range | Set for every session; with idmapped mounts (E-20) it costs nothing | live: golden copy |
| E-20 | Incus 6.0.0 (Ubuntu 24.04's package) never writes the map of the user namespace it probes idmapped mounts with, so the kernel answers EINVAL | Every new isolated instance rewrites its rootfs: 2.1 to 2.7 s instead of 0.4 to 0.5 s | setup installs Incus from the Zabbly repository where it can, and reports 6.0.0 as degraded | `setup::tests::versions` |
| E-21 | A `root.size` below the space the instance already uses | The instance can neither stop cleanly nor start ("Failed clearing ownership: disk quota exceeded") | `box.disk` below 4 GiB is refused | `config::tests::a_disk_quota_below_the_image_is_refused` |
| E-22 | errand's broker connects to its targets directly | On a host whose only egress is an HTTP proxy, guest traffic through errand's broker fails | Stated in [design.md](design.md); errand-box does not change errand | (documented) |
| E-23 | A rootless incusd lives in a network namespace of its own | A proxy device cannot reach host 127.0.0.1 | The shim relays the broker through a unix socket (`broker_link = "unix"`) | `relay::tests::unix_socket_reaches_a_tcp_server` |
| E-24 | Rootless Incus on btrfs | `btrfs qgroup limit` needs root, so no disk quota | Reported as degraded in rootless mode | (documented in [research/rootless.md](research/rootless.md)) |
| E-25 | A btrfs pool on a loop file | Space freed inside a guest returns to the host only after `fstrim` | `maint` runs `fstrim` on the pool every `maintenance.fstrim_hours` | live: maint |
| E-26 | `incus image copy` between two projects of one server | Fails: "The source server isn't listening on the network" | setup exports the image and imports it into errand's project | live: setup |
| E-27 | The session launcher is killed with SIGKILL | The guest process dies with the client, but the instance keeps running | A watchdog in its own session stops the instance when the launcher's pipe closes; `maint` reaps what both miss; `boot.autostart=false` stops a restart from reviving it | `session::tests` (lease), live: SIGKILL |
| E-28 | incusd restarts, after a host restart, every instance that was running | A session instance runs with no launcher | The project profile sets `boot.autostart=false` | live: setup |
| E-29 | `incus exec --user N` gives the process no supplementary groups | The agent is not in the guest's `docker` group, so `docker` gets "permission denied" on its socket. Seen in build | The shim runs the launcher as the guest's root; the launcher drops to the agent with `setpriv --init-groups` | `session::tests::exec_args_start_the_launcher_in_the_workspace`; live: docker inside |
| E-30 | The guest has 65536 ids, and a restricted project forbids a larger `security.idmap.size`; the guest has no default route | Rootless podman as the agent has no subordinate range ("cannot set up namespace using newuidmap"), and then pasta fails ("Couldn't set IPv4 route(s) in guest"). Seen in build | The image gives the agent ids 1001 to 65535 and makes slirp4netns the rootless network; an image file owned by an id above 64535 needs `sudo podman` | live: podman inside |
| E-31 | A new errand-box changes a file of the guest image, such as the launcher | An old image or an old session instance runs a launcher that does not match the shim | The image records a digest of its inputs; setup rebuilds an image whose digest differs, and a session remakes a stopped instance made from an older image | `image::tests::the_inputs_digest_follows_every_input`, `session::tests::prepare_remakes_a_stopped_copy_of_an_older_image`; live: setup |
| E-32 | The loop file of the pool is sparse, and the host disk has less free space than the pool can still grow | When the host disk fills, every guest gets I/O errors. Seen in build: a 20 GiB pool on a disk with 14 GiB free | setup reports it as degraded with both numbers; `engine.pool_size` sets a smaller pool | `setup::tests::a_loop_pool_larger_than_the_free_disk_is_reported` |
| E-33 | PID 1 does not reap orphans (a platform process, not an init) | A daemon that exits stays a zombie with its pid and start time, so a `pid:starttime` lease still reads alive. Seen in build: s6-svscan stopped, and `up` kept reporting it as running while every daemon was down | A lease whose process is a zombie counts as dead | `procs::tests::a_zombie_lease_is_dead`; live: up after a restart |
