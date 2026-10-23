# Tests

Four kinds, from the cheapest. Each one prints PASS or FAIL per check and
exits 0 only when every check passed. Read the exit code unpiped.

| kind | needs | command |
| --- | --- | --- |
| 1. unit and integration tests | cargo | `cargo test --locked` |
| 2. the local CI steps | cargo, shellcheck, curl, the network for one step | `sh scripts/ci-local.sh` |
| 3. the live tests | root on a host where `errand-box setup` finished | `sh tests/live/live.sh` |
| 4. other kinds of host | root on a host where Incus runs | `sh tests/hosts/host.sh NAME IMAGE` |

## 1. Unit and integration tests

`cargo test` runs the unit tests of every module and `tests/shim.rs`.

`tests/shim.rs` runs the real `bailey` shim against `tests/fake-incus`, a
POSIX sh stand-in for the Incus client that keeps instances as directories
and logs every call. It checks `doctor`, the probe, trust and a rewritten
policy, a whole session with errand's pipes, SIGTERM, SIGKILL caught by the
watchdog, and `reap`. Every call the shim makes goes through the `Runner`
trait; the unit tests answer it with `run::Fake`, so a test can state the
exact Incus calls a function makes.

A test named in [edge-cases.md](edge-cases.md) as `module::test` is one of
these.

## 2. The local CI steps

[`scripts/ci-local.sh`](../scripts/ci-local.sh) runs the steps that
`.github/workflows/gates.yml` runs, one by one, and the workflow calls the same
script, so the two cannot differ. CI is off on this repository: run this
before every push, and say in a pull request that CI did not run.

| step | what it proves |
| --- | --- |
| `gate` | the template's checks (docs, markers, one home per sentence, placeholders, control bytes, changelog, attribution, secrets) and `scripts/check-todo.sh` |
| `kill-by-name` | no code signals a daemon by name (E-12) |
| `shellcheck` | every shell script this repository ships |
| `fmt`, `clippy` | format, and clippy with warnings as errors |
| `test` | section 1 |
| `pins` | `tests/pins-test.sh`: `scripts/pins.sh` against releases on `file://` URLs. A bump writes both digests and moves `latest`; a digest that differs from upstream's `.sha256` changes nothing |
| `errand-config` | `tests/errand-config-test.sh`: the pinned errand accepts the config errand-box writes. It downloads errand (digest checked) and runs it against the config from `tests/data/`. This step caught `output.postImages`, a key that errand v0.3.1 refuses |
| `musl` | the static release binary builds for this machine and runs |
| `cross` | the static binary builds for the other architecture (x86_64 or aarch64) with `rust-lld`, and, where `qemu-<arch>-static` is installed, the unit and integration tests pass there under qemu-user |

## 3. The live tests

[`tests/live/live.sh`](../tests/live/live.sh) starts real session
instances through the installed shim, as the errand account, with the policy
shape errand writes. It changes the host: `up` stops every daemon and starts
them again. Run it after `setup` and after any change to the shim, the image
or `up`. Name tests to run some of them: `sh tests/live/live.sh session inside`.

| test | what it checks | edge cases |
| --- | --- | --- |
| `probe` | `bailey doctor` as errand; the per-session check `run -- true` | |
| `session` | a session with a stand-in broker: the agent's uid and groups, the workspace, host ownership of what the agent writes, no network device and no default route, the broker reachable, direct egress blocked, the file size limit, the environment file deleted, systemd as PID 1, the instance stopped and its lease cleared | E-19, E-29 |
| `acp` | `kage rpc` in a session answers errand's `initialize` and `session/new` | |
| `inside` | Docker run and build, `sudo podman`, rootless podman, a FUSE mount by the agent, the CPU and memory limits as the guest sees them. Images are pulled through a relay that stands in for errand's broker: set `EB_LIVE_PROXY` (or `HTTPS_PROXY`), and `EB_LIVE_CA` for a proxy that intercepts TLS | E-03, E-04, E-06, E-16, E-29, E-30 |
| `quota` | a 5 GiB copy of the golden instance accepts 1 GiB and refuses 6 GiB | E-05, E-21 |
| `signals` | SIGTERM, then SIGKILL, to the launcher's process group: the instance stops and the lease clears | E-27 |
| `reap` | with the launcher and its watchdog both killed, `errand-box reap` stops the instance | E-27 |
| `maint` | `maint --once`, with fstrim | E-25 |
| `up` | every daemon stopped through s6, device modes reset to 0600, then `errand-box up`: every service up, a second `up` changes nothing, the probe passes | E-01, E-02, E-03, E-33 |
| `errand` | errand runs under the supervisor, accepts its config, and its own probe of the shim says the backend enforces every guarantee | |

### The last run

2026-10-23, on the build host (Ubuntu 24.04 in a hosted container, kernel
6.18, x86_64, 4 CPUs, 16 GiB, cgroup v1, no init, no KVM), Incus 6.0.6,
errand v0.3.1, kage v0.5.0. The first run found four defects; each has a
fix, a row in [edge-cases.md](edge-cases.md), and a test:

| found by | defect | fixed by |
| --- | --- | --- |
| `inside` | `docker`: "permission denied" on its socket, because `incus exec --user` gives no supplementary groups | E-29 |
| `inside` | rootless podman: no subordinate ids, then pasta could not copy a default route | E-30 |
| `up` | after s6-svscan stopped, `up` reported it running and started nothing: the dead process was a zombie that PID 1 never reaped | E-33 |
| `errand` (setup) | errand v0.3.1 refused the config and restarted every second: `output.postImages` is a key of a later errand | the `errand-config` step |

The run after the fixes passed every check, 52 of 52:
[`tests/live/2026-10-23.txt`](../tests/live/2026-10-23.txt) is its output.

### What the live tests do not cover

- WSL2, and a real aarch64 machine: the `cross` step runs the unit and
  integration tests for aarch64 under qemu-user, which proves the code, not
  a host. Each is an open entry in [`TODO/INDEX.md`](../TODO/INDEX.md).
- errand with a real chat service and a real model gateway. That needs a
  bot token, a channel, a user id, and a gateway key and URL; the operator
  supplies them as described in [`SECURITY.md`](../SECURITY.md).

## 4. Other kinds of host

[`tests/hosts/host.sh`](../tests/hosts/host.sh) makes another kind of host
as a nested Incus system container: systemd as PID 1 on Ubuntu and Debian,
OpenRC and musl on Alpine. In it, it installs errand-box through
`bootstrap.sh` from a release served the way GitHub serves one, runs setup
twice, runs the live tests, restarts the container, and checks that
errand-box comes back. The test credentials are not real, so the errand
daemon crash-loops (T-015); that is the one service the setup checks allow
to fail.

```sh
sh tests/hosts/host.sh u2404 ubuntu/24.04
sh tests/hosts/host.sh deb13 debian/13
sh tests/hosts/host.sh alpine alpine/3.22
sh tests/hosts/host.sh u2404 ubuntu/24.04 destroy
```

A nested host cannot make a loop-file pool, so it uses a `dir` pool and the
`quota` live test is skipped (E-05). Set `EB_HOSTS_CA` to the proxy's CA
when the egress proxy intercepts TLS.
