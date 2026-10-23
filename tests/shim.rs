//! The bailey interface end to end, as errand calls it, against a fake
//! `incus` (tests/fake-incus). No Incus, root or network is needed, so these
//! run in every build and on CI.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

struct World {
    root: PathBuf,
    policy: PathBuf,
}

const SESSION: &str = "abc123";

fn world(name: &str) -> World {
    let root = std::env::temp_dir().join(format!("eb-it-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake-incus"),
        bin.join("incus"),
    )
    .unwrap();
    fs::set_permissions(bin.join("incus"), fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_errand-box"), bin.join("bailey")).unwrap();
    // The golden instance.
    let g = root.join("fake-state/instances/eb-golden");
    fs::create_dir_all(g.join("config")).unwrap();
    fs::create_dir_all(g.join("devices")).unwrap();
    fs::write(g.join("status"), "Stopped").unwrap();
    fs::write(
        root.join("runtime.toml"),
        format!(
            "mode = 'root'\nincus_bin = '{}'\ngolden = 'eb-golden'\nguest_uid = 1000\nguest_gid = 1000\nstop_seconds = 1\nguest_proxy_port = 3128\nbroker_link = 'tcp'\nhost_runtime_dirs = ['/usr/local/lib/errand-box/bin']\nstate_root = '{}'\n",
            bin.join("incus").display(),
            root.join("errand/state").display()
        ),
    )
    .unwrap();
    // errand's directories and the policy it writes for one session.
    let st = root.join("errand/state").join(SESSION);
    let proj = root.join("errand/projects/demo");
    fs::create_dir_all(st.join("home")).unwrap();
    fs::create_dir_all(&proj).unwrap();
    for d in [&root, &root.join("errand"), &root.join("errand/state"), &st] {
        fs::set_permissions(d, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let policy = st.join("policy.toml");
    fs::write(
        &policy,
        format!(
            r#"# Generated per session by errand. Do not edit.
[filesystem]
reset = true
read = ["/usr", "/etc/ssl", {{ path = "{sd}/resolv.conf", at = "/etc/resolv.conf" }}, {{ path = "{p}", at = "/workspace" }}, {{ path = "{st}", at = "/state" }}, "/usr/local/lib/errand-box/bin"]
write = [{{ path = "{p}", at = "/workspace" }}, {{ path = "{st}", at = "/state" }}]
execute = ["/usr", "/state/home/bin", {{ path = "{p}", at = "/workspace" }}]
[[device]]
path = "/dev/null"
access = "rw"

[env]
pass = ["PROVIDER_KEY", "XDG_DATA_HOME"]

[env.set]
PATH = "/state/home/bin:/usr/bin:/bin"
HOME = "/state/home"
HTTPS_PROXY = "http://169.254.169.1:41234"

[resources]
file_max = "1g"
tmp_size = "512m"
shm_size = "256m"

[network]
egress_allow = [{{ host = "*", port = 443 }}]
"#,
            st = st.display(),
            sd = root.join("errand/state").display(),
            p = proj.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&policy, fs::Permissions::from_mode(0o600)).unwrap();
    World { root, policy }
}

impl World {
    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(self.root.join("bin/bailey"));
        c.args(args);
        c.env(
            "PATH",
            format!("{}:/usr/bin:/bin", self.root.join("bin").display()),
        );
        c.env("ERRAND_BOX_RUNTIME", self.root.join("runtime.toml"));
        c.env("ERRAND_BOX_STATE", self.root.join("eb-state"));
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).stdin(Stdio::null()).output().unwrap()
    }

    fn log(&self) -> String {
        fs::read_to_string(self.root.join("fake-state/log")).unwrap_or_default()
    }

    fn inst(&self) -> PathBuf {
        self.root.join("fake-state/instances/eb-abc123")
    }

    fn status(&self) -> String {
        fs::read_to_string(self.inst().join("status"))
            .unwrap_or_default()
            .trim()
            .to_owned()
    }

    fn policy(&self) -> &str {
        self.policy.to_str().unwrap()
    }

    fn session(&self, command: &[&str]) -> Child {
        let p = self.policy().to_owned();
        let mut args = vec![
            "run",
            "--isolate",
            "--egress-proxy",
            "169.254.169.1:41234",
            "--config",
            &p,
            "--profile",
            "ai-agent",
            "--",
        ];
        args.extend_from_slice(command);
        let mut c = self.cmd(&args);
        c.env("PROVIDER_KEY", "nonce-for-test");
        c.env("XDG_DATA_HOME", "/state/kage");
        c.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // errand starts the launcher in a process group of its own.
        c.process_group(0);
        c.spawn().unwrap()
    }
}

fn wait_for(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < limit {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn doctor_prints_what_errand_parses_and_fails_without_golden() {
    let w = world("doctor");
    let out = w.run(&["doctor"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    assert!(lines.iter().any(|l| l.starts_with("landlock: ABI")));
    assert!(lines.contains(&"user namespaces: yes"));
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("cgroup delegation:") && !l.ends_with("no"))
    );
    fs::remove_dir_all(w.root.join("fake-state/instances/eb-golden")).unwrap();
    assert_eq!(w.run(&["doctor"]).status.code(), Some(1));
}

#[test]
fn probe_needs_trust_and_refuses_a_rewritten_policy() {
    let w = world("probe");
    let p = w.policy();
    let probe = ["run", "--config", p, "--quiet", "--", "true"];
    let out = w.run(&probe);
    assert_eq!(
        out.status.code(),
        Some(1),
        "an untrusted policy must be refused"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("not trusted"));
    let t = w.run(&["trust", p]);
    assert!(t.status.success(), "{}", String::from_utf8_lossy(&t.stderr));
    assert_eq!(
        String::from_utf8_lossy(&t.stdout).trim(),
        format!("trusted {p}")
    );
    assert!(w.run(&probe).status.success());
    // The shape probe creates no instance.
    assert!(!w.inst().exists());
    // The per-session check prepares the instance without starting it.
    let check = w.run(&[
        "run",
        "--isolate",
        "--config",
        p,
        "--profile",
        "untrusted",
        "--quiet",
        "--",
        "true",
    ]);
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(!String::from_utf8_lossy(&check.stderr).contains("not applying"));
    assert_eq!(w.status(), "Stopped");
    // A session that rewrites its own policy cannot get it applied.
    fs::write(
        &w.policy,
        fs::read_to_string(&w.policy).unwrap().replace("1g", "9g"),
    )
    .unwrap();
    let out = w.run(&probe);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("changed after"));
    assert!(w.run(&["untrust", p]).status.success());
    assert_eq!(w.run(&["untrust", p]).status.code(), Some(1));
}

#[test]
fn a_session_passes_the_agent_protocol_through_and_stops_its_instance() {
    let w = world("session");
    assert!(w.run(&["trust", w.policy()]).status.success());
    let mut child = w.session(&["cat"]);
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1}\n")
        .unwrap();
    let mut out = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    let status = child.wait().unwrap();
    let mut err = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    assert!(status.success(), "{err}");
    assert_eq!(
        out, "{\"jsonrpc\":\"2.0\",\"id\":1}\n",
        "stdout carries only the agent's bytes"
    );
    let log = w.log();
    for want in [
        "copy eb-golden eb-abc123 --config user.errand-box.session=abc123",
        "config device add eb-abc123 eb-workspace disk",
        "config device add eb-abc123 eb-state disk",
        "config device add eb-abc123 eb-broker proxy bind=instance connect=tcp:127.0.0.1:41234 listen=tcp:0.0.0.0:41234",
        "config device add eb-abc123 eb-sysproxy proxy bind=instance connect=tcp:127.0.0.1:41234 listen=tcp:0.0.0.0:3128",
        "start eb-abc123",
        "file push - eb-abc123/run/errand-box/env.sh --mode 0600 --uid 1000 --gid 1000",
        "exec -T --cwd /workspace eb-abc123 -- /usr/local/libexec/errand-box/launch 1000 1000 cat",
        "stop eb-abc123",
    ] {
        assert!(log.contains(want), "missing call {want:?} in:\n{log}");
    }
    // Host system paths, the resolv.conf grant and the host kage directory
    // are not attached: the guest has its own.
    assert!(!log.contains("eb-mount"), "{log}");
    // The environment went through the API, not argv.
    assert!(!log.contains("nonce-for-test"));
    let env = fs::read_to_string(w.inst().join("files/run/errand-box/env.sh")).unwrap();
    assert!(
        env.contains("export PROVIDER_KEY='nonce-for-test'\n"),
        "{env}"
    );
    assert!(env.contains("export HOME='/state/home'\n"));
    assert!(env.contains("EB_FSIZE=1073741824\n"));
    assert_eq!(w.status(), "Stopped");
    assert!(!w.inst().join("config/user.errand-box.lease").exists());
}

#[test]
fn sigterm_to_the_group_ends_the_agent_and_stops_the_instance() {
    let w = world("sigterm");
    assert!(w.run(&["trust", w.policy()]).status.success());
    let mut child = w.session(&["sleep", "30"]);
    let pid = child.id() as i32;
    assert!(
        wait_for(Duration::from_secs(10), || w.log().contains("exec -T")),
        "{}",
        w.log()
    );
    std::thread::sleep(Duration::from_millis(300));
    let t = Instant::now();
    // SAFETY: signalling the test's own child process group.
    unsafe { libc::kill(-pid, libc::SIGTERM) };
    let status = child.wait().unwrap();
    assert!(
        t.elapsed() < Duration::from_secs(8),
        "took {:?}",
        t.elapsed()
    );
    assert!(!status.success());
    assert_eq!(w.status(), "Stopped", "{}", w.log());
    assert!(w.log().contains("stop eb-abc123"));
}

#[test]
fn sigkill_of_the_launcher_is_caught_by_the_watchdog() {
    let w = world("sigkill");
    assert!(w.run(&["trust", w.policy()]).status.success());
    let mut child = w.session(&["sleep", "30"]);
    let pid = child.id() as i32;
    assert!(
        wait_for(Duration::from_secs(10), || w.log().contains("exec -T")),
        "{}",
        w.log()
    );
    assert!(w.inst().join("config/user.errand-box.lease").exists());
    // SAFETY: signalling the test's own child process group.
    unsafe { libc::kill(-pid, libc::SIGKILL) };
    let _ = child.wait();
    assert!(
        wait_for(Duration::from_secs(15), || w.status() == "Stopped"
            && !w.inst().join("config/user.errand-box.lease").exists()),
        "the watchdog did not stop the instance:\n{}",
        w.log()
    );
}

#[test]
fn reap_stops_an_instance_whose_launcher_is_gone() {
    let w = world("reap");
    let d = w.inst();
    fs::create_dir_all(d.join("config")).unwrap();
    fs::create_dir_all(d.join("devices")).unwrap();
    fs::write(d.join("status"), "Running").unwrap();
    // pid 1 never has start time 1, so this lease names no live process.
    fs::write(d.join("config/user.errand-box.lease"), "1:1").unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_errand-box"));
    c.arg("reap");
    c.env(
        "PATH",
        format!("{}:/usr/bin:/bin", w.root.join("bin").display()),
    );
    c.env("ERRAND_BOX_RUNTIME", w.root.join("runtime.toml"));
    c.env("ERRAND_BOX_STATE", w.root.join("eb-state"));
    let out = c.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(w.status(), "Stopped");
}
