//! `errand-box up`: bring the host to the running state, idempotently.
//!
//! Run as root. It is safe to run any number of times: each step reads the
//! current state and changes only what differs. It is the whole recovery
//! after a host restart (E-02), so it handles what a restart loses: device
//! modes, kernel settings, and every daemon.

use crate::config::Config;
use crate::host::Host;
use crate::incus::{Endpoint, Incus};
use crate::layout::{self, Layout};
use crate::paths;
use crate::procs;
use crate::report::{Report, St};
use crate::run::{Cmd, Runner};
use crate::supervise::{self, Service};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const SCAN_DIR: &str = "/var/lib/errand-box/s6";
pub const PROXY_ENV: &str = "/etc/errand-box/proxy.env";
pub const SELF_BIN: &str = "/usr/local/lib/errand-box/bin/errand-box";
pub const ERRAND_BIN: &str = "/usr/local/lib/errand-box/bin/errand";
pub const LOG_DIR: &str = "/var/log/errand-box";

/// The lxcfs program to run, or why none runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lxcfs {
    pub bin: String,
    pub env: Vec<(String, String)>,
    pub mount: String,
    pub note: Option<String>,
}

/// Pick an lxcfs that will serve guests here (E-06, E-07, E-08).
/// - It must run at the path the installed LXC mount hook reads.
/// - lxcfs 6 refuses every /proc file when it cannot read
///   /proc/1/personality; a guest then fails to start in the mount hook. On
///   such a host only an lxcfs older than 6 is usable.
pub fn choose_lxcfs(lay: &Layout, host: &Host, runner: &dyn Runner) -> Result<Lxcfs, String> {
    let mut cands: Vec<(String, Vec<(String, String)>)> = Vec::new();
    if !lay.lxcfs.is_empty() {
        cands.push((lay.lxcfs.clone(), lay.lxcfs_env.clone()));
    }
    for p in ["/usr/bin/lxcfs", "/usr/sbin/lxcfs"] {
        if Path::new(p).exists() && !cands.iter().any(|c| c.0 == p) {
            cands.push((p.to_owned(), Vec::new()));
        }
    }
    if cands.is_empty() {
        return Err(
            "no lxcfs is installed: guests see the host's memory and CPU count in /proc (E-06)"
                .into(),
        );
    }
    for (bin, env) in &cands {
        let mut c = Cmd::new(bin)
            .arg("--version")
            .timeout(Duration::from_secs(10));
        for (k, v) in env {
            c = c.env(k, v);
        }
        let out = runner.run(&c);
        let major = layout::lxcfs_major(&format!("{}{}", out.stdout, out.stderr)).unwrap_or(0);
        if host.pid1_personality_readable || (major > 0 && major < 6) {
            let note = (!host.pid1_personality_readable).then(|| format!("lxcfs {major} is used because lxcfs 6 refuses every /proc file when /proc/1/personality cannot be read (E-08)"));
            return Ok(Lxcfs {
                bin: bin.clone(),
                env: env.clone(),
                mount: lay.lxcfs_mount.clone(),
                note,
            });
        }
    }
    Err("/proc/1/personality cannot be read here and only lxcfs 6 or later is installed, which then refuses every /proc file and makes guests fail to start; lxcfs is not run, so guests see the host's memory and CPU count (E-08). Install an lxcfs older than 6 to fix this".into())
}

/// The supervised services for root mode.
pub fn services(
    lay: &Layout,
    lx: Option<&Lxcfs>,
    errand_ready: bool,
    errand_user: &str,
) -> Vec<Service> {
    let mut v = vec![Service {
        name: "incusd".into(),
        argv: vec![
            lay.incusd.clone(),
            "--group".into(),
            "incus-admin".into(),
            "--logfile".into(),
            "/var/log/incus/incusd.log".into(),
        ],
        env: Vec::new(),
        user: None,
        env_file: Some(PROXY_ENV.into()),
        prepare: vec!["mkdir -p /var/log/incus".into()],
        description: "Incus daemon".into(),
        down: false,
    }];
    if !lay.incus_user.is_empty() {
        v.push(Service {
            name: "incus-user".into(),
            argv: vec![lay.incus_user.clone(), "--group".into(), "incus".into()],
            env: Vec::new(),
            user: None,
            env_file: None,
            prepare: Vec::new(),
            description: "Incus per-user projects (errand reaches Incus only through this)".into(),
            down: false,
        });
    }
    if let Some(l) = lx {
        v.push(Service {
            name: "lxcfs".into(),
            argv: vec![l.bin.clone(), l.mount.clone()],
            env: l.env.clone(),
            user: None,
            env_file: None,
            prepare: vec![
                format!("fusermount3 -u {m} 2>/dev/null || fusermount -u {m} 2>/dev/null || umount -l {m} 2>/dev/null", m = l.mount),
                format!("mkdir -p {}", l.mount),
            ],
            description: "lxcfs: guests see their own limits in /proc".into(),
            down: false,
        });
    }
    v.push(Service {
        name: "errand-box-maint".into(),
        argv: vec![SELF_BIN.into(), "maint".into(), "--loop".into()],
        env: Vec::new(),
        user: None,
        env_file: None,
        prepare: Vec::new(),
        description:
            "errand-box maintenance: reap orphaned sessions, fstrim the pool, delete old instances"
                .into(),
        down: false,
    });
    v.push(Service {
        name: "errand".into(),
        argv: vec![ERRAND_BIN.into(), "run".into()],
        env: Vec::new(),
        user: Some(errand_user.to_owned()),
        env_file: Some(PROXY_ENV.into()),
        prepare: Vec::new(),
        description: "errand".into(),
        down: !errand_ready,
    });
    v
}

/// Write `content` to `path` if it differs. Returns whether it changed.
pub fn write_if_changed(path: &Path, content: &str, mode: u32) -> Result<bool, String> {
    if fs::read_to_string(path).ok().as_deref() == Some(content) {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
        return Ok(false);
    }
    if let Some(d) = path.parent() {
        fs::create_dir_all(d).map_err(|e| format!("cannot create {}: {e}", d.display()))?;
    }
    let tmp = path.with_extension("errand-box-tmp");
    fs::write(&tmp, content).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(true)
}

fn devices(r: &mut Report) {
    for d in ["/dev/fuse", "/dev/net/tun"] {
        match fs::metadata(d) {
            Err(_) => r.add(
                d,
                St::Degraded,
                "missing: FUSE or tun inside guests will not work",
            ),
            Ok(m) if m.permissions().mode() & 0o777 == 0o666 => r.add(d, St::Ok, "mode 0666"),
            Ok(m) => match fs::set_permissions(d, fs::Permissions::from_mode(0o666)) {
                Ok(()) => r.add(
                    d,
                    St::Changed,
                    format!("mode {:o} -> 0666 (E-03)", m.permissions().mode() & 0o777),
                ),
                Err(e) => r.add(d, St::Degraded, format!("cannot set mode 0666: {e}")),
            },
        }
    }
}

/// Raise kernel limits that many guests with Docker inside run into.
pub const SYSCTLS: &[(&str, u64)] = &[
    ("fs/inotify/max_user_instances", 65536),
    ("fs/inotify/max_user_watches", 1048576),
    ("fs/inotify/max_queued_events", 1048576),
    ("kernel/keys/maxkeys", 20000),
    ("kernel/keys/maxbytes", 1400000),
];

fn sysctls(r: &mut Report) {
    let mut changed = Vec::new();
    let mut failed = Vec::new();
    for (k, want) in SYSCTLS {
        let p = format!("/proc/sys/{k}");
        let cur = fs::read_to_string(&p)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok());
        match cur {
            Some(c) if c >= *want => {}
            Some(_) => match fs::write(&p, want.to_string()) {
                Ok(()) => changed.push(k.replace('/', ".")),
                Err(_) => failed.push(k.replace('/', ".")),
            },
            None => failed.push(k.replace('/', ".")),
        }
    }
    if !failed.is_empty() {
        r.add(
            "sysctls",
            St::Degraded,
            format!("cannot raise {}", failed.join(", ")),
        );
    } else if changed.is_empty() {
        r.add("sysctls", St::Ok, "at or above the wanted values");
    } else {
        r.add(
            "sysctls",
            St::Changed,
            format!("raised {}", changed.join(", ")),
        );
    }
}

/// The proxy environment for incusd and errand, from this run's
/// environment (E-13). With no proxy in the environment the file is kept,
/// since a boot-time run may not see the operator's environment.
pub fn proxy_env_text(cfg: &Config, env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let url = match cfg.network.proxy.as_str() {
        "none" => return Some(String::new()),
        "auto" => env("HTTPS_PROXY").or_else(|| env("https_proxy"))?,
        u => u.to_owned(),
    };
    Some(format!(
        "# Written by errand-box up from its environment (edge case E-13).\nHTTPS_PROXY={url}\nHTTP_PROXY={url}\nhttps_proxy={url}\nhttp_proxy={url}\nNO_PROXY=localhost,127.0.0.1,::1\nno_proxy=localhost,127.0.0.1,::1\n"
    ))
}

/// An s6 program from the distribution's package (apt and apk both ship
/// one). The s6-overlay release tarballs are not used: their programs start
/// each other by absolute slashpackage paths such as
/// /package/admin/s6/command/s6-supervise, so they work only when unpacked
/// at /.
pub fn s6_bin(name: &str) -> String {
    for d in ["/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
        let p = format!("{d}/{name}");
        if Path::new(&p).exists() {
            return p;
        }
    }
    format!("/usr/bin/{name}")
}

fn svscan_lease() -> Option<String> {
    fs::read_to_string(Path::new(paths::RUN_DIR).join("s6-svscan.lease"))
        .ok()
        .map(|l| l.trim().to_owned())
}

fn svscan_running() -> bool {
    svscan_lease().is_some_and(|l| procs::lease_alive(&l))
}

/// The pid of a running s6-svscan that is not the installed one (left by an
/// older errand-box): it is replaced.
fn svscan_stale() -> Option<i32> {
    let pid = svscan_pid()?;
    let exe = fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let want = fs::canonicalize(s6_bin("s6-svscan")).ok()?;
    (exe != want).then_some(pid)
}

fn svscan_pid() -> Option<i32> {
    let l = svscan_lease()?;
    if !procs::lease_alive(&l) {
        return None;
    }
    l.split(':').next()?.parse().ok()
}

fn start_svscan() -> Result<(), String> {
    fs::create_dir_all(paths::RUN_DIR).map_err(|e| e.to_string())?;
    fs::create_dir_all(LOG_DIR).map_err(|e| e.to_string())?;
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(Path::new(LOG_DIR).join("s6-svscan.log"))
        .map_err(|e| e.to_string())?;
    let log2 = log.try_clone().map_err(|e| e.to_string())?;
    let mut c = Command::new(s6_bin("s6-svscan"));
    c.arg(SCAN_DIR)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log2));
    c.env("PATH", supervise::PATH_VALUE);
    // SAFETY: setsid and signal resets are async-signal-safe.
    unsafe {
        c.pre_exec(|| {
            procs::reset_signals_in_child()?;
            libc::setsid();
            Ok(())
        });
    }
    let child = c
        .spawn()
        .map_err(|e| format!("cannot start s6-svscan: {e}"))?;
    let pid = child.id() as i32;
    std::thread::sleep(Duration::from_millis(200));
    let lease = format!("{pid}:{}", procs::start_time(pid).unwrap_or(0));
    fs::write(Path::new(paths::RUN_DIR).join("s6-svscan.lease"), lease)
        .map_err(|e| e.to_string())?;
    // The child stays ours until this process exits; it is not waited for.
    std::mem::forget(child);
    Ok(())
}

/// Write the s6 service directories; returns the names whose run script
/// changed.
/// The services whose `run` changed, and those that got a logger only now.
pub fn write_s6(svcs: &[Service], r: &mut Report) -> (Vec<String>, Vec<String>) {
    let mut changed = Vec::new();
    let mut new_log = Vec::new();
    for s in svcs {
        let dir = Path::new(SCAN_DIR).join(&s.name);
        match write_if_changed(&dir.join("run"), &supervise::s6_run(s, SELF_BIN), 0o755) {
            Ok(true) => changed.push(s.name.clone()),
            Ok(false) => {}
            Err(e) => r.add(&format!("s6 {}", s.name), St::Failed, e),
        }
        let log_run = dir.join("log/run");
        let had_log = log_run.exists();
        let _ = fs::create_dir_all(dir.join("log"));
        match write_if_changed(
            &log_run,
            &supervise::s6_log_run(&s.name, &s6_bin("s6-log"), LOG_DIR),
            0o755,
        ) {
            Ok(_) if !had_log => new_log.push(s.name.clone()),
            Ok(_) => {}
            Err(e) => r.add(&format!("s6 {} log", s.name), St::Failed, e),
        }
        let down = dir.join("down");
        if s.down {
            let _ = fs::write(&down, "");
        } else {
            let _ = fs::remove_file(&down);
        }
    }
    (changed, new_log)
}

/// The last line `name` logged, for a report.
fn last_log_line(name: &str) -> String {
    let text =
        fs::read_to_string(Path::new(LOG_DIR).join(name).join("current")).unwrap_or_default();
    let line = text
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    // Drop the timestamp s6-log puts first.
    let line = line.split_once(' ').map(|(_, rest)| rest).unwrap_or(line);
    line.chars().take(200).collect()
}

fn s6_up(svcs: &[Service], runner: &dyn Runner, r: &mut Report) {
    if !Path::new(&s6_bin("s6-svscan")).exists() {
        r.add(
            "supervisor",
            St::Failed,
            "s6 is not installed; run `errand-box setup`",
        );
        return;
    }
    let (changed, new_log) = write_s6(svcs, r);
    // s6-svscan connects a service to its logger only when it first sees
    // the service, so a logger added to a running scan needs a new scan.
    // Daemons stopped here are started again below; incusd leaves its guests
    // running when it stops.
    let replace = svscan_stale().or_else(|| {
        if new_log.is_empty() {
            None
        } else {
            svscan_pid()
        }
    });
    if let Some(pid) = replace {
        // By pid, never by name (E-12).
        procs::kill(pid, libc::SIGTERM);
        let t = Instant::now();
        while svscan_running() && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(100));
        }
        if svscan_running() {
            procs::kill(pid, libc::SIGKILL);
            std::thread::sleep(Duration::from_millis(200));
        }
        let _ = fs::remove_file(Path::new(paths::RUN_DIR).join("s6-svscan.lease"));
    }
    // An s6-svscan that is still stopping reads as running, and exits under
    // the services this pass starts. The second pass starts a new one.
    for pass in 0..2 {
        let mut p = Report::default();
        if svscan_running() {
            let _ = runner.run(&Cmd::new(&s6_bin("s6-svscanctl")).args(["-a", SCAN_DIR]));
            p.add("supervisor", St::Ok, "s6-svscan is running");
        } else {
            match start_svscan() {
                Ok(()) => p.add(
                    "supervisor",
                    St::Changed,
                    "started s6-svscan (this host has no init, E-01)",
                ),
                Err(e) => {
                    r.add("supervisor", St::Failed, e);
                    return;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(300));
        s6_services(svcs, &changed, &new_log, runner, &mut p);
        if svscan_running() || pass == 1 {
            r.rows.extend(p.rows);
            return;
        }
    }
}

fn s6_services(
    svcs: &[Service],
    changed: &[String],
    new_log: &[String],
    runner: &dyn Runner,
    r: &mut Report,
) {
    for s in svcs {
        let dir = format!("{SCAN_DIR}/{}", s.name);
        if s.down {
            let _ = runner.run(&Cmd::new(&s6_bin("s6-svc")).args(["-d", &dir]));
            r.add(&s.name, St::Skipped, "installed, not started");
            continue;
        }
        if changed.contains(&s.name) {
            // s6-supervise signals its own child by pid (E-12).
            let _ = runner.run(&Cmd::new(&s6_bin("s6-svc")).args(["-t", &dir]));
        }
        let _ = runner.run(&Cmd::new(&s6_bin("s6-svc")).args(["-u", &dir]));
        let svstat = || {
            runner
                .run(&Cmd::new(&s6_bin("s6-svstat")).arg(&dir))
                .stdout
                .trim()
                .to_owned()
        };
        let mut state = svstat();
        // A service up for under 2 s may be restarting in a loop: look again.
        if supervise::svstat_seconds(&state).is_some_and(|n| n < 2) || !state.starts_with("up") {
            let first = supervise::svstat_pid(&state);
            std::thread::sleep(Duration::from_millis(2500));
            state = svstat();
            if !state.starts_with("up") || supervise::svstat_pid(&state) != first {
                r.add(
                    &s.name,
                    St::Failed,
                    format!(
                        "it restarts: {state}; last log line: {}",
                        last_log_line(&s.name)
                    ),
                );
                continue;
            }
        }
        r.add(
            &s.name,
            if changed.contains(&s.name) || new_log.contains(&s.name) {
                St::Changed
            } else {
                St::Ok
            },
            state,
        );
    }
}

pub const UNIT_DIR: &str = "/etc/systemd/system";

/// The units the Incus packages ship, started as they are. errand-box adds
/// units only for what the packages do not run.
const PACKAGED_UNITS: &[&str] = &[
    "incus.socket",
    "incus.service",
    "incus-user.socket",
    "incus-lxcfs.service",
    "lxcfs.service",
];

fn systemd_up(all: &[Service], runner: &dyn Runner, r: &mut Report) {
    for unit in PACKAGED_UNITS {
        let present = ["/usr/lib/systemd/system", "/lib/systemd/system", UNIT_DIR]
            .iter()
            .any(|d| Path::new(d).join(unit).exists());
        if !present {
            continue;
        }
        let out = runner.run(&Cmd::new("systemctl").args(["enable", "--now", unit]));
        if out.ok() {
            r.add(unit, St::Ok, "enabled and started");
        } else {
            r.add(unit, St::Failed, out.why());
        }
    }
    let svcs: Vec<Service> = all
        .iter()
        .filter(|s| matches!(s.name.as_str(), "errand" | "errand-box-maint"))
        .cloned()
        .collect();
    let svcs = &svcs[..];
    let mut changed = Vec::new();
    for s in svcs {
        let unit = format!(
            "errand-box-{}.service",
            s.name.trim_start_matches("errand-box-")
        );
        let after: &[&str] = &["incus.service"];
        match write_if_changed(
            &Path::new(UNIT_DIR).join(&unit),
            &supervise::systemd_unit(s, SELF_BIN, after),
            0o644,
        ) {
            Ok(true) => changed.push(unit.clone()),
            Ok(false) => {}
            Err(e) => r.add(&unit, St::Failed, e),
        }
    }
    let _ = runner.run(&Cmd::new("systemctl").arg("daemon-reload"));
    for s in svcs {
        let unit = format!(
            "errand-box-{}.service",
            s.name.trim_start_matches("errand-box-")
        );
        if s.down {
            let _ = runner.run(&Cmd::new("systemctl").args(["disable", "--now", &unit]));
            r.add(&s.name, St::Skipped, "installed, not started");
            continue;
        }
        let verb = if changed.contains(&unit) {
            "restart"
        } else {
            "start"
        };
        let _ = runner.run(&Cmd::new("systemctl").args(["enable", &unit]));
        let out = runner.run(&Cmd::new("systemctl").args([verb, &unit]));
        if out.ok() {
            r.add(
                &s.name,
                if changed.contains(&unit) {
                    St::Changed
                } else {
                    St::Ok
                },
                format!("{unit} active"),
            );
        } else {
            r.add(&s.name, St::Failed, format!("{unit}: {}", out.why()));
        }
    }
}

/// Wait until the daemon answers.
pub fn wait_incus(runner: &dyn Runner, limit: Duration) -> Result<String, String> {
    let inc = Incus::new(Endpoint::default(), runner);
    let t = Instant::now();
    loop {
        match inc.server_version() {
            Ok(v) => return Ok(v),
            Err(e) if t.elapsed() > limit => return Err(e),
            Err(_) => std::thread::sleep(Duration::from_millis(500)),
        }
    }
}

/// errand starts only when its config, its program and the sandbox
/// settings all exist; before that its first `bailey doctor` would fail.
pub fn errand_ready(cfg: &Config) -> bool {
    Path::new(&cfg.errand.home)
        .join(".config/errand/config.json")
        .exists()
        && Path::new(ERRAND_BIN).exists()
        && crate::runtime::path().exists()
}

pub fn up(cfg: &Config, host: &Host, runner: &dyn Runner, r: &mut Report) {
    if paths::euid() != 0 {
        r.add("root", St::Failed, "errand-box up needs root");
        return;
    }
    devices(r);
    sysctls(r);
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    match proxy_env_text(cfg, &env) {
        Some(t) => match write_if_changed(Path::new(PROXY_ENV), &t, 0o644) {
            Ok(true) => r.add(
                "proxy",
                St::Changed,
                if t.is_empty() {
                    "no egress proxy".to_owned()
                } else {
                    format!("{PROXY_ENV} updated from HTTPS_PROXY")
                },
            ),
            Ok(false) => r.add(
                "proxy",
                St::Ok,
                if t.is_empty() {
                    "no egress proxy"
                } else {
                    "unchanged"
                },
            ),
            Err(e) => r.add("proxy", St::Failed, e),
        },
        None => r.add(
            "proxy",
            St::Ok,
            "no HTTPS_PROXY in this environment; the previous setting is kept",
        ),
    }
    let lay = layout::detect();
    if lay.flavor.is_empty() {
        r.add(
            "incus",
            St::Failed,
            "no incusd is installed; run `errand-box setup`",
        );
        return;
    }
    let lx = match choose_lxcfs(&lay, host, runner) {
        Ok(l) => {
            r.add(
                "lxcfs",
                if l.note.is_some() {
                    St::Degraded
                } else {
                    St::Ok
                },
                l.note
                    .clone()
                    .unwrap_or_else(|| format!("{} at {}", l.bin, l.mount)),
            );
            Some(l)
        }
        Err(e) => {
            r.add("lxcfs", St::Degraded, e);
            None
        }
    };
    let svcs = services(&lay, lx.as_ref(), errand_ready(cfg), &cfg.errand.user);
    if host.systemd {
        systemd_up(&svcs, runner, r);
    } else {
        s6_up(&svcs, runner, r);
    }
    match wait_incus(runner, Duration::from_secs(90)) {
        Ok(v) => r.add("incus answers", St::Ok, format!("server {v}")),
        Err(e) => r.add("incus answers", St::Failed, e),
    }
    if let Some(l) = &lx {
        let t = Instant::now();
        let mut ok = false;
        while t.elapsed() < Duration::from_secs(10) {
            if fs::read_to_string(format!("{}/proc/meminfo", l.mount)).is_ok() {
                ok = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        if ok {
            r.add(
                "lxcfs serves",
                St::Ok,
                format!("{}/proc/meminfo reads", l.mount),
            );
        } else {
            r.add(
                "lxcfs serves",
                St::Degraded,
                format!("{}/proc/meminfo does not read", l.mount),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::Fake;

    fn lay() -> Layout {
        Layout {
            flavor: "zabbly".into(),
            incusd: "/opt/incus/lib/systemd/incusd".into(),
            incus_user: "/opt/incus/lib/systemd/incus-user".into(),
            lxcfs: "/opt/incus/bin/lxcfs".into(),
            lxcfs_env: vec![("LD_LIBRARY_PATH".into(), "/opt/incus/lib".into())],
            lxcfs_mount: "/var/lib/incus-lxcfs".into(),
        }
    }

    #[test]
    fn lxcfs_6_is_refused_where_pid1_personality_is_hidden() {
        let f = Fake::new();
        f.on("/opt/incus/bin/lxcfs --version", 0, "\"6.0.6\"");
        let host = Host::default();
        let e = choose_lxcfs(&lay(), &host, &f);
        // /usr/bin/lxcfs may or may not exist on the test machine; with no
        // older lxcfs the choice must be an error, never lxcfs 6.
        match e {
            Ok(l) => assert_ne!(l.bin, "/opt/incus/bin/lxcfs"),
            Err(msg) => assert!(msg.contains("E-08")),
        }
        let readable = Host {
            pid1_personality_readable: true,
            ..Host::default()
        };
        assert_eq!(
            choose_lxcfs(&lay(), &readable, &f).unwrap().bin,
            "/opt/incus/bin/lxcfs"
        );
    }

    #[test]
    fn errand_is_installed_down_until_configured() {
        let s = services(&lay(), None, false, "errand");
        let e = s.iter().find(|s| s.name == "errand").unwrap();
        assert!(e.down);
        assert_eq!(e.user.as_deref(), Some("errand"));
        assert!(!s.iter().any(|s| s.name == "lxcfs"));
        let inc = s.iter().find(|s| s.name == "incusd").unwrap();
        assert_eq!(inc.argv[0], "/opt/incus/lib/systemd/incusd");
    }

    #[test]
    fn proxy_env_from_the_environment() {
        let cfg = Config::default();
        let t = proxy_env_text(&cfg, &|k| {
            (k == "HTTPS_PROXY").then(|| "http://127.0.0.1:45823".to_owned())
        })
        .unwrap();
        assert!(t.contains("HTTPS_PROXY=http://127.0.0.1:45823\n"));
        assert_eq!(proxy_env_text(&cfg, &|_| None), None);
        let mut none = Config::default();
        none.network.proxy = "none".into();
        assert_eq!(proxy_env_text(&none, &|_| None).as_deref(), Some(""));
    }

    #[test]
    fn write_if_changed_reports_change_once() {
        let p = std::env::temp_dir()
            .join(format!("eb-wic-{}", std::process::id()))
            .join("f");
        assert!(write_if_changed(&p, "a", 0o644).unwrap());
        assert!(!write_if_changed(&p, "a", 0o644).unwrap());
        assert!(write_if_changed(&p, "b", 0o644).unwrap());
    }
}
