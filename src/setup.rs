//! `errand-box setup`: everything, from a bare host to errand running its
//! sessions in Incus. Run as root. Idempotent: each step reads the current
//! state first and changes only what differs, so a second run reports
//! nothing changed.

use crate::config::Config;
use crate::errandcfg;
use crate::fetch;
use crate::host::{self, Host};
use crate::image;
use crate::incus::{Endpoint, Incus};
use crate::paths;
use crate::procs;
use crate::report::{Report, St};
use crate::run::{Cmd, Runner};
use crate::runtime::{self, Runtime};
use crate::session::IMAGE_KEY;
use crate::up::{self, ERRAND_BIN, SELF_BIN};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Args {
    pub config: PathBuf,
    pub env: Option<PathBuf>,
    pub creds: Option<PathBuf>,
    pub skip_image: bool,
}

pub const GOLDEN: &str = "eb-golden";
pub const GUEST_UID: u32 = 1000;
pub const ZABBLY_KEY_URL: &str = "https://pkgs.zabbly.com/key.asc";
pub const ZABBLY_FPR: &str = "4EFC590696CB15B87C73A3AD82CC8797C838DCFD";
const BIN_DIR: &str = "/usr/local/lib/errand-box/bin";
const SHIM_DIR: &str = "/usr/local/lib/errand-box/shim";

fn long(c: Cmd) -> Cmd {
    c.timeout(Duration::from_secs(1800))
        .env("DEBIAN_FRONTEND", "noninteractive")
}

/// The Zabbly apt source for a channel and suite.
pub fn zabbly_sources(channel: &str, suite: &str, arch: &str) -> String {
    let deb_arch = if arch == "aarch64" { "arm64" } else { "amd64" };
    format!(
        "# Written by errand-box setup.\nEnabled: yes\nTypes: deb\nURIs: https://pkgs.zabbly.com/incus/{channel}\nSuites: {suite}\nComponents: main\nArchitectures: {deb_arch}\nSigned-By: /etc/apt/keyrings/zabbly.asc\n"
    )
}

/// The client version from `incus version`.
pub fn client_version(out: &str) -> Option<String> {
    out.lines()
        .find_map(|l| l.strip_prefix("Client version:"))
        .map(|v| v.trim().to_owned())
}

/// True for Incus 6.0.0, which never uses idmapped mounts and rewrites every
/// new isolated instance (E-20).
pub fn lacks_idmap_fix(version: &str) -> bool {
    version.trim() == "6.0.0"
}

fn packages(cfg: &Config, host: &Host, runner: &dyn Runner, r: &mut Report) {
    let (channel, note) = host.incus_channel(&cfg.engine.incus);
    if let Some(n) = note {
        r.add("incus line", St::Degraded, n);
    } else {
        r.add("incus line", St::Ok, channel.clone());
    }
    match host.pkg.as_str() {
        "apt" => apt(host, &channel, runner, r),
        "apk" => {
            let out = runner.run(&long(Cmd::new("apk").args([
                "add",
                "--no-cache",
                "incus",
                "incus-client",
                "lxcfs",
                "btrfs-progs",
                "iproute2",
                "curl",
                "xz",
                "ca-certificates",
                "tar",
            ])));
            r.add(
                "packages",
                if out.ok() { St::Ok } else { St::Failed },
                if out.ok() {
                    "apk: incus, lxcfs, btrfs-progs".to_owned()
                } else {
                    format!(
                        "apk add: {} (Incus is in Alpine's community repository)",
                        out.why()
                    )
                },
            );
        }
        other => {
            if host::which("incus").is_some() {
                r.add(
                    "packages",
                    St::Ok,
                    format!("{other}: using the installed incus"),
                );
            } else {
                r.add("packages", St::Failed, format!("errand-box installs Incus with apt or apk; on this {other} host install incus, lxcfs and btrfs-progs first, then run setup again"));
            }
        }
    }
    let v = runner.run(&Cmd::new("incus").arg("version"));
    match client_version(&v.stdout) {
        Some(ver) if lacks_idmap_fix(&ver) => r.add("incus version", St::Degraded, format!("{ver}: never uses idmapped mounts, so each new isolated instance rewrites its rootfs (about 2-3 s, E-20); 6.0.1 and later fix it")),
        Some(ver) => r.add("incus version", St::Ok, ver),
        None => r.add("incus version", St::Failed, "incus is not installed or does not run"),
    }
}

fn apt(host: &Host, channel: &str, runner: &dyn Runner, r: &mut Report) {
    let upd = runner.run(&long(Cmd::new("apt-get").args(["-q", "update"])));
    if !upd.ok() {
        r.add("apt", St::Failed, format!("apt-get update: {}", upd.why()));
        return;
    }
    let base = runner.run(&long(Cmd::new("apt-get").args([
        "-q",
        "-y",
        "install",
        "curl",
        "ca-certificates",
        "gnupg",
        "xz-utils",
        "iproute2",
        "btrfs-progs",
        "lxcfs",
        "tar",
    ])));
    if !base.ok() {
        r.add(
            "apt",
            St::Failed,
            format!("apt-get install: {}", base.why()),
        );
        return;
    }
    if let Some(suite) = host.zabbly_suite() {
        let key = Path::new("/etc/apt/keyrings/zabbly.asc");
        if !key.exists() {
            let tmp = "/etc/apt/keyrings/zabbly.asc.part";
            let _ = fs::create_dir_all("/etc/apt/keyrings");
            let got = runner.run(&Cmd::new("curl").args(["-fsSL", "-o", tmp, ZABBLY_KEY_URL]));
            let fpr = runner.run(&Cmd::new("gpg").args(["--show-keys", "--with-colons", tmp]));
            let ok = got.ok()
                && fpr
                    .stdout
                    .lines()
                    .any(|l| l.starts_with("fpr:") && l.contains(ZABBLY_FPR));
            if !ok {
                let _ = fs::remove_file(tmp);
                r.add(
                    "zabbly key",
                    St::Failed,
                    format!("the key at {ZABBLY_KEY_URL} does not have fingerprint {ZABBLY_FPR}"),
                );
                return;
            }
            let _ = fs::rename(tmp, key);
            r.add(
                "zabbly key",
                St::Changed,
                format!("fingerprint {ZABBLY_FPR} checked"),
            );
        }
        let src =
            Path::new("/etc/apt/sources.list.d").join(format!("zabbly-incus-{channel}.sources"));
        // Only one Zabbly channel may be configured at a time.
        for other in ["stable", "lts-6.0"] {
            if other != channel {
                let _ = fs::remove_file(
                    Path::new("/etc/apt/sources.list.d")
                        .join(format!("zabbly-incus-{other}.sources")),
                );
            }
        }
        match up::write_if_changed(&src, &zabbly_sources(channel, suite, &host.arch), 0o644) {
            Ok(changed) => {
                if changed {
                    let _ = runner.run(&long(Cmd::new("apt-get").args(["-q", "update"])));
                }
                // apt-get update exits 0 when a fetch failed (E-14).
                let pol = runner.run(&Cmd::new("apt-cache").args(["policy", "incus"]));
                if !pol.stdout.contains("pkgs.zabbly.com") {
                    r.add("zabbly repository", St::Failed, "apt-cache policy shows no pkgs.zabbly.com origin for incus; apt-get update did not reach it (E-14)");
                    return;
                }
                r.add(
                    "zabbly repository",
                    if changed { St::Changed } else { St::Ok },
                    format!("incus/{channel} for {suite}"),
                );
            }
            Err(e) => {
                r.add("zabbly repository", St::Failed, e);
                return;
            }
        }
    } else {
        r.add(
            "zabbly repository",
            St::Skipped,
            format!(
                "no Zabbly build for {:?}; using the distribution's incus",
                host.os_codename
            ),
        );
    }
    let inc = runner.run(&long(
        Cmd::new("apt-get").args(["-q", "-y", "install", "incus"]),
    ));
    r.add(
        "packages",
        if inc.ok() { St::Ok } else { St::Failed },
        if inc.ok() {
            "incus, lxcfs, btrfs-progs".to_owned()
        } else {
            format!("apt-get install incus: {}", inc.why())
        },
    );
}

/// Append `line` to `file` unless it is there. Returns whether it changed.
pub fn ensure_line(file: &Path, line: &str) -> Result<bool, String> {
    let cur = fs::read_to_string(file).unwrap_or_default();
    if cur.lines().any(|l| l.trim() == line) {
        return Ok(false);
    }
    let mut new = cur;
    if !new.is_empty() && !new.ends_with('\n') {
        new.push('\n');
    }
    new.push_str(line);
    new.push('\n');
    fs::write(file, new).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    Ok(true)
}

/// True when `owner` has any range in a subordinate id file.
pub fn has_range(text: &str, owner: &str) -> bool {
    text.lines().any(|l| {
        l.split(':').next() == Some(owner)
            && l.split(':')
                .nth(2)
                .and_then(|n| n.trim().parse::<u64>().ok())
                .is_some_and(|n| n >= 65536)
    })
}

fn users(cfg: &Config, runner: &dyn Runner, r: &mut Report) -> Option<(u32, u32, bool)> {
    let u = &cfg.errand.user;
    if procs::account(u).is_none() {
        let out = if host::which("useradd").is_some() {
            runner.run(&Cmd::new("useradd").args([
                "--system",
                "--create-home",
                "--home-dir",
                &cfg.errand.home,
                "--shell",
                "/usr/sbin/nologin",
                u,
            ]))
        } else {
            runner.run(&Cmd::new("adduser").args([
                "-S",
                "-D",
                "-h",
                &cfg.errand.home,
                "-s",
                "/sbin/nologin",
                u,
            ]))
        };
        if !out.ok() {
            r.add(
                "errand account",
                St::Failed,
                format!("cannot create {u}: {}", out.why()),
            );
            return None;
        }
        r.add(
            "errand account",
            St::Changed,
            format!("created {u}, home {}", cfg.errand.home),
        );
    } else {
        r.add("errand account", St::Ok, u.clone());
    }
    let (uid, gid, home) = procs::account(u)?;
    if home != cfg.errand.home {
        r.add(
            "errand home",
            St::Failed,
            format!("{u} exists with home {home}, not {}", cfg.errand.home),
        );
        return None;
    }
    let groups = runner.run(&Cmd::new("id").args(["-nG", u]));
    if !groups.stdout.split_whitespace().any(|g| g == "incus") {
        let out = if host::which("usermod").is_some() {
            runner.run(&Cmd::new("usermod").args(["-aG", "incus", u]))
        } else {
            runner.run(&Cmd::new("addgroup").args([u.as_str(), "incus"]))
        };
        r.add(
            "incus group",
            if out.ok() { St::Changed } else { St::Failed },
            if out.ok() {
                format!("{u} reaches Incus through incus-user only")
            } else {
                out.why()
            },
        );
    } else {
        r.add(
            "incus group",
            St::Ok,
            format!("{u} is in group incus (not incus-admin)"),
        );
    }
    let mut changed = false;
    for (file, id) in [("/etc/subuid", uid), ("/etc/subgid", gid)] {
        let p = Path::new(file);
        let text = fs::read_to_string(p).unwrap_or_default();
        let mut c = false;
        if !has_range(&text, "root") {
            c |= ensure_line(p, "root:1000000:1000000000").unwrap_or(false);
        }
        // raw.idmap may map only host ids root is allowed to delegate.
        c |= ensure_line(p, &format!("root:{id}:1")).unwrap_or(false);
        changed |= c;
    }
    r.add(
        "subordinate ids",
        if changed { St::Changed } else { St::Ok },
        format!("root may map {u} ({uid}:{gid}) into its guests"),
    );
    for d in ["projects", "state"] {
        let p = Path::new(&cfg.errand.home).join(d);
        let _ = fs::create_dir_all(&p);
        let _ = runner.run(
            &Cmd::new("chown").args([format!("{uid}:{gid}"), p.to_string_lossy().into_owned()]),
        );
        let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o750));
    }
    Some((uid, gid, changed))
}

fn extract(runner: &dyn Runner, archive: &Path, into: &Path) -> Result<(), String> {
    fs::create_dir_all(into).map_err(|e| e.to_string())?;
    let out = runner.run(&Cmd::new("tar").args([
        "-xJf",
        &archive.to_string_lossy(),
        "-C",
        &into.to_string_lossy(),
    ]));
    if out.ok() {
        Ok(())
    } else {
        Err(format!("tar: {}", out.why()))
    }
}

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    for e in fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find_file(&p, name) {
                return Some(f);
            }
        } else if p.file_name().is_some_and(|n| n == name) {
            return Some(p);
        }
    }
    None
}

fn install_file(src: &Path, dst: &Path, mode: u32) -> Result<bool, String> {
    let same = fs::read(dst).ok().map(|b| crate::sha256::digest_hex(&b))
        == fs::read(src).ok().map(|b| crate::sha256::digest_hex(&b));
    if same && dst.exists() {
        return Ok(false);
    }
    if let Some(d) = dst.parent() {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = dst.with_extension("errand-box-tmp");
    fs::copy(src, &tmp)
        .map_err(|e| format!("cannot copy {} to {}: {e}", src.display(), tmp.display()))?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())?;
    fs::rename(&tmp, dst).map_err(|e| e.to_string())?;
    Ok(true)
}

fn link(target: &str, at: &Path) -> Result<bool, String> {
    if fs::read_link(at)
        .ok()
        .is_some_and(|t| t == Path::new(target))
    {
        return Ok(false);
    }
    if let Some(d) = at.parent() {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let _ = fs::remove_file(at);
    std::os::unix::fs::symlink(target, at)
        .map_err(|e| format!("cannot link {}: {e}", at.display()))?;
    Ok(true)
}

fn binaries(cfg: &Config, host: &Host, runner: &dyn Runner, r: &mut Report, cache: &Path) {
    let me = std::env::current_exe().unwrap_or_else(|_| PathBuf::from(SELF_BIN));
    let mut c = false;
    if me != Path::new(SELF_BIN) {
        match install_file(&me, Path::new(SELF_BIN), 0o755) {
            Ok(x) => c |= x,
            Err(e) => {
                r.add("errand-box", St::Failed, e);
                return;
            }
        }
    }
    c |= link("../bin/errand-box", &Path::new(SHIM_DIR).join("bailey")).unwrap_or(false);
    c |= link(SELF_BIN, Path::new("/usr/local/bin/errand-box")).unwrap_or(false);
    r.add(
        "errand-box",
        if c { St::Changed } else { St::Ok },
        format!("{SELF_BIN}; `bailey` for errand is {SHIM_DIR}/bailey"),
    );
    for (tool, version, bin) in [
        ("errand", &cfg.errand.version, "errand"),
        ("kage", &cfg.kage.version, "kage"),
    ] {
        let res = (|| -> Result<(String, bool), String> {
            let (v, a) = fetch::asset(tool, version, &host.arch)?;
            let archive = fetch::fetch(runner, &a, cache)?;
            let tmp = cache.join(format!("x-{tool}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&tmp);
            extract(runner, &archive, &tmp)?;
            let f =
                find_file(&tmp, bin).ok_or_else(|| format!("no {bin} in the {tool} archive"))?;
            let changed = install_file(&f, &Path::new(BIN_DIR).join(bin), 0o755)?;
            let _ = fs::remove_dir_all(&tmp);
            Ok((v, changed))
        })();
        match res {
            Ok((v, ch)) => r.add(
                tool,
                if ch { St::Changed } else { St::Ok },
                format!("{v} in {BIN_DIR}, digest matches pins.toml"),
            ),
            Err(e) => r.add(tool, St::Failed, e),
        }
    }
}

fn s6(host: &Host, runner: &dyn Runner, r: &mut Report) {
    if host.systemd {
        r.add(
            "s6",
            St::Skipped,
            "systemd is PID 1; its units supervise the daemons",
        );
        return;
    }
    if Path::new(&up::s6_bin("s6-svscan")).exists() {
        r.add(
            "s6",
            St::Ok,
            format!("{} (no init on this host, E-01)", up::s6_bin("s6-svscan")),
        );
        return;
    }
    let out = match host.pkg.as_str() {
        "apt" => runner.run(&long(
            Cmd::new("apt-get").args(["-q", "-y", "install", "s6"]),
        )),
        "apk" => runner.run(&long(Cmd::new("apk").args(["add", "--no-cache", "s6"]))),
        other => {
            r.add(
                "s6",
                St::Failed,
                format!(
                    "this host has no init; install the s6 package with {other} and run setup again"
                ),
            );
            return;
        }
    };
    if out.ok() && Path::new(&up::s6_bin("s6-svscan")).exists() {
        r.add(
            "s6",
            St::Changed,
            "installed the s6 package (no init on this host, E-01)",
        );
    } else {
        r.add(
            "s6",
            St::Failed,
            format!("cannot install s6: {}", out.why()),
        );
    }
}

fn errand_config(cfg: &Config, args: &Args, uid: u32, gid: u32, r: &mut Report) {
    let dst = Path::new(&cfg.errand.home).join(".config/errand/config.json");
    let (Some(envp), Some(credp)) = (&args.env, &args.creds) else {
        if dst.exists() {
            r.add(
                "errand config",
                St::Ok,
                format!(
                    "{} (kept; pass --env and --creds to rewrite it)",
                    dst.display()
                ),
            );
        } else {
            r.add("errand config", St::Skipped, "not written: pass --env errand-box.env --creds creds.env; errand stays stopped until then");
        }
        return;
    };
    let res = (|| -> Result<(bool, String), String> {
        let s = errandcfg::parse_env(
            &fs::read_to_string(envp).map_err(|e| format!("{}: {e}", envp.display()))?,
        )?;
        let c = errandcfg::parse_env(
            &fs::read_to_string(credp).map_err(|e| format!("{}: {e}", credp.display()))?,
        )?;
        let json = errandcfg::build(&s, &c, &cfg.errand.home)?;
        let text = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())? + "\n";
        let dir = dst.parent().unwrap();
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let changed = up::write_if_changed(&dst, &text, 0o600)?;
        for p in [dir.parent().unwrap(), dir, dst.as_path()] {
            // SAFETY: chown on paths we just created.
            let cp = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).unwrap();
            unsafe { libc::chown(cp.as_ptr(), uid, gid) };
        }
        Ok((changed, errandcfg::summary(&s, &c)))
    })();
    match res {
        Ok((ch, sum)) => r.add(
            "errand config",
            if ch { St::Changed } else { St::Ok },
            format!("{} mode 0600; {sum}", dst.display()),
        ),
        Err(e) => r.add("errand config", St::Failed, e),
    }
}

fn pool_size(cfg: &Config) -> String {
    if cfg.engine.pool_size != "auto" {
        return cfg.engine.pool_size.clone();
    }
    let free = statvfs_free("/var/lib/incus").unwrap_or(40 << 30);
    let gib = (free / 2) >> 30;
    format!("{}GiB", gib.max(20))
}

fn statvfs_free(p: &str) -> Option<u64> {
    let c = std::ffi::CString::new(p).ok()?;
    // SAFETY: statvfs fills a struct we own.
    unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut s) != 0 {
            return None;
        }
        Some(s.f_bavail as u64 * s.f_frsize as u64)
    }
}

/// True when a pool on a sparse loop file can grow past the free space of
/// the disk that holds the file.
fn loop_outgrows_host(total: u64, used: u64, host_free: u64) -> bool {
    total.saturating_sub(used) > host_free
}

fn pool(cfg: &Config, inc: &Incus, r: &mut Report) -> bool {
    let name = &cfg.engine.pool;
    if let Ok(v) = inc.query(&format!("/1.0/storage-pools/{name}")) {
        let source = v
            .pointer("/config/source")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let res = inc
            .query(&format!("/1.0/storage-pools/{name}/resources"))
            .ok();
        let space = |k: &str| {
            res.as_ref()
                .and_then(|x| x.pointer(&format!("/space/{k}")))
                .and_then(|x| x.as_u64())
        };
        let host_free = Path::new(source)
            .parent()
            .and_then(|d| statvfs_free(&d.to_string_lossy()));
        match (source.ends_with(".img"), space("total"), space("used"), host_free) {
            (true, Some(total), Some(used), Some(free)) if loop_outgrows_host(total, used, free) => r.add(
                "storage pool",
                St::Degraded,
                format!(
                    "{name}: its loop file can still grow {} GiB, and the host has {} GiB free; guests get I/O errors when the host disk fills (E-32)",
                    (total - used) >> 30,
                    free >> 30
                ),
            ),
            _ => r.add("storage pool", St::Ok, format!("{name} ({})", cfg.engine.pool_driver)),
        }
        return true;
    }
    let mut args = vec![
        "storage".to_owned(),
        "create".to_owned(),
        name.clone(),
        cfg.engine.pool_driver.clone(),
    ];
    if cfg.engine.pool_driver != "dir" {
        args.push(format!("size={}", pool_size(cfg)));
    }
    let out = inc.run(args);
    if !out.ok() {
        r.add("storage pool", St::Failed, out.why());
        return false;
    }
    if cfg.engine.pool_driver == "dir" {
        r.add(
            "storage pool",
            St::Degraded,
            format!("{name} is a dir pool: box.disk is accepted but not enforced (E-05)"),
        );
    } else {
        r.add(
            "storage pool",
            St::Changed,
            format!(
                "{name}: {} on a loop file of {}; thin, trimmed by maint (E-25)",
                cfg.engine.pool_driver,
                pool_size(cfg)
            ),
        );
    }
    true
}

fn project(
    cfg: &Config,
    host: &Host,
    uid: u32,
    gid: u32,
    runner: &dyn Runner,
    r: &mut Report,
    fp: &str,
) -> Option<String> {
    let project = format!("user-{uid}");
    let admin = Incus::new(Endpoint::default(), runner);
    if admin.query(&format!("/1.0/projects/{project}")).is_err() {
        // incus-user creates the project on the account's first call.
        let out = runner.run(&Cmd::new(SELF_BIN).args([
            "__as",
            &cfg.errand.user,
            "--",
            "incus",
            "project",
            "list",
        ]));
        if admin.query(&format!("/1.0/projects/{project}")).is_err() {
            r.add(
                "errand project",
                St::Failed,
                format!("incus-user did not create {project}: {}", out.why()),
            );
            return None;
        }
        r.add(
            "errand project",
            St::Changed,
            format!(
                "{project} created by incus-user: restricted, disks only under {}",
                cfg.errand.home
            ),
        );
    } else {
        r.add(
            "errand project",
            St::Ok,
            format!(
                "{project}: restricted, disks only under {}",
                cfg.errand.home
            ),
        );
    }
    let pinc = Incus::new(
        Endpoint {
            project: Some(project.clone()),
            ..Endpoint::default()
        },
        runner,
    );
    let pv = admin.query(&format!("/1.0/projects/{project}")).ok()?;
    if pv
        .pointer("/config/restricted.devices.proxy")
        .and_then(|v| v.as_str())
        != Some("allow")
    {
        let out = admin.run(["project", "set", &project, "restricted.devices.proxy=allow"]);
        r.add(
            "broker devices",
            if out.ok() { St::Changed } else { St::Failed },
            if out.ok() {
                "proxy devices allowed in the project, for the link to errand's broker".to_owned()
            } else {
                out.why()
            },
        );
    }
    let old = image::alias_target(&pinc, &cfg.image.alias);
    if old.as_deref() != Some(fp) {
        let dir = Path::new(paths::SYSTEM_STATE).join("images");
        let _ = fs::create_dir_all(&dir);
        let base = dir.join("guest");
        let base_s = base.to_string_lossy().into_owned();
        let ex = runner.run(
            &admin
                .cmd(["image", "export", &cfg.image.alias, &base_s])
                .timeout(Duration::from_secs(1800)),
        );
        let file = format!("{base_s}.tar.gz");
        let _ = pinc.run(["image", "alias", "delete", &cfg.image.alias]);
        let im = runner.run(
            &pinc
                .cmd(["image", "import", &file, "--alias", &cfg.image.alias])
                .timeout(Duration::from_secs(1800)),
        );
        let _ = fs::remove_file(&file);
        if !ex.ok() || !im.ok() {
            r.add(
                "project image",
                St::Failed,
                format!("export: {} import: {}", ex.why(), im.why()),
            );
            return None;
        }
        // The golden instance is remade from the new image below; copies
        // of the old one keep their own volumes.
        if let Some(o) = old {
            let _ = pinc.run(["image", "delete", &o]);
        }
        // A copy between projects of one server needs a network listener;
        // export and import do not (E-26).
        r.add(
            "project image",
            St::Changed,
            format!("{} imported into {project} (E-26)", cfg.image.alias),
        );
    } else {
        r.add(
            "project image",
            St::Ok,
            format!("{} {}", cfg.image.alias, &fp[..12.min(fp.len())]),
        );
    }
    let prof = pinc.query("/1.0/profiles/default").ok()?;
    let mut changed = Vec::new();
    if prof.pointer("/devices/eth0").is_some()
        && pinc
            .run(["profile", "device", "remove", "default", "eth0"])
            .ok()
    {
        changed.push("no network device".to_owned());
    }
    let root_pool = prof
        .pointer("/devices/root/pool")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let root_size = prof
        .pointer("/devices/root/size")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if root_pool != cfg.engine.pool || root_size != cfg.boxes.disk {
        let out = pinc.run([
            "profile",
            "device",
            "set",
            "default",
            "root",
            &format!("pool={}", cfg.engine.pool),
            &format!("size={}", cfg.boxes.disk),
        ]);
        if out.ok() {
            changed.push(format!("root {} on {}", cfg.boxes.disk, cfg.engine.pool));
        } else {
            r.add("project profile", St::Failed, out.why());
            return None;
        }
    }
    let want = [
        ("security.nesting", "true".to_owned()),
        ("security.idmap.isolated", "true".to_owned()),
        (
            "raw.idmap",
            format!("uid {uid} {GUEST_UID}\ngid {gid} {GUEST_UID}"),
        ),
        ("limits.memory", cfg.boxes.memory.clone()),
        ("limits.cpu", cfg.box_cpus(host.cpus).to_string()),
        // incusd restarts, after a host restart, every instance that was
        // running; a session instance must run only under its launcher.
        ("boot.autostart", "false".to_owned()),
    ];
    let mut kv = Vec::new();
    for (k, v) in want {
        if prof
            .pointer(&format!("/config/{}", k.replace('/', "~1")))
            .and_then(|x| x.as_str())
            != Some(v.as_str())
        {
            kv.push(format!("{k}={v}"));
        }
    }
    if !kv.is_empty() {
        let mut args = vec!["profile".to_owned(), "set".to_owned(), "default".to_owned()];
        args.extend(kv.iter().cloned());
        let out = pinc.run(args);
        if !out.ok() {
            r.add("project profile", St::Failed, out.why());
            return None;
        }
        changed.push(
            kv.iter()
                .map(|s| s.split('=').next().unwrap_or(""))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    if changed.is_empty() {
        r.add(
            "project profile",
            St::Ok,
            "no NIC, nesting, isolated uids, agent uid 1000 = errand, limits",
        );
    } else {
        r.add("project profile", St::Changed, changed.join("; "));
    }
    Some(project)
}

fn golden(cfg: &Config, project: &str, fp: &str, runner: &dyn Runner, r: &mut Report) -> bool {
    let pinc = Incus::new(
        Endpoint {
            project: Some(project.to_owned()),
            ..Endpoint::default()
        },
        runner,
    );
    match pinc.instance(GOLDEN) {
        Ok(Some(i)) if i.config.get(IMAGE_KEY).map(String::as_str) == Some(fp) => {
            r.add(
                "golden instance",
                St::Ok,
                format!("{GOLDEN}, never started, so copies need no rootfs rewrite"),
            );
            return true;
        }
        Ok(Some(_)) => {
            let _ = pinc.delete(GOLDEN);
        }
        Ok(None) => {}
        Err(e) => {
            r.add("golden instance", St::Failed, e);
            return false;
        }
    }
    match pinc.init(
        &cfg.image.alias,
        GOLDEN,
        &[(IMAGE_KEY.to_owned(), fp.to_owned())],
    ) {
        Ok(()) => {
            r.add(
                "golden instance",
                St::Changed,
                format!("{GOLDEN} from {} (never started)", cfg.image.alias),
            );
            true
        }
        Err(e) => {
            r.add("golden instance", St::Failed, e);
            false
        }
    }
}

pub fn setup(args: &Args, runner: &dyn Runner) -> Report {
    let mut r = Report::default();
    if paths::euid() != 0 {
        r.add("root", St::Failed, "errand-box setup needs root");
        return r;
    }
    let cfg = match crate::config::load(&args.config) {
        Ok(c) => c,
        Err(e) => {
            r.add("config", St::Failed, e);
            return r;
        }
    };
    if args.config != Path::new(crate::config::DEFAULT_PATH) {
        match fs::read_to_string(&args.config) {
            Ok(t) => {
                match up::write_if_changed(Path::new(crate::config::DEFAULT_PATH), &t, 0o644) {
                    Ok(ch) => r.add(
                        "config",
                        if ch { St::Changed } else { St::Ok },
                        format!(
                            "{} -> {}",
                            args.config.display(),
                            crate::config::DEFAULT_PATH
                        ),
                    ),
                    Err(e) => r.add("config", St::Failed, e),
                }
            }
            Err(_) => r.add("config", St::Ok, "defaults (no file given)"),
        }
    }
    if cfg.engine.mode != "root" {
        r.add("engine", St::Failed, "engine.mode = rootless is not set up by this command yet; docs/rootless.md has the measured procedure and TODO/engine.md the entry");
        return r;
    }
    let host = host::detect();
    r.add("host", St::Ok, host.describe());
    if !host.apparmor {
        r.add("apparmor", St::Degraded, "no AppArmor in this kernel: Incus loses the profile it adds; seccomp and isolated uid ranges remain (E-17)");
    }
    if !host.cap_sys_resource {
        r.add(
            "CAP_SYS_RESOURCE",
            St::Degraded,
            "root lacks it here; Incus does not need it (E-18)",
        );
    }
    let cache = Path::new(paths::SYSTEM_STATE).join("cache");
    packages(&cfg, &host, runner, &mut r);
    if r.failed() {
        return r;
    }
    let Some((uid, gid, subid_changed)) = users(&cfg, runner, &mut r) else {
        return r;
    };
    s6(&host, runner, &mut r);
    binaries(&cfg, &host, runner, &mut r, &cache);
    errand_config(&cfg, args, uid, gid, &mut r);
    if r.failed() {
        return r;
    }
    up::up(&cfg, &host, runner, &mut r);
    if subid_changed {
        // incusd reads /etc/subuid when it starts.
        restart_incusd(&host, runner, &mut r);
    }
    if r.failed() {
        return r;
    }
    let admin = Incus::new(Endpoint::default(), runner);
    if !pool(&cfg, &admin, &mut r) {
        return r;
    }
    let want_inputs = match fetch::asset("kage", &cfg.kage.version, &host.arch) {
        Ok((kv, _)) => image::inputs_digest(&cfg, &kv),
        Err(e) => {
            r.add("guest image", St::Failed, e);
            return r;
        }
    };
    let have = image::alias_target(&admin, &cfg.image.alias);
    let current = have
        .as_deref()
        .and_then(|fp| image::recorded_inputs(&admin, fp))
        .as_deref()
        == Some(want_inputs.as_str());
    match (have.is_some(), current, args.skip_image) {
        (_, true, _) => r.add(
            "guest image",
            St::Ok,
            format!(
                "{} is current; `errand-box image build` rebuilds it",
                cfg.image.alias
            ),
        ),
        (false, _, true) => {
            r.add(
                "guest image",
                St::Failed,
                format!("{} is missing and --skip-image was given", cfg.image.alias),
            );
            return r;
        }
        (true, false, true) => r.add(
            "guest image",
            St::Degraded,
            format!(
                "{} was built from other inputs (E-31); --skip-image keeps it",
                cfg.image.alias
            ),
        ),
        _ => {
            if have.is_some() {
                r.add(
                    "guest image",
                    St::Changed,
                    format!(
                        "{} was built from other inputs; rebuilding (E-31)",
                        cfg.image.alias
                    ),
                );
            }
            if let Err(e) = image::build(&cfg, &host, runner, &mut r, &cache) {
                r.add("guest image", St::Failed, e);
                return r;
            }
        }
    }
    let Some(fp) = image::alias_target(&admin, &cfg.image.alias) else {
        r.add("guest image", St::Failed, "the image has no fingerprint");
        return r;
    };
    let Some(project) = project(&cfg, &host, uid, gid, runner, &mut r, &fp) else {
        return r;
    };
    if !golden(&cfg, &project, &fp, runner, &mut r) {
        return r;
    }
    let rt = Runtime {
        mode: "root".into(),
        incus_bin: "incus".into(),
        incus_dir: String::new(),
        // Named, because the shim's own client directory has no default
        // project; incus-user allows the account this one only.
        project: project.clone(),
        golden: GOLDEN.into(),
        guest_uid: GUEST_UID,
        guest_gid: GUEST_UID,
        stop_seconds: cfg.boxes.stop_seconds,
        guest_proxy_port: cfg.network.guest_proxy_port,
        broker_link: "tcp".into(),
        host_runtime_dirs: vec![BIN_DIR.into()],
        admin_project: project,
        state_root: format!("{}/state", cfg.errand.home),
    };
    match up::write_if_changed(&runtime::path(), &runtime::render(&rt), 0o644) {
        Ok(ch) => r.add(
            "runtime.toml",
            if ch { St::Changed } else { St::Ok },
            runtime::path().display().to_string(),
        ),
        Err(e) => {
            r.add("runtime.toml", St::Failed, e);
            return r;
        }
    }
    // errand starts only now that its sandbox is complete.
    let mut r2 = Report::default();
    up::up(&cfg, &host, runner, &mut r2);
    if let Some(row) = r2.rows.iter().find(|x| x.0 == "errand") {
        r.rows.push(row.clone());
    }
    let doc = runner.run(&Cmd::new(SELF_BIN).args([
        "__as",
        &cfg.errand.user,
        "--",
        &format!("{SHIM_DIR}/bailey"),
        "doctor",
    ]));
    if doc.ok() {
        r.add(
            "bailey doctor as errand",
            St::Ok,
            doc.stdout
                .lines()
                .filter(|l| l.contains("incus:") || l.contains("golden"))
                .map(str::trim)
                .collect::<Vec<_>>()
                .join("; "),
        );
    } else {
        r.add(
            "bailey doctor as errand",
            St::Failed,
            format!("{}{}", doc.stdout, doc.stderr).trim().to_owned(),
        );
    }
    if !Path::new(ERRAND_BIN).exists() {
        r.add("errand", St::Failed, format!("{ERRAND_BIN} is missing"));
    }
    r
}

fn restart_incusd(host: &Host, runner: &dyn Runner, r: &mut Report) {
    let out = if host.systemd {
        runner.run(&Cmd::new("systemctl").args(["restart", "incus.service"]))
    } else {
        // s6-supervise sends SIGTERM to the pid it started (E-12); incusd
        // leaves its guests running on SIGTERM.
        runner
            .run(&Cmd::new(&up::s6_bin("s6-svc")).args(["-t", &format!("{}/incusd", up::SCAN_DIR)]))
    };
    let ok = out.ok() && up::wait_incus(runner, Duration::from_secs(90)).is_ok();
    r.add(
        "incusd restart",
        if ok { St::Changed } else { St::Failed },
        if ok {
            "re-read /etc/subuid".to_owned()
        } else {
            out.why()
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zabbly_source_names_channel_suite_and_arch() {
        let s = zabbly_sources("lts-6.0", "noble", "x86_64");
        assert!(s.contains("URIs: https://pkgs.zabbly.com/incus/lts-6.0\n"));
        assert!(s.contains("Suites: noble\n"));
        assert!(s.contains("Architectures: amd64\n"));
        assert!(zabbly_sources("stable", "trixie", "aarch64").contains("arm64"));
    }

    #[test]
    fn a_loop_pool_larger_than_the_free_disk_is_reported() {
        let g = 1u64 << 30;
        assert!(loop_outgrows_host(20 * g, 3 * g, 14 * g));
        assert!(!loop_outgrows_host(20 * g, 3 * g, 17 * g));
        assert!(!loop_outgrows_host(20 * g, 25 * g, 0));
    }

    #[test]
    fn versions() {
        assert_eq!(
            client_version("Client version: 6.0.6\nServer version: 6.0.6\n").as_deref(),
            Some("6.0.6")
        );
        assert!(lacks_idmap_fix("6.0.0"));
        assert!(!lacks_idmap_fix("6.0.6"));
    }

    #[test]
    fn subordinate_ranges() {
        assert!(has_range("root:1000000:1000000000\n", "root"));
        assert!(!has_range("root:994:1\nubuntu:100000:65536\n", "root"));
        let p = std::env::temp_dir().join(format!("eb-subuid-{}", std::process::id()));
        std::fs::write(&p, "ubuntu:100000:65536").unwrap();
        assert!(ensure_line(&p, "root:994:1").unwrap());
        assert!(!ensure_line(&p, "root:994:1").unwrap());
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "ubuntu:100000:65536\nroot:994:1\n"
        );
    }
}
