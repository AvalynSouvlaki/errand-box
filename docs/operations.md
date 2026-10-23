# Operations

How to install, run, update and repair errand-box. Every command here runs
as root unless it says otherwise.

## Install

1. Get the program. From a release, `bootstrap.sh` fetches it and checks
   its digest:

   ```sh
   sh bootstrap.sh
   ```

   From a checkout, build it (Rust 1.99, pinned in `rust-toolchain.toml`):

   ```sh
   cargo build --release && install -m 755 target/release/errand-box /usr/local/sbin/
   ```

2. Write the three inputs. Copies with every key are in `examples/`:
   - `errand-box.toml`: sizes, versions, the image. Every key has a default;
     an empty file is valid.
   - `errand-box.env`: errand's settings that are not secret.
   - `creds.env`: the secrets. Keep it mode 0600 and out of any repository.

3. Run setup:

   ```sh
   errand-box setup --config ./errand-box.toml --env ./errand-box.env --creds ./creds.env
   ```

   It prints one line per step: `ok` (already so), `changed`, `DEGRADED` (works
   with less, and says what is lost), `FAILED`, or `skipped`. It exits 0 when
   nothing failed. Run it again at any time: a second run on a set-up host
   changes nothing.

Without `--env` and `--creds`, setup prepares everything but leaves errand
stopped.

## What setup does

| step | result |
| --- | --- |
| packages | Incus from the Zabbly repository (the 6.0 LTS line on a cgroup v1 host), lxcfs, btrfs-progs; s6 on a host with no init |
| the errand account | a system account (default `errand`, home `/var/lib/errand`), in group `incus` |
| subordinate ids | root may delegate `1000000:1000000000`, and errand's uid and gid, to guests |
| programs | errand-box in `/usr/local/lib/errand-box/bin`, linked as `bailey` in `/usr/local/lib/errand-box/shim`; errand and kage from `pins.toml`, digests checked |
| errand's config | `~errand/.config/errand/config.json`, mode 0600, from the two env files |
| daemons | through `errand-box up` |
| storage | a btrfs pool `errand-box` on a thin loop file (half the free space, at least 20 GiB, unless `engine.pool_size` says otherwise) |
| the guest image | built on this host (`errand-box image build`), about 10 minutes |
| errand's project | made by incus-user on the errand account's first call; setup allows proxy devices in it, imports the image, and sets its default profile |
| the golden instance | `eb-golden`, created and never started |
| `/etc/errand-box/runtime.toml` | what the shim reads on every call |
| a check | `bailey doctor`, run as the errand account |

## After a restart, and at boot

```sh
errand-box up
```

It restores device modes, kernel settings, the proxy setting and every
daemon, and is safe to run any number of times. Where systemd is PID 1, the
units setup enabled start at boot. Where there is no init, nothing runs at
boot: run `errand-box up` from whatever the platform runs at start, or by
hand. The first `bailey doctor` errand runs fails until `up` has run, so
errand cannot start sessions on a half-up host.

## Look before you act

```sh
errand-box doctor
```

It changes nothing. It reports the host, every degraded condition, and
whether Incus, the golden instance and errand's config are there.

## Change a setting

Edit `/etc/errand-box/errand-box.toml` (or the file you pass with
`--config`), then run `errand-box setup` again. The profile, the pool and
`runtime.toml` follow. A new `box.disk` or `box.memory` applies to every
session instance at its next start.

## Rebuild the guest image

```sh
errand-box image build && errand-box setup
```

The build replaces the image alias and deletes the image it replaces. Setup
then imports it into errand's project and remakes the golden instance. A
session whose stopped instance came from an older image gets a new instance
at its next start; what the agent installed in the old one is lost, and the
workspace and state, which are on the host, are not (E-31).

Setup also rebuilds the image by itself when the image was made from other
inputs than this errand-box carries: another `image/` file, kage version,
registry mirror or guest proxy port. `--skip-image` keeps the old image and
reports it as degraded.

## Periodic work

The supervisor runs `errand-box maint --loop`. Each pass:
- stops any running session instance whose launcher and watchdog are gone,
  every `maintenance.reap_seconds` (60 s);
- runs `fstrim` on the pool every `maintenance.fstrim_hours` (24 h), so
  space freed inside guests returns to the host (E-25);
- once a day, deletes stopped session instances unused for `box.keep_days`
  (14), or whose session errand deleted.

`errand-box maint --once`, `errand-box reap` and `errand-box gc` run one part
by hand.

## Where things are

| path | what |
| --- | --- |
| `/etc/errand-box/errand-box.toml` | the configuration setup used |
| `/etc/errand-box/runtime.toml` | what setup decided, for the shim |
| `/etc/errand-box/proxy.env` | the egress proxy for incusd and errand, from `HTTPS_PROXY` |
| `/usr/local/lib/errand-box/` | errand-box, errand, kage, and the `bailey` link |
| `/var/lib/errand-box/s6/` | the s6 service directories (hosts with no init) |
| `/var/lib/errand-box/cache/` | downloads, named by their SHA-256 |
| `/var/log/errand-box/<daemon>/current` | each daemon's log under s6, at most ten files of 1 MB |
| `/var/log/errand-box/s6-svscan.log` | s6-svscan's own log |
| `/var/lib/incus/storage-pools/errand-box` | the pool |
| `~errand/.local/state/errand-box/` | the shim's trust records, locks and Incus client directory |

## When something fails

| symptom | look at |
| --- | --- |
| errand will not start: "bailey doctor failed" | `errand-box doctor`, then `errand-box up` |
| setup: "apt-cache policy shows no pkgs.zabbly.com origin" | the egress proxy (`HTTPS_PROXY`) and the network; apt-get update reported success without reaching it (E-14) |
| setup: guest image FAILED | the last lines it printed are the end of `provision.sh`'s output |
| a guest sees the host's memory | the `lxcfs` line of `errand-box up` (E-06, E-07, E-08) |
| a session instance keeps running after errand stopped | `errand-box reap`; then the watchdog's lease in `incus config show` |
| `up`: a daemon FAILED, "it restarts" | the last log line `up` prints, then `/var/log/errand-box/<daemon>/current` |
| errand restarts every second | `/var/log/errand-box/errand/current`; a `ConfigError` there means the config setup wrote does not fit this errand (the gate step `errand-config` checks it) |
| setup: storage pool DEGRADED (E-32) | the host disk has less free space than the pool can grow; free space, or set `engine.pool_size` before the pool is made |
