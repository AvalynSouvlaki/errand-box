//! One errand session in one Incus instance.
//!
//! The instance is named after the session, copied from the golden instance
//! the first time, and kept (stopped) between runs, so what the agent
//! installed survives a revived session. Its host directories and its link to
//! errand's broker are set again on every run from the policy errand wrote.
//!
//! Lifetime rules (docs/bailey-contract.md section 8):
//! - the launcher exits only after the instance is stopped;
//! - SIGTERM to errand's process group stops the agent, then the instance;
//! - SIGKILL of the launcher cannot run any code, so a watchdog in its own
//!   session holds a pipe to the launcher and stops the instance when the
//!   pipe closes. `errand-box reap` catches what both miss (E-27).

use crate::incus::{Incus, Instance};
use crate::paths;
use crate::policy::{Mount, Policy};
use crate::procs;
use crate::run::Runner;
use crate::runtime::Runtime;
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const PREFIX: &str = "eb-";
pub const LEASE_KEY: &str = "user.errand-box.lease";
pub const SESSION_KEY: &str = "user.errand-box.session";
pub const LAST_USED_KEY: &str = "user.errand-box.last-used";
/// The fingerprint of the image a golden instance (and each copy of it) was
/// made from.
pub const IMAGE_KEY: &str = "user.errand-box.image";
pub const LAUNCHER: &str = "/usr/local/libexec/errand-box/launch";
pub const ENV_FILE: &str = "/run/errand-box/env.sh";
/// The address errand gives the session for its broker.
pub const BROKER_ADDR: &str = "169.254.169.1";

/// An Incus instance name for a session id: `eb-` plus the id made safe,
/// with a digest suffix whenever the id had to change or is long.
pub fn instance_name(id: &str) -> String {
    let lower = id.to_ascii_lowercase();
    let mut clean = String::new();
    for c in lower.chars() {
        let c = if c.is_ascii_alphanumeric() { c } else { '-' };
        if c == '-' && clean.ends_with('-') {
            continue;
        }
        clean.push(c);
    }
    let clean = clean.trim_matches('-').to_owned();
    // `eb-golden` and `eb-golden-next` are setup's (E-43); a session id that
    // would land on them gets the digest form.
    let reserved = clean == "golden" || clean.starts_with("golden-");
    if !clean.is_empty() && clean == id && clean.len() <= 50 && !reserved {
        return format!("{PREFIX}{clean}");
    }
    let digest = &crate::sha256::digest_hex(id.as_bytes())[..10];
    let head: String = clean.chars().take(40).collect();
    let head = head.trim_matches('-');
    if head.is_empty() {
        format!("{PREFIX}{digest}")
    } else {
        format!("{PREFIX}{head}-{digest}")
    }
}

/// The devices a run needs, by name. errand-box owns every device whose
/// name starts with `eb-`; it leaves others alone.
pub fn desired_devices(
    mounts: &[Mount],
    broker: Option<u16>,
    rt: &Runtime,
    relay_sock: Option<&str>,
) -> BTreeMap<String, (String, BTreeMap<String, String>)> {
    let mut out = BTreeMap::new();
    for (i, m) in mounts.iter().enumerate() {
        let name = match m.path.as_str() {
            crate::policy::WORKSPACE => "eb-workspace".to_owned(),
            crate::policy::STATE => "eb-state".to_owned(),
            _ => format!("eb-mount{i}"),
        };
        let mut p = BTreeMap::new();
        p.insert("source".into(), m.source.clone());
        p.insert("path".into(), m.path.clone());
        if m.readonly {
            p.insert("readonly".into(), "true".into());
        }
        out.insert(name, ("disk".to_owned(), p));
    }
    if let Some(port) = broker {
        let connect = match relay_sock {
            Some(s) => format!("unix:{s}"),
            None => format!("tcp:127.0.0.1:{port}"),
        };
        for (name, listen) in [("eb-broker", port), ("eb-sysproxy", rt.guest_proxy_port)] {
            let mut p = BTreeMap::new();
            p.insert("listen".into(), format!("tcp:0.0.0.0:{listen}"));
            p.insert("connect".into(), connect.clone());
            p.insert("bind".into(), "instance".into());
            out.insert(name.to_owned(), ("proxy".to_owned(), p));
        }
    }
    out
}

/// Make the instance's `eb-` devices equal `want`: remove what differs or is
/// no longer wanted, then add what is missing.
pub fn reconcile(
    inc: &Incus,
    inst: &Instance,
    want: &BTreeMap<String, (String, BTreeMap<String, String>)>,
) -> Result<(), String> {
    for (name, props) in &inst.devices {
        if !name.starts_with(PREFIX) {
            continue;
        }
        let same = want.get(name).is_some_and(|(kind, p)| {
            props.get("type") == Some(kind)
                && p.iter().all(|(k, v)| props.get(k) == Some(v))
                && props.len() == p.len() + 1
        });
        if !same {
            inc.device_remove(&inst.name, name)?;
        }
    }
    let refreshed = inc
        .instance(&inst.name)?
        .ok_or("the instance disappeared")?;
    for (name, (kind, props)) in want {
        if !refreshed.devices.contains_key(name) {
            inc.device_add(&inst.name, name, kind, props)?;
        }
    }
    Ok(())
}

/// Create the session's instance from the golden one if it does not exist,
/// and set its devices. Returns the instance as it is now.
pub fn prepare(
    inc: &Incus,
    rt: &Runtime,
    policy: &Policy,
    broker: Option<u16>,
    relay_sock: Option<&str>,
) -> Result<Instance, String> {
    let id = policy.session_id()?;
    let name = instance_name(&id);
    let skip = |p: &str| {
        rt.host_runtime_dirs
            .iter()
            .any(|d| p == d || p.starts_with(&format!("{d}/")))
    };
    let mounts = policy.mounts(&skip)?;
    let mut existing = inc.instance(&name)?;
    if let Some(i) = &existing {
        // A copy of an older golden instance has an older launcher. The
        // rootfs holds nothing errand needs (the workspace and the state are
        // host mounts), so a stopped one starts over from the current image
        // (E-31).
        if !i.running() {
            let golden = inc
                .instance(&rt.golden)?
                .ok_or_else(|| format!("{} is missing; run `errand-box setup`", rt.golden))?;
            if i.config.get(IMAGE_KEY) != golden.config.get(IMAGE_KEY) {
                inc.delete(&name)?;
                existing = None;
            }
        }
    }
    let inst = match existing {
        Some(i) => i,
        None => {
            inc.copy(&rt.golden, &name, &[(SESSION_KEY.to_owned(), id.clone())])
                .map_err(|e| format!("cannot copy {} to {name}: {e}", rt.golden))?;
            inc.instance(&name)?.ok_or("the new instance is missing")?
        }
    };
    reconcile(
        inc,
        &inst,
        &desired_devices(&mounts, broker, rt, relay_sock),
    )?;
    inc.config_set(&name, &[(LAST_USED_KEY.to_owned(), now_secs().to_string())])?;
    inc.instance(&name)?
        .ok_or_else(|| "the instance disappeared".to_owned())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Single-quote a value for POSIX sh.
pub fn sh_quote(v: &str) -> String {
    format!("'{}'", v.replace('\'', "'\\''"))
}

/// The environment file the guest launcher reads: one `export` per variable,
/// then `EB_FSIZE` for the file size limit.
pub fn env_script(env: &BTreeMap<String, String>, fsize: Option<u64>) -> String {
    let mut s = String::from("# Written by errand-box for one run. The launcher deletes it.\n");
    for (k, v) in env {
        if crate::policy::valid_env_name(k) && !k.starts_with("EB_") {
            s.push_str(&format!("export {k}={}\n", sh_quote(v)));
        }
    }
    if let Some(n) = fsize {
        s.push_str(&format!("EB_FSIZE={n}\n"));
    }
    s
}

/// The lock a launcher holds for its instance. The launcher, its watchdog
/// and root's `reap` lock the same file (runtime.toml `lock_dir`); a file
/// under the caller's own state directory would differ between root and
/// the errand account, and lock nothing.
pub fn lock_path(rt: &Runtime, name: &str) -> PathBuf {
    let dir = if rt.lock_dir.is_empty() {
        paths::state_dir().join("locks")
    } else {
        PathBuf::from(&rt.lock_dir)
    };
    dir.join(format!("{name}.lock"))
}

fn say(quiet: bool, msg: &str) {
    if !quiet {
        eprintln!("errand-box: {msg}");
    }
}

/// Wait until a command runs inside the instance.
fn wait_ready(inc: &Incus, name: &str, limit: Duration) -> Result<(), String> {
    let t = Instant::now();
    loop {
        let out = inc.exec(name, &["true"]);
        if out.ok() {
            return Ok(());
        }
        if t.elapsed() > limit {
            return Err(format!(
                "{name} did not answer within {}s: {}",
                limit.as_secs(),
                out.why()
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The arguments for `incus exec` that start the agent.
pub fn exec_args(rt: &Runtime, name: &str, cmd: &[String]) -> Vec<String> {
    let mut a = Vec::new();
    if !rt.project.is_empty() {
        a.push("--project".to_owned());
        a.push(rt.project.clone());
    }
    // The launcher starts as the guest's root and drops to the agent with
    // its supplementary groups; `--user` would leave it none (E-29).
    a.extend(
        [
            "exec",
            "-T",
            "--cwd",
            crate::policy::WORKSPACE,
            name,
            "--",
            LAUNCHER,
            &rt.guest_uid.to_string(),
            &rt.guest_gid.to_string(),
        ]
        .map(str::to_owned),
    );
    a.extend(cmd.iter().cloned());
    a
}

/// Run one session to its end. Returns the exit code for the launcher.
pub fn run(
    rt: &Runtime,
    runner: &dyn Runner,
    policy: &Policy,
    broker: Option<u16>,
    cmd: &[String],
    from_env: &dyn Fn(&str) -> Option<String>,
    quiet: bool,
) -> i32 {
    let set = procs::term_signals();
    procs::block(&set);
    match run_inner(rt, runner, policy, broker, cmd, from_env, quiet, set) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("errand-box: {e}");
            125
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_inner(
    rt: &Runtime,
    runner: &dyn Runner,
    policy: &Policy,
    broker: Option<u16>,
    cmd: &[String],
    from_env: &dyn Fn(&str) -> Option<String>,
    quiet: bool,
    set: libc::sigset_t,
) -> Result<i32, String> {
    let inc = Incus::new(rt.endpoint(), runner);
    let id = policy.session_id()?;
    let name = instance_name(&id);
    let _lock = procs::lock(&lock_path(rt, &name), false)?;

    let relay = match (rt.broker_link.as_str(), broker) {
        ("unix", Some(port)) => {
            let sock = paths::state_dir()
                .join("relay")
                .join(format!("{name}.sock"));
            std::fs::create_dir_all(sock.parent().unwrap()).map_err(|e| e.to_string())?;
            crate::relay::unix_to_tcp(&sock, format!("127.0.0.1:{port}"))
                .map_err(|e| format!("cannot start the broker relay: {e}"))?;
            Some(sock.to_string_lossy().into_owned())
        }
        _ => None,
    };

    let mut inst = prepare(&inc, rt, policy, broker, relay.as_deref())?;
    if inst.running() {
        match inst.config.get(LEASE_KEY) {
            Some(lease) if procs::lease_alive(lease) => {
                return Err(format!("{name} is already running for launcher {lease}"));
            }
            _ => {
                // Running with no live launcher: stop it so this run starts
                // from a clean guest, not one that carries the dead
                // session's processes (E-56). run_agent then starts it.
                say(
                    quiet,
                    &format!("{name} was left running without a launcher; restarting it clean"),
                );
                inc.stop(&name, rt.stop_seconds)?;
                inst = inc
                    .instance(&name)?
                    .ok_or_else(|| format!("{name} disappeared"))?;
            }
        }
    }
    let lease = procs::self_lease();
    inc.config_set(&name, &[(LEASE_KEY.to_owned(), lease.clone())])?;
    let watchdog = spawn_watchdog(&name, &lease)?;

    let result = run_agent(rt, &inc, &inst, policy, cmd, from_env, quiet, set);

    say(quiet, &format!("stopping {name}"));
    if let Err(e) = inc.stop(&name, rt.stop_seconds) {
        say(false, &format!("could not stop {name}: {e}"));
    }
    let _ = inc.config_unset(&name, LEASE_KEY);
    drop(watchdog);
    result
}

#[allow(clippy::too_many_arguments)]
fn run_agent(
    rt: &Runtime,
    inc: &Incus,
    inst: &Instance,
    policy: &Policy,
    cmd: &[String],
    from_env: &dyn Fn(&str) -> Option<String>,
    quiet: bool,
    set: libc::sigset_t,
) -> Result<i32, String> {
    let name = &inst.name;
    if !inst.running() {
        inc.start(name)
            .map_err(|e| format!("cannot start {name}: {e}"))?;
    }
    wait_ready(inc, name, Duration::from_secs(30))?;
    let env = policy.agent_env(from_env);
    inc.file_push(
        name,
        ENV_FILE,
        env_script(&env, policy.file_max).into_bytes(),
        "0600",
        rt.guest_uid,
        rt.guest_gid,
    )
    .map_err(|e| format!("cannot write the session environment: {e}"))?;

    let mut c = Command::new(&rt.incus_bin);
    c.args(exec_args(rt, name, cmd));
    c.env_clear();
    for k in ["PATH", "LANG", "LC_ALL", "TERM"] {
        if let Some(v) = std::env::var_os(k) {
            c.env(k, v);
        }
    }
    if !rt.incus_dir.is_empty() {
        c.env("INCUS_DIR", &rt.incus_dir);
    }
    // Never the session's HOME: see crate::incus::conf_dir.
    c.env("INCUS_CONF", crate::incus::conf_dir());
    if let Some(h) = paths::home_of(paths::euid()) {
        c.env("HOME", h);
    }
    c.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // SAFETY: the closure only calls async-signal-safe functions.
    unsafe {
        c.pre_exec(procs::reset_signals_in_child);
    }
    let mut child = c
        .spawn()
        .map_err(|e| format!("cannot run {}: {e}", rt.incus_bin))?;
    let pid = child.id() as i32;
    let done = Arc::new(AtomicBool::new(false));
    {
        let done = done.clone();
        std::thread::spawn(move || {
            let sig = procs::wait_signal(&set);
            if done.load(Ordering::SeqCst) {
                return;
            }
            // errand signals the whole group, so the client usually has the
            // signal already; sending it again is harmless.
            procs::kill(pid, libc::SIGTERM);
            let _ = sig;
            std::thread::sleep(Duration::from_secs(3));
            if !done.load(Ordering::SeqCst) {
                procs::kill(pid, libc::SIGKILL);
            }
        });
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    done.store(true, Ordering::SeqCst);
    say(quiet, &format!("agent in {name} exited: {status}"));
    Ok(status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0)))
}

/// The watchdog's write end. Dropping it closes the pipe.
pub struct Watchdog {
    _w: std::fs::File,
}

fn spawn_watchdog(name: &str, lease: &str) -> Result<Watchdog, String> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: pipe2 writes two descriptors into the array we own.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err("cannot create the watchdog pipe".into());
    }
    use std::os::fd::FromRawFd;
    // SAFETY: both descriptors were just created and are owned here.
    let (r, w) = unsafe {
        (
            std::fs::File::from_raw_fd(fds[0]),
            std::fs::File::from_raw_fd(fds[1]),
        )
    };
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut c = Command::new(exe);
    c.arg0("errand-box").args(["__watchdog", name, lease]);
    c.stdin(Stdio::from(r))
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid and signal resets are async-signal-safe.
    unsafe {
        c.pre_exec(|| {
            procs::reset_signals_in_child()?;
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    c.spawn()
        .map_err(|e| format!("cannot start the watchdog: {e}"))?;
    Ok(Watchdog { _w: w })
}

/// The watchdog: wait for the launcher's pipe to close, then stop the
/// instance if the launcher's lease is still on it.
pub fn watchdog(rt: &Runtime, runner: &dyn Runner, name: &str, lease: &str) -> i32 {
    // SAFETY: ignoring signals has no memory effects.
    unsafe {
        for s in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::signal(s, libc::SIG_IGN);
        }
    }
    let mut sink = [0u8; 64];
    loop {
        // SAFETY: read into a buffer we own; fd 0 is the pipe.
        let n = unsafe { libc::read(0, sink.as_mut_ptr().cast(), sink.len()) };
        if n == 0
            || (n < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted)
        {
            break;
        }
    }
    // The launcher has ended. If it ended cleanly it holds no lock now and
    // has removed its lease.
    let _lock = procs::lock(&lock_path(rt, name), true);
    let inc = Incus::new(rt.endpoint(), runner);
    if let Ok(Some(inst)) = inc.instance(name)
        && inst.config.get(LEASE_KEY).map(String::as_str) == Some(lease)
    {
        let _ = inc.stop(name, rt.stop_seconds);
        let _ = inc.config_unset(name, LEASE_KEY);
    }
    0
}

/// Stop every running session instance whose launcher is gone.
pub fn reap(rt: &Runtime, runner: &dyn Runner, out: &mut dyn Write) -> Result<usize, String> {
    let inc = Incus::new(rt.endpoint_for_caller(), runner);
    let mut n = 0;
    for inst in inc.instances(PREFIX)? {
        if inst.name == rt.golden || !inst.running() {
            continue;
        }
        let alive = inst
            .config
            .get(LEASE_KEY)
            .is_some_and(|l| procs::lease_alive(l));
        if alive {
            continue;
        }
        let Ok(_lock) = procs::lock(&lock_path(rt, &inst.name), false) else {
            continue;
        };
        let _ = writeln!(out, "stopping {}: its launcher is gone", inst.name);
        // One instance that will not stop must not stop the whole pass (E-57).
        match inc.stop(&inst.name, rt.stop_seconds) {
            Ok(()) => {
                let _ = inc.config_unset(&inst.name, LEASE_KEY);
                n += 1;
            }
            Err(e) => {
                let _ = writeln!(out, "could not stop {}: {e}", inst.name);
            }
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::Fake;

    fn rt() -> Runtime {
        Runtime {
            mode: "root".into(),
            incus_bin: "incus".into(),
            incus_dir: String::new(),
            project: String::new(),
            golden: "eb-golden".into(),
            guest_uid: 1000,
            guest_gid: 1000,
            stop_seconds: 4,
            guest_proxy_port: 3128,
            broker_link: "tcp".into(),
            host_runtime_dirs: vec![],
            admin_project: String::new(),
            state_root: String::new(),
            lock_dir: String::new(),
        }
    }

    #[test]
    fn names() {
        assert_eq!(instance_name("abc123"), "eb-abc123");
        let n = instance_name("Thread_42/x");
        assert!(n.starts_with("eb-thread-42-x-"), "{n}");
        let long = "a".repeat(80);
        assert!(instance_name(&long).len() <= 63);
        assert!(instance_name("___").starts_with("eb-"));
        assert_ne!(instance_name("A"), instance_name("a"));
        for id in ["golden", "golden-next", "Golden"] {
            let n = instance_name(id);
            assert!(
                n.starts_with("eb-golden-") && n.len() > "eb-golden-next".len(),
                "{n}"
            );
        }
    }

    #[test]
    fn devices_for_a_session_with_a_broker() {
        let m = vec![
            Mount {
                source: "/p".into(),
                path: "/workspace".into(),
                readonly: false,
            },
            Mount {
                source: "/s".into(),
                path: "/state".into(),
                readonly: false,
            },
            Mount {
                source: "/data".into(),
                path: "/data".into(),
                readonly: true,
            },
        ];
        let d = desired_devices(&m, Some(41234), &rt(), None);
        assert_eq!(d["eb-workspace"].1["source"], "/p");
        assert_eq!(d["eb-mount2"].1["readonly"], "true");
        assert_eq!(d["eb-broker"].1["listen"], "tcp:0.0.0.0:41234");
        assert_eq!(d["eb-broker"].1["connect"], "tcp:127.0.0.1:41234");
        assert_eq!(d["eb-sysproxy"].1["listen"], "tcp:0.0.0.0:3128");
        let d = desired_devices(&m, Some(41234), &rt(), Some("/r/eb-x.sock"));
        assert_eq!(d["eb-broker"].1["connect"], "unix:/r/eb-x.sock");
        let d = desired_devices(&m, None, &rt(), None);
        assert!(!d.contains_key("eb-broker"));
    }

    #[test]
    fn env_script_quotes_and_skips_bad_names() {
        let mut env = BTreeMap::new();
        env.insert("A".to_owned(), "it's $HOME".to_owned());
        env.insert("BAD-NAME".to_owned(), "x".to_owned());
        env.insert("EB_FSIZE".to_owned(), "1".to_owned());
        let s = env_script(&env, Some(1024));
        assert!(s.contains("export A='it'\\''s $HOME'\n"));
        assert!(!s.contains("BAD-NAME"));
        assert!(s.ends_with("EB_FSIZE=1024\n"));
        assert_eq!(s.matches("EB_FSIZE").count(), 1);
    }

    #[test]
    fn exec_args_start_the_launcher_in_the_workspace() {
        let a = exec_args(&rt(), "eb-x", &["kage".into(), "rpc".into()]);
        assert_eq!(
            a.join(" "),
            "exec -T --cwd /workspace eb-x -- /usr/local/libexec/errand-box/launch 1000 1000 kage rpc"
        );
        let mut r = rt();
        r.project = "p".into();
        assert!(
            exec_args(&r, "eb-x", &[])
                .join(" ")
                .starts_with("--project p exec")
        );
    }

    #[test]
    fn reconcile_removes_stale_and_adds_missing() {
        let f = Fake::new();
        f.on(
            "incus query /1.0/instances/eb-x",
            0,
            r#"{"name":"eb-x","status":"Stopped","config":{},"devices":{"eb-broker":{"type":"proxy","listen":"tcp:0.0.0.0:1","connect":"tcp:127.0.0.1:1","bind":"instance"},"root":{"type":"disk"}}}"#,
        );
        let inc = Incus::new(rt().endpoint(), &f);
        let inst = inc.instance("eb-x").unwrap().unwrap();
        let m = vec![
            Mount {
                source: "/p".into(),
                path: "/workspace".into(),
                readonly: false,
            },
            Mount {
                source: "/s".into(),
                path: "/state".into(),
                readonly: false,
            },
        ];
        reconcile(&inc, &inst, &desired_devices(&m, Some(2), &rt(), None)).unwrap();
        let lines = f.lines();
        assert!(
            lines.contains(&"incus config device remove eb-x eb-broker".to_owned()),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("incus config device add eb-x eb-workspace disk"))
        );
        assert!(!lines.iter().any(|l| l.contains("remove eb-x root")));
    }

    #[test]
    fn prepare_copies_the_golden_instance_once() {
        let f = Fake::new();
        f.on(
            "incus query /1.0/instances/eb-abc123",
            1,
            "Error: Instance not found",
        );
        f.on("incus copy", 0, "");
        let inc = Incus::new(rt().endpoint(), &f);
        let p = crate::policy::parse(&crate::policy::tests_sample()).unwrap();
        // After the copy the fake still answers "not found", so prepare
        // reports the missing instance; the copy call is what is tested.
        let _ = prepare(&inc, &rt(), &p, None, None);
        assert!(
            f.lines()
                .iter()
                .any(|l| l
                    == "incus copy eb-golden eb-abc123 --config user.errand-box.session=abc123"),
            "{:?}",
            f.lines()
        );
    }

    fn with_image(name: &str, status: &str, image: &str) -> String {
        serde_json::json!({ "name": name, "status": status, "config": { IMAGE_KEY: image }, "devices": {} }).to_string()
    }

    #[test]
    fn prepare_remakes_a_stopped_copy_of_an_older_image() {
        let p = crate::policy::parse(&crate::policy::tests_sample()).unwrap();
        for (status, image, remade) in [
            ("Stopped", "old", true),
            ("Stopped", "new", false),
            ("Running", "old", false),
        ] {
            let f = Fake::new();
            f.on(
                "incus query /1.0/instances/eb-abc123",
                0,
                &with_image("eb-abc123", status, image),
            );
            f.on(
                "incus query /1.0/instances/eb-golden",
                0,
                &with_image("eb-golden", "Stopped", "new"),
            );
            let inc = Incus::new(rt().endpoint(), &f);
            let _ = prepare(&inc, &rt(), &p, None, None);
            let lines = f.lines();
            assert_eq!(
                lines.iter().any(|l| l == "incus delete --force eb-abc123"),
                remade,
                "{status} {image}: {lines:?}"
            );
            assert_eq!(
                lines
                    .iter()
                    .any(|l| l.starts_with("incus copy eb-golden eb-abc123")),
                remade,
                "{status} {image}: {lines:?}"
            );
        }
    }
}
