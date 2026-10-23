# Changelog

What changed, when, and where the evidence is. The version is the version
of the `errand-box` program (`Cargo.toml`); a version is a statement about
its commands, its configuration keys and the `bailey` interface it gives
errand.

---

## Unreleased

### 2026-10-23T10:28Z: hardening, QOL commands, and a host matrix

**Record:** [TODO/PROGRESS.md](TODO/PROGRESS.md); entries T-011 and T-018
closed, T-012, T-013 and T-014 to partial, T-020 added; E-34 to E-65 in
[docs/edge-cases.md](docs/edge-cases.md). Not deployed and not released: no tag, no release asset. CI is off;
every step ran through `scripts/ci-local.sh`.

- **Start-up order (E-34).** Each daemon waits for the ones it needs before
  it starts (`errand-box __wait`), and `up` watches them all until each
  keeps one pid. The crash-loop check reads s6 2.13's `pid N pgid N` format
  (E-62).
- **systemd (T-011).** `up` verifies each unit stays up, sets incusd's
  egress proxy through Incus `core.proxy_*` (E-13), and manages the packaged
  lxcfs unit with a drop-in that never stops it  -  stopping it stopped
  incusd, which `Requires=` it (E-37). Ubuntu 24.04 passes setup, the live
  tests, a reboot and a session after it.
- **OpenRC and musl.** setup creates the `incus-admin` and `incus` groups
  Alpine omits (E-35) and writes an OpenRC boot hook (E-36).
- **New commands.** `status`, `shell`, `logs`, `uninstall [--purge]`,
  `image import`, `gc --dry-run`, and `--json` for setup, up, status, doctor
  and image.
- **Lifecycle.** gc and reap do not abort on one instance (E-57) and share
  the launcher's lock (E-40); a run does not reuse a running orphan (E-56);
  maint keeps its timers across restarts (E-41); `up` falls back to the last
  applied config when the live one will not parse (E-42).
- **Security.** symlink-safe chown under errand's home (E-54); proxy.env at
  0600, quoted (E-46); the Zabbly keyring must hold exactly the pinned key
  (E-55); a per-instance process limit, and `bailey doctor` reporting the
  real limits (E-63). Found and not closed: the broker needs proxy devices,
  which let the errand account reach the admin socket (E-61, T-020).
- **The image.** `scripts/build-image.sh` builds it with distrobuilder and
  records an inputs digest; `errand-box image import` checks SHA256SUMS and
  records the digest so setup takes it as current.
- **aarch64.** The `cross` CI step builds for the other architecture and
  runs every test under qemu-user.

### 2026-10-23T08:30Z: errand-box 0.1.0, built and tested on one host

**Record:** [TODO/PROGRESS.md](TODO/PROGRESS.md), entries T-001 to T-008 in
[TODO/INDEX.md](TODO/INDEX.md), and [docs/tests.md](docs/tests.md).
Not deployed and not released: no tag, no release asset. CI is off on this
repository; every step ran through `scripts/ci-local.sh`.

- `errand-box setup` installs Incus (Zabbly, the 6.0 LTS line on cgroup v1),
  s6 on a host with no init, errand and kage at pinned digests; builds the
  guest image; and configures errand's restricted Incus project, a golden
  instance and errand itself. A second run changes nothing.
- The `bailey` shim gives each errand session its own system container:
  systemd, Docker, podman (root and rootless) and FUSE inside; a 20 GiB
  quota; 80% of the host's memory and CPUs; no network device except the
  link to errand's broker. errand v0.3.1 runs unchanged, and its own probe
  reports that the backend enforces every guarantee.
- `errand-box up` restores everything a restart loses. The supervisor's
  daemons log to bounded files; a daemon that restarts in a loop is
  reported with its last log line.
- 33 host conditions, each with a fallback and a test, in
  [docs/edge-cases.md](docs/edge-cases.md). Four of them (E-29, E-30, E-33,
  and errand's refused config key) were found by the live tests of this
  session.
- The research: the engine claims checked on a second host
  ([docs/research/engines.md](docs/research/engines.md)), and every route
  without root measured and ranked
  ([docs/research/rootless.md](docs/research/rootless.md)).
- Scheduled workflows: `freshness` opens a pull request for each newer
  errand, kage or Rust; `watchdog` opens an issue when an upstream that
  setup reads is gone.
