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
use std::fmt::Write as _;
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
/// A digest of the files a running daemon read at start. It goes into the
/// daemon's run script or unit, so a new program or a new proxy.env changes
/// that text, and `up` restarts the daemon (E-49).
pub fn stamp(files: &[&str]) -> String {
    let mut all = Vec::new();
    for f in files {
        all.extend_from_slice(f.as_bytes());
        all.push(0);
        all.extend(fs::read(f).unwrap_or_default());
        all.push(0);
    }
    crate::sha256::digest_hex(&all)[..16].to_owned()
}

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
            ADMIN_GROUP.into(),
            "--logfile".into(),
            "/var/log/incus/incusd.log".into(),
        ],
        env: Vec::new(),
        user: None,
        env_file: Some(PROXY_ENV.into()),
        prepare: {
            // Under s6, incusd inherits the limits of whoever ran `up`; the
            // packaged service managers raise them, so match that (E-65).
            let mut p = vec![
                "ulimit -n 1048576 2>/dev/null".to_owned(),
                "ulimit -u unlimited 2>/dev/null".to_owned(),
                "mkdir -p /var/log/incus".to_owned(),
            ];
            // Guests incusd starts before lxcfs serves get no lxcfs files.
            if let Some(l) = lx {
                p.push(format!(
                    "{SELF_BIN} __wait 30 file:{}/proc/meminfo",
                    l.mount
                ));
            }
            p
        },
        description: "Incus daemon".into(),
        down: false,
    }];
    if !lay.incus_user.is_empty() {
        v.push(Service {
            name: "incus-user".into(),
            argv: vec![lay.incus_user.clone(), "--group".into(), USER_GROUP.into()],
            env: Vec::new(),
            user: None,
            env_file: None,
            // incus-user exits when incusd does not answer yet (E-34).
            prepare: vec![format!("{SELF_BIN} __wait 120 incus")],
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
        description: format!(
            "errand-box maintenance: reap orphaned sessions, fstrim the pool, delete old instances; program {}",
            stamp(&[SELF_BIN])
        ),
        down: false,
    });
    v.push(Service {
        name: "errand".into(),
        argv: vec![ERRAND_BIN.into(), "run".into()],
        env: Vec::new(),
        user: Some(errand_user.to_owned()),
        env_file: Some(PROXY_ENV.into()),
        // errand's first `bailey doctor` needs both sockets (E-34).
        prepare: vec![format!(
            "{SELF_BIN} __wait 120 incus socket:{}",
            user_socket().display()
        )],
        description: format!(
            "errand; program, shim and proxy {}",
            stamp(&[ERRAND_BIN, SELF_BIN, PROXY_ENV])
        ),
        down: !errand_ready,
    });
    v
}

/// The group incusd gives its socket to, and the one incus-user gives its
/// socket to. Alpine's incus package creates neither, and incusd does not
/// start with a group that does not exist (E-35).
pub const ADMIN_GROUP: &str = "incus-admin";
pub const USER_GROUP: &str = "incus";

/// Create the system group `name` if it is missing. Returns whether it
/// changed.
pub fn ensure_group(name: &str, runner: &dyn Runner) -> Result<bool, String> {
    if procs::group_exists(name) {
        return Ok(false);
    }
    let out = if crate::host::which("groupadd").is_some() {
        runner.run(&Cmd::new("groupadd").args(["--system", name]))
    } else {
        runner.run(&Cmd::new("addgroup").args(["-S", name]))
    };
    if out.ok() {
        Ok(true)
    } else {
        Err(format!("cannot create group {name}: {}", out.why()))
    }
}

fn incus_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(
        crate::run::env("INCUS_DIR")
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| "/var/lib/incus".to_owned()),
    )
}

/// incusd's socket.
pub fn incus_socket() -> std::path::PathBuf {
    incus_dir().join("unix.socket")
}

/// incus-user's socket, the only one errand reaches.
pub fn user_socket() -> std::path::PathBuf {
    incus_dir().join("unix.socket.user")
}

/// True when incusd answers `GET /1.0` on `sock` with 200. A socket that
/// accepts is not enough: incusd listens before it serves.
pub fn incus_answers(sock: &Path) -> bool {
    use std::io::{Read, Write};
    let Ok(mut s) = std::os::unix::net::UnixStream::connect(sock) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
    if s.write_all(b"GET /1.0 HTTP/1.0\r\nHost: incus\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut buf = [0u8; 12];
    let mut n = 0;
    while n < buf.len() {
        match s.read(&mut buf[n..]) {
            Ok(0) | Err(_) => break,
            Ok(k) => n += k,
        }
    }
    status_is_ok(&buf[..n])
}

fn status_is_ok(head: &[u8]) -> bool {
    head.starts_with(b"HTTP/1.") && head.get(9..12) == Some(b"200")
}

/// Whether one thing `errand-box __wait` waits for is ready: `incus` (the
/// daemon answers), `file:PATH` (a byte of it reads; lxcfs serves) or
/// `socket:PATH` (it accepts a connection).
pub fn ready(what: &str) -> Result<bool, String> {
    if what == "incus" {
        return Ok(incus_answers(&incus_socket()));
    }
    match what.split_once(':') {
        Some(("file", p)) => Ok(fs::File::open(p)
            .and_then(|mut f| std::io::Read::read(&mut f, &mut [0u8; 1]))
            .is_ok()),
        Some(("socket", p)) => Ok(std::os::unix::net::UnixStream::connect(p).is_ok()),
        _ => Err(format!(
            "cannot wait for {what:?}: use incus, file:PATH or socket:PATH"
        )),
    }
}

/// `errand-box __wait SECONDS WHAT...`: block until every WHAT is ready, so
/// a daemon starts after the ones it needs (E-34). A supervisor runs it
/// before the program and starts the program even when it times out.
pub fn wait_for(whats: &[String], limit: Duration) -> Result<(), String> {
    let t = Instant::now();
    for w in whats {
        while !ready(w)? {
            if t.elapsed() > limit {
                return Err(format!("{w} is not ready after {} s", limit.as_secs()));
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    Ok(())
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
/// since a boot-time run may not see the operator's environment. The values
/// are single-quoted, which both sh and systemd's EnvironmentFile read; the
/// operator's NO_PROXY is kept after errand-box's own entries.
pub fn proxy_env_text(
    cfg: &Config,
    env: &dyn Fn(&str) -> Option<String>,
) -> Option<Result<String, String>> {
    let url = match cfg.network.proxy.as_str() {
        "none" => return Some(Ok(String::new())),
        "auto" => env("HTTPS_PROXY").or_else(|| env("https_proxy"))?,
        u => u.to_owned(),
    };
    if !proxy_url_ok(&url) {
        return Some(Err(format!(
            "the proxy {:?} has a character errand-box will not put in a shell file (quote, backslash, $, backtick, space)",
            url.chars().take(8).collect::<String>() + "..."
        )));
    }
    let mut no = vec![
        "localhost".to_owned(),
        "127.0.0.1".to_owned(),
        "::1".to_owned(),
    ];
    for extra in [env("NO_PROXY"), env("no_proxy")].into_iter().flatten() {
        for h in extra.split(',').map(str::trim).filter(|h| !h.is_empty()) {
            if proxy_url_ok(h) && !no.iter().any(|x| x == h) {
                no.push(h.to_owned());
            }
        }
    }
    let no = no.join(",");
    Some(Ok(format!(
        "# Written by errand-box up from its environment (edge case E-13).\nHTTPS_PROXY='{url}'\nHTTP_PROXY='{url}'\nhttps_proxy='{url}'\nhttp_proxy='{url}'\nNO_PROXY='{no}'\nno_proxy='{no}'\n"
    )))
}

/// True for a proxy URL or host that is safe inside single quotes.
pub fn proxy_url_ok(u: &str) -> bool {
    !u.is_empty()
        && u.chars()
            .all(|c| c.is_ascii_graphic() && !matches!(c, '\'' | '"' | '\\' | '$' | '`'))
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
    (!same_program(&exe.to_string_lossy(), &want.to_string_lossy())).then_some(pid)
}

/// True when a process's `/proc/PID/exe` names `want`. A program replaced
/// by a package upgrade reads as `PATH (deleted)`: it is the same program,
/// and replacing the whole tree for it would restart incusd (E-48).
pub fn same_program(exe: &str, want: &str) -> bool {
    exe.trim_end_matches(" (deleted)") == want
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

/// Stop and remove the service directories of services no longer in the
/// list, for example lxcfs after `choose_lxcfs` stops choosing one: left in
/// place, its old run script would keep running (E-51).
fn prune_s6(svcs: &[Service], runner: &dyn Runner, r: &mut Report) {
    let Ok(d) = fs::read_dir(SCAN_DIR) else {
        return;
    };
    let mut gone = Vec::new();
    for e in d.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || svcs.iter().any(|s| s.name == name) {
            continue;
        }
        let dir = e.path().to_string_lossy().into_owned();
        // -d stops the service by its supervisor, which signals by pid.
        let _ = runner.run(
            &Cmd::new(&s6_bin("s6-svc"))
                .args(["-wD", "-T", "10000", "-d", &dir])
                .timeout(Duration::from_secs(15)),
        );
        if fs::remove_dir_all(&dir).is_ok() {
            gone.push(name);
        }
    }
    if !gone.is_empty() {
        // -n stops the supervisors of directories that are gone.
        let _ = runner.run(&Cmd::new(&s6_bin("s6-svscanctl")).args(["-an", SCAN_DIR]));
        r.add(
            "supervisor",
            St::Changed,
            format!("removed {}: no longer run (E-51)", gone.join(", ")),
        );
    }
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
    prune_s6(svcs, runner, r);
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
            for (step, st, d) in p.rows {
                r.add(&step, st, d);
            }
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
    let svstat = |dir: &str| {
        runner
            .run(&Cmd::new(&s6_bin("s6-svstat")).arg(dir))
            .stdout
            .trim()
            .to_owned()
    };
    let mut started = Vec::new();
    for s in svcs {
        let dir = format!("{SCAN_DIR}/{}", s.name);
        if s.down {
            let _ = runner.run(&Cmd::new(&s6_bin("s6-svc")).args(["-d", &dir]));
            continue;
        }
        if changed.contains(&s.name) {
            // s6-supervise signals its own child by pid (E-12).
            let _ = runner.run(&Cmd::new(&s6_bin("s6-svc")).args(["-t", &dir]));
        }
        let _ = runner.run(&Cmd::new(&s6_bin("s6-svc")).args(["-u", &dir]));
        started.push(s.name.clone());
    }
    // All are started first, then watched together: a daemon that waits for
    // another one settles once that one is up (E-34).
    let settled = settle(&started, Duration::from_secs(30), |n| {
        svstat(&format!("{SCAN_DIR}/{n}"))
    });
    for s in svcs {
        if s.down {
            r.add(&s.name, St::Skipped, "installed, not started");
            continue;
        }
        let (ok, state) = settled
            .iter()
            .find(|x| x.0 == s.name)
            .map(|x| (x.1, x.2.clone()))
            .unwrap_or_default();
        if !ok {
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

/// How long a service must keep one pid to count as up, not restarting.
pub const STEADY: Duration = Duration::from_secs(3);

/// Watch the s6 services `names` until each has kept one pid for `STEADY`,
/// or `limit` passes. `svstat` gives a service's `s6-svstat` line. Returns
/// each name, whether it settled, and its last state.
pub fn settle(
    names: &[String],
    limit: Duration,
    svstat: impl Fn(&str) -> String,
) -> Vec<(String, bool, String)> {
    let t = Instant::now();
    let mut w: Vec<(String, Option<u32>, Instant, bool, String)> = names
        .iter()
        .map(|n| (n.clone(), None, Instant::now(), false, String::new()))
        .collect();
    loop {
        for (name, pid, since, done, state) in w.iter_mut().filter(|x| !x.3) {
            *state = svstat(name);
            let now_pid = supervise::svstat_pid(state).filter(|_| state.starts_with("up"));
            if now_pid.is_none() || now_pid != *pid {
                *pid = now_pid;
                *since = Instant::now();
            }
            let up_for = supervise::svstat_seconds(state).unwrap_or(0);
            *done = now_pid.is_some()
                && (Duration::from_secs(up_for) >= STEADY || since.elapsed() >= STEADY);
        }
        if w.iter().all(|x| x.3) || t.elapsed() >= limit {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    w.into_iter().map(|x| (x.0, x.3, x.4)).collect()
}

pub const UNIT_DIR: &str = "/etc/systemd/system";

/// The units the Incus packages ship that are started as they are. lxcfs
/// is decided apart (`lxcfs_plan`); incus-user runs from its socket.
const PACKAGED_UNITS: &[&str] = &["incus.socket", "incus.service", "incus-user.socket"];

/// The packaged unit that serves the mount the installed hook reads.
pub fn packaged_lxcfs_unit(lay: &Layout) -> &'static str {
    if lay.lxcfs_mount == "/var/lib/incus-lxcfs" {
        "incus-lxcfs.service"
    } else {
        "lxcfs.service"
    }
}

/// The drop-in errand-box adds to the packaged lxcfs unit (E-37). The unit
/// is never stopped or disabled: Zabbly's incus.service `Requires=` it, so
/// stopping it stops incusd and every running session's exec with it, and a
/// disabled one is started again by that dependency anyway. The drop-in:
/// - clears `ConditionVirtualization=!container`, so lxcfs runs on a host
///   that is itself a container;
/// - runs the lxcfs `choose_lxcfs` picked, when that is not the packaged
///   one (E-08);
/// - with no usable lxcfs, adds a condition that is never met, so the unit
///   is skipped; a skipped dependency does not stop incus.service starting.
pub fn lxcfs_dropin(lay: &Layout, lx: Option<&Lxcfs>) -> String {
    let mut s = String::from(
        "# Written by errand-box up; edits are lost on the next run (E-37).\n[Unit]\nConditionVirtualization=\n",
    );
    match lx {
        None => s.push_str(
            "# lxcfs 6 refuses every /proc file on this host, and guests then fail to start (E-08).\nConditionPathExists=/nonexistent/errand-box-no-usable-lxcfs\n",
        ),
        Some(l) if l.bin != lay.lxcfs || l.mount != lay.lxcfs_mount => {
            let _ = writeln!(s, "\n[Service]\nEnvironment=LD_LIBRARY_PATH=");
            for (k, v) in &l.env {
                let _ = writeln!(s, "Environment={k}={v}");
            }
            let _ = writeln!(s, "ExecStart=\nExecStart={} {}", l.bin, l.mount);
        }
        Some(_) => {}
    }
    s
}

fn unit_present(unit: &str) -> bool {
    ["/usr/lib/systemd/system", "/lib/systemd/system", UNIT_DIR]
        .iter()
        .any(|d| Path::new(d).join(unit).exists())
}

/// `systemctl show` properties of a unit.
fn unit_props(runner: &dyn Runner, unit: &str) -> std::collections::BTreeMap<String, String> {
    let out = runner.run(&Cmd::new("systemctl").args([
        "show",
        "-p",
        "ActiveState",
        "-p",
        "SubState",
        "-p",
        "NRestarts",
        "-p",
        "ConditionResult",
        unit,
    ]));
    out.stdout
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

/// The last line a unit logged, for a report.
fn unit_last_line(runner: &dyn Runner, unit: &str) -> String {
    let out = runner.run(&Cmd::new("journalctl").args([
        "-u",
        unit,
        "-n",
        "1",
        "-o",
        "cat",
        "--no-pager",
    ]));
    out.stdout.trim().chars().take(200).collect()
}

/// Watch a started unit for `STEADY`: it must be active and must not
/// restart meanwhile. `Err` says why not.
fn unit_settles(runner: &dyn Runner, unit: &str, limit: Duration) -> Result<(), String> {
    let t = Instant::now();
    let first = unit_props(runner, unit);
    if first.get("ConditionResult").map(String::as_str) == Some("no") {
        return Err("skipped: a condition of the unit is not met here".into());
    }
    let n0 = first.get("NRestarts").cloned().unwrap_or_default();
    let mut steady_since: Option<Instant> = None;
    loop {
        let p = unit_props(runner, unit);
        let active = p.get("ActiveState").map(String::as_str) == Some("active");
        let restarted = p.get("NRestarts").cloned().unwrap_or_default() != n0;
        if restarted {
            return Err(format!(
                "it restarts ({} restarts); last log line: {}",
                p.get("NRestarts").cloned().unwrap_or_default(),
                unit_last_line(runner, unit)
            ));
        }
        if active {
            let since = *steady_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= STEADY {
                return Ok(());
            }
        } else {
            steady_since = None;
        }
        if t.elapsed() >= limit {
            return Err(format!(
                "{} ({}); last log line: {}",
                p.get("ActiveState").cloned().unwrap_or_default(),
                p.get("SubState").cloned().unwrap_or_default(),
                unit_last_line(runner, unit)
            ));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn lxcfs_serves(mount: &str, limit: Duration) -> bool {
    let t = Instant::now();
    loop {
        if ready(&format!("file:{mount}/proc/meminfo")).unwrap_or(false) {
            return true;
        }
        if t.elapsed() >= limit {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Our own unit for a service, written if it changed. Returns the unit name
/// and whether it changed.
fn own_unit(s: &Service, after: &[&str], before: &[&str], r: &mut Report) -> (String, bool) {
    let unit = format!(
        "errand-box-{}.service",
        s.name.trim_start_matches("errand-box-")
    );
    let changed = match write_if_changed(
        &Path::new(UNIT_DIR).join(&unit),
        &supervise::systemd_unit(s, SELF_BIN, after, before),
        0o644,
    ) {
        Ok(c) => c,
        Err(e) => {
            r.add(&unit, St::Failed, e);
            false
        }
    };
    (unit, changed)
}

fn systemd_up(
    all: &[Service],
    lay: &Layout,
    lx: Option<&Lxcfs>,
    runner: &dyn Runner,
    r: &mut Report,
) {
    let systemctl = |args: &[&str]| runner.run(&Cmd::new("systemctl").args(args.iter().copied()));
    // lxcfs first: guests incusd starts before it serves get no lxcfs files.
    let packaged = packaged_lxcfs_unit(lay);
    let mount = lx.map(|l| l.mount.clone()).unwrap_or_default();
    if unit_present(packaged) {
        let path = Path::new(UNIT_DIR)
            .join(format!("{packaged}.d"))
            .join("errand-box.conf");
        let changed = match write_if_changed(&path, &lxcfs_dropin(lay, lx), 0o644) {
            Ok(c) => c,
            Err(e) => {
                r.add(packaged, St::Failed, e);
                false
            }
        };
        if changed {
            let _ = systemctl(&["daemon-reload"]);
        }
        // An older errand-box ran lxcfs from a unit of its own.
        if unit_present("errand-box-lxcfs.service") {
            let _ = systemctl(&["disable", "--now", "errand-box-lxcfs.service"]);
            let _ = fs::remove_file(Path::new(UNIT_DIR).join("errand-box-lxcfs.service"));
            let _ = systemctl(&["daemon-reload"]);
        }
        let _ = systemctl(&["enable", packaged]);
        let active = unit_props(runner, packaged)
            .get("ActiveState")
            .map(String::as_str)
            == Some("active");
        // A running lxcfs is left running when the drop-in is unchanged:
        // a restart restarts incusd too, and breaks /proc in running guests.
        let verb = match (changed, active) {
            (true, true) => Some("restart"),
            (_, false) => Some("start"),
            (false, true) => None,
        };
        let out = verb.map(|v| systemctl(&[v, packaged]));
        let why = out.as_ref().filter(|o| !o.ok()).map(|o| o.why());
        match (lx, why) {
            (None, _) => r.add(
                packaged,
                St::Degraded,
                "kept from running: lxcfs 6 makes guests fail to start here (E-08)",
            ),
            (Some(_), Some(w)) => r.add(packaged, St::Failed, w),
            (Some(_), None) if lxcfs_serves(&mount, Duration::from_secs(10)) => r.add(
                packaged,
                match verb {
                    Some("restart") => St::Changed,
                    _ if changed => St::Changed,
                    _ => St::Ok,
                },
                if verb == Some("restart") {
                    format!("serving {mount}; restarted for its new drop-in, which restarts incusd (E-37)")
                } else {
                    format!("serving {mount}")
                },
            ),
            (Some(_), None) => r.add(
                packaged,
                St::Degraded,
                format!("{mount}/proc/meminfo does not read; `errand-box logs lxcfs`"),
            ),
        }
    } else if let Some(s) = all.iter().find(|s| s.name == "lxcfs") {
        // No packaged unit: errand-box's own.
        let (unit, changed) = own_unit(s, &[], &["incus.service"], r);
        let _ = systemctl(&["daemon-reload"]);
        let _ = systemctl(&["enable", &unit]);
        let out = systemctl(&[if changed { "restart" } else { "start" }, &unit]);
        if out.ok() && lxcfs_serves(&mount, Duration::from_secs(10)) {
            r.add(
                &unit,
                if changed { St::Changed } else { St::Ok },
                format!("serving {mount}"),
            );
        } else {
            r.add(
                &unit,
                St::Degraded,
                format!("{mount}/proc/meminfo does not read: {}", out.why()),
            );
        }
    }
    let mut units = Vec::new();
    for s in all
        .iter()
        .filter(|s| matches!(s.name.as_str(), "errand" | "errand-box-maint"))
    {
        let (unit, changed) = own_unit(s, &["incus.service", "incus-user.socket"], &[], r);
        units.push((s.name.clone(), unit, changed, s.down));
    }
    let _ = systemctl(&["daemon-reload"]);
    for unit in PACKAGED_UNITS {
        if !unit_present(unit) {
            continue;
        }
        let out = systemctl(&["enable", "--now", unit]);
        if !out.ok() {
            r.add(unit, St::Failed, out.why());
            continue;
        }
        if unit.ends_with(".socket") {
            r.add(unit, St::Ok, "enabled and listening");
            continue;
        }
        match unit_settles(runner, unit, Duration::from_secs(30)) {
            Ok(()) => r.add(unit, St::Ok, "enabled and active"),
            Err(e) => r.add(unit, St::Failed, e),
        }
    }
    for (name, unit, changed, down) in &units {
        if *down {
            let _ = systemctl(&["disable", "--now", unit]);
            r.add(name, St::Skipped, "installed, not started");
            continue;
        }
        let verb = if *changed { "restart" } else { "start" };
        let _ = systemctl(&["enable", unit]);
        let out = systemctl(&[verb, unit]);
        if !out.ok() {
            r.add(name, St::Failed, format!("{unit}: {}", out.why()));
            continue;
        }
        match unit_settles(runner, unit, Duration::from_secs(30)) {
            Ok(()) => r.add(
                name,
                if *changed { St::Changed } else { St::Ok },
                format!("{unit} active"),
            ),
            Err(e) => r.add(name, St::Failed, format!("{unit}: {e}")),
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
    errand_missing(cfg).is_empty()
}

/// What errand needs and does not have yet.
pub fn errand_missing(cfg: &Config) -> Vec<String> {
    let conf = Path::new(&cfg.errand.home).join(".config/errand/config.json");
    let rt = crate::runtime::path();
    [conf.as_path(), Path::new(ERRAND_BIN), rt.as_path()]
        .iter()
        .filter(|p| !p.exists())
        .map(|p| p.display().to_string())
        .collect()
}

/// The proxy a proxy.env text sets, if any.
pub fn proxy_of(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.strip_prefix("HTTPS_PROXY="))
        .map(|v| v.trim_matches('\'').to_owned())
}

/// What to do with Incus's `core.proxy_https` and `core.proxy_http`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyStep {
    Keep,
    Set(String),
    Unset,
    /// Set by someone else: left as it is.
    Foreign(String),
}

/// incusd downloads images through `core.proxy_*` when its environment has
/// no proxy, which is the case under a packaged systemd unit: it does not
/// read proxy.env (E-13). `want` is this run's proxy (`Some("")` for none,
/// `None` to keep), `prev` the one proxy.env held before this run, `cur` the
/// key's value. A key errand-box did not set is left alone.
pub fn proxy_key_step(want: Option<&str>, prev: Option<&str>, cur: &str) -> KeyStep {
    let Some(w) = want else {
        return KeyStep::Keep;
    };
    if cur == w {
        return KeyStep::Keep;
    }
    if !cur.is_empty() && Some(cur) != prev {
        return KeyStep::Foreign(cur.to_owned());
    }
    if w.is_empty() {
        KeyStep::Unset
    } else {
        KeyStep::Set(w.to_owned())
    }
}

const NO_PROXY_HOSTS: &str = "localhost,127.0.0.1,::1";

fn incus_proxy_keys(want: Option<&str>, prev: Option<&str>, runner: &dyn Runner, r: &mut Report) {
    let inc = Incus::new(Endpoint::default(), runner);
    let cur = inc
        .query("/1.0")
        .ok()
        .and_then(|v| {
            v.pointer("/config/core.proxy_https")
                .and_then(|x| x.as_str())
                .map(str::to_owned)
        })
        .unwrap_or_default();
    match proxy_key_step(want, prev, &cur) {
        KeyStep::Keep => {}
        KeyStep::Foreign(v) => r.add(
            "incus proxy",
            St::Ok,
            format!("core.proxy_https is {v}, set outside errand-box; left as it is"),
        ),
        KeyStep::Set(u) => {
            let out = inc.run([
                "config".to_owned(),
                "set".to_owned(),
                format!("core.proxy_https={u}"),
                format!("core.proxy_http={u}"),
                format!("core.proxy_ignore_hosts={NO_PROXY_HOSTS}"),
            ]);
            r.add(
                "incus proxy",
                if out.ok() { St::Changed } else { St::Failed },
                if out.ok() {
                    "core.proxy_https and core.proxy_http set: incusd downloads images through the proxy (E-13)".to_owned()
                } else {
                    out.why()
                },
            );
        }
        KeyStep::Unset => {
            let mut why = String::new();
            for k in [
                "core.proxy_https",
                "core.proxy_http",
                "core.proxy_ignore_hosts",
            ] {
                let out = inc.run(["config", "unset", k]);
                if !out.ok() {
                    why = out.why();
                }
            }
            r.add(
                "incus proxy",
                if why.is_empty() {
                    St::Changed
                } else {
                    St::Failed
                },
                if why.is_empty() {
                    "core.proxy_* unset: no egress proxy".to_owned()
                } else {
                    why
                },
            );
        }
    }
}

fn groups(runner: &dyn Runner, r: &mut Report) {
    let mut made = Vec::new();
    for g in [ADMIN_GROUP, USER_GROUP] {
        match ensure_group(g, runner) {
            Ok(true) => made.push(g),
            Ok(false) => {}
            Err(e) => {
                r.add("groups", St::Failed, e);
                return;
            }
        }
    }
    if made.is_empty() {
        r.add("groups", St::Ok, format!("{ADMIN_GROUP}, {USER_GROUP}"));
    } else {
        r.add(
            "groups",
            St::Changed,
            format!("created {} (E-35)", made.join(", ")),
        );
    }
}

pub fn up(cfg: &Config, host: &Host, runner: &dyn Runner, r: &mut Report) {
    if paths::euid() != 0 {
        r.add("root", St::Failed, "errand-box up needs root");
        return;
    }
    devices(r);
    sysctls(r);
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let prev_proxy = fs::read_to_string(PROXY_ENV)
        .ok()
        .and_then(|t| proxy_of(&t));
    let want_text = match proxy_env_text(cfg, &env) {
        Some(Err(e)) => {
            r.add("proxy", St::Failed, e);
            None
        }
        Some(Ok(t)) => Some(t),
        None => None,
    };
    let failed_already = r.failed();
    match want_text.clone() {
        // Mode 0600: the URL may carry the proxy's credentials; only root
        // reads it, before a daemon drops its privileges.
        Some(t) => match write_if_changed(Path::new(PROXY_ENV), &t, 0o600) {
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
        None if failed_already => {}
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
    groups(runner, r);
    let svcs = services(&lay, lx.as_ref(), errand_ready(cfg), &cfg.errand.user);
    if host.systemd {
        systemd_up(&svcs, &lay, lx.as_ref(), runner, r);
    } else {
        s6_up(&svcs, runner, r);
    }
    // errand left stopped is the one thing an operator must act on: say why.
    let missing = errand_missing(cfg);
    if let Some(row) = r
        .rows
        .iter_mut()
        .find(|x| x.0 == "errand" && x.1 == St::Skipped && !missing.is_empty())
    {
        row.1 = St::Degraded;
        row.2 = format!(
            "not started: {} missing; `errand-box setup --env F --creds F` writes them",
            missing.join(", ")
        );
    }
    match wait_incus(runner, Duration::from_secs(180)) {
        Ok(v) => {
            r.add("incus answers", St::Ok, format!("server {v}"));
            let want = want_text
                .as_deref()
                .map(|t| proxy_of(t).unwrap_or_default());
            incus_proxy_keys(want.as_deref(), prev_proxy.as_deref(), runner, r);
        }
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
        let t = proxy_env_text(&cfg, &|k| match k {
            "HTTPS_PROXY" => Some("http://127.0.0.1:45823".to_owned()),
            "NO_PROXY" => Some("corp.example, 127.0.0.1".to_owned()),
            _ => None,
        })
        .unwrap()
        .unwrap();
        assert!(t.contains("HTTPS_PROXY='http://127.0.0.1:45823'\n"));
        assert!(t.contains("NO_PROXY='localhost,127.0.0.1,::1,corp.example'\n"));
        assert_eq!(proxy_of(&t).as_deref(), Some("http://127.0.0.1:45823"));
        assert_eq!(proxy_env_text(&cfg, &|_| None), None);
        let mut none = Config::default();
        none.network.proxy = "none".into();
        assert_eq!(proxy_env_text(&none, &|_| None), Some(Ok(String::new())));
        let bad = proxy_env_text(&cfg, &|k| {
            (k == "HTTPS_PROXY").then(|| "http://p:1$(id)".to_owned())
        });
        assert!(matches!(bad, Some(Err(_))));
    }

    #[test]
    fn daemons_wait_for_the_ones_they_need() {
        let l = Lxcfs {
            bin: "/opt/incus/bin/lxcfs".into(),
            env: Vec::new(),
            mount: "/var/lib/incus-lxcfs".into(),
            note: None,
        };
        let s = services(&lay(), Some(&l), true, "errand");
        let prep = |n: &str| s.iter().find(|x| x.name == n).unwrap().prepare.join("\n");
        assert!(prep("incusd").contains("__wait 30 file:/var/lib/incus-lxcfs/proc/meminfo"));
        assert!(prep("incus-user").contains("__wait 120 incus"));
        assert!(prep("errand").contains("__wait 120 incus socket:"));
        let inc = s.iter().find(|x| x.name == "incusd").unwrap();
        assert!(inc.argv.contains(&ADMIN_GROUP.to_owned()));
        assert!(
            !services(&lay(), None, true, "errand")[0]
                .prepare
                .join(" ")
                .contains("__wait")
        );
    }

    #[test]
    fn a_service_settles_only_on_a_steady_pid() {
        let names = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let calls = std::sync::Mutex::new(0u32);
        let t = Instant::now();
        let got = settle(&names, Duration::from_secs(5), |n| match n {
            // Up for long: settled at the first look.
            "a" => "up (pid 10) 400 seconds".to_owned(),
            // A new pid at every look: restarting.
            "b" => {
                let mut c = calls.lock().unwrap();
                *c += 1;
                format!("up (pid {}) 0 seconds", 100 + *c)
            }
            // Down for good.
            _ => "down (exitcode 1) 0 seconds, want up".to_owned(),
        });
        assert!(t.elapsed() >= Duration::from_secs(5));
        assert!(got[0].1);
        assert!(!got[1].1);
        assert!(!got[2].1);
        assert!(got[2].2.starts_with("down"));
        // A just-started service with one pid settles after STEADY.
        let t = Instant::now();
        let got = settle(&names[..1], Duration::from_secs(10), |_| {
            "up (pid 7) 0 seconds".into()
        });
        assert!(got[0].1);
        assert!(t.elapsed() >= STEADY && t.elapsed() < Duration::from_secs(6));
    }

    #[test]
    fn the_lxcfs_drop_in_never_stops_the_packaged_unit() {
        let ours = Lxcfs {
            bin: "/opt/incus/bin/lxcfs".into(),
            env: vec![("LD_LIBRARY_PATH".into(), "/opt/incus/lib".into())],
            mount: "/var/lib/incus-lxcfs".into(),
            note: None,
        };
        let older = Lxcfs {
            bin: "/usr/bin/lxcfs".into(),
            env: Vec::new(),
            ..ours.clone()
        };
        let same = lxcfs_dropin(&lay(), Some(&ours));
        assert!(same.contains("ConditionVirtualization=\n"));
        assert!(!same.contains("ExecStart"));
        let other = lxcfs_dropin(&lay(), Some(&older));
        assert!(other.contains("ExecStart=\nExecStart=/usr/bin/lxcfs /var/lib/incus-lxcfs\n"));
        assert!(other.contains("Environment=LD_LIBRARY_PATH=\n"));
        let none = lxcfs_dropin(&lay(), None);
        assert!(none.contains("ConditionPathExists=/nonexistent/"));
        let distro = Layout {
            lxcfs_mount: "/var/lib/lxcfs".into(),
            ..lay()
        };
        assert_eq!(packaged_lxcfs_unit(&distro), "lxcfs.service");
        assert_eq!(packaged_lxcfs_unit(&lay()), "incus-lxcfs.service");
    }

    #[test]
    fn incus_proxy_keys_follow_the_proxy_errand_box_set() {
        let p = "http://10.0.0.5:3128";
        assert_eq!(proxy_key_step(None, None, ""), KeyStep::Keep);
        assert_eq!(proxy_key_step(Some(p), None, ""), KeyStep::Set(p.into()));
        assert_eq!(proxy_key_step(Some(p), None, p), KeyStep::Keep);
        // The proxy moved; the key holds the one errand-box wrote before.
        assert_eq!(
            proxy_key_step(Some("http://10.0.0.6:3128"), Some(p), p),
            KeyStep::Set("http://10.0.0.6:3128".into())
        );
        assert_eq!(proxy_key_step(Some(""), Some(p), p), KeyStep::Unset);
        // Someone else's value stays.
        assert_eq!(
            proxy_key_step(Some(p), Some(p), "http://corp:8080"),
            KeyStep::Foreign("http://corp:8080".into())
        );
        assert_eq!(proxy_key_step(Some(""), None, ""), KeyStep::Keep);
        assert_eq!(
            proxy_of("# x\nHTTPS_PROXY=http://a:1\nHTTP_PROXY=http://a:1\n").as_deref(),
            Some("http://a:1")
        );
    }

    #[test]
    fn a_replaced_program_is_the_same_program() {
        assert!(same_program(
            "/usr/bin/s6-svscan (deleted)",
            "/usr/bin/s6-svscan"
        ));
        assert!(same_program("/usr/bin/s6-svscan", "/usr/bin/s6-svscan"));
        assert!(!same_program("/opt/s6/bin/s6-svscan", "/usr/bin/s6-svscan"));
    }

    #[test]
    fn a_stamp_follows_the_files_it_names() {
        let d = std::env::temp_dir().join(format!("eb-stamp-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let f = d.join("prog");
        fs::write(&f, "one").unwrap();
        let p = f.to_string_lossy().into_owned();
        let a = stamp(&[&p]);
        assert_eq!(a, stamp(&[&p]));
        fs::write(&f, "two").unwrap();
        assert_ne!(a, stamp(&[&p]));
        assert_eq!(a.len(), 16);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn readiness_checks() {
        assert!(status_is_ok(b"HTTP/1.0 200 OK"));
        assert!(status_is_ok(b"HTTP/1.1 200 OK"));
        assert!(!status_is_ok(b"HTTP/1.0 403 Fo"));
        assert!(!status_is_ok(b""));
        assert!(ready("bogus").is_err());
        assert!(!ready("file:/nonexistent/proc/meminfo").unwrap());
        assert!(!ready("socket:/nonexistent.socket").unwrap());
        assert!(ready("file:/proc/self/status").unwrap());
        let t = Instant::now();
        assert!(wait_for(&["file:/nonexistent".into()], Duration::from_secs(1)).is_err());
        assert!(t.elapsed() < Duration::from_secs(3));
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
