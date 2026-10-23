# Changelog

What changed, when, and where the evidence is. The version is the version
of the `errand-box` program (`Cargo.toml`); a version is a statement about
its commands, its configuration keys and the `bailey` interface it gives
errand.

---

## Unreleased

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
