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
    /// Print each step to stderr as it finishes.
    pub live: bool,
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

/// True when a `gpg --with-colons` listing holds exactly one primary key
/// and its fingerprint is `want`: a keyring with extra keys is refused.
pub fn key_is_only(colons: &str, want: &str) -> bool {
    let mut primaries = Vec::new();
    let mut after_pub = false;
    for l in colons.lines() {
        if l.starts_with("pub:") {
            after_pub = true;
        } else if l.starts_with("fpr:") && after_pub {
            if let Some(fp) = l.split(':').nth(9) {
                primaries.push(fp.to_owned());
            }
            after_pub = false;
        } else if l.starts_with("sub:") {
            after_pub = false;
        }
    }
    primaries == [want]
}

/// The Zabbly apt source for a channel and suite.
pub fn zabbly_sources(channel: &str, suite: &str, arch: &str) -> String {
    let deb_arch = if arch == "aarch64" { "arm64" } else { "amd64" };
    format!(
        "# Written by errand-box setup.\nEnabled: yes\nTypes: deb\nURIs: https://pkgs.zabbly.com/incus/{channel}\nSuites: {suite}\nComponents: main\nArchitectures: {deb_arch}\nSigned-By: /etc/apt/keyrings/zabbly.asc\n"
    )
}

/// The Zabbly channel already configured in `/etc/apt/sources.list.d`, if
/// any. setup keeps it when the operator asked for `auto` and it runs on
/// this host, so a second setup does not move an existing install from one
/// Incus line to another (E-64).
pub fn configured_zabbly_channel() -> Option<String> {
    for ch in ["stable", "lts-6.0"] {
        let p = format!("/etc/apt/sources.list.d/zabbly-incus-{ch}.sources");
        if Path::new(&p).exists() {
            return Some(ch.to_owned());
        }
    }
    None
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
    let (mut channel, mut note) = host.incus_channel(&cfg.engine.incus);
    // Keep an already-configured channel on `auto`, unless this host cannot
    // run it (a v1 host and the `stable` line); switching it is an upgrade
    // or a downgrade the operator did not ask for (E-64).
    if cfg.engine.incus == "auto"
        && let Some(cur) = configured_zabbly_channel()
        && cur != channel
        && !(cur == "stable" && !host.cgroup_v2)
    {
        note = Some(format!(
            "keeping the {cur} line already configured here, not moving to {channel} (E-64)"
        ));
        channel = cur;
    }
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
            let have = !crate::layout::detect().flavor.is_empty();
            let st = match (out.ok(), out.stdout.contains("Installing "), have) {
                (true, false, _) => St::Ok,
                (true, true, _) => St::Changed,
                // A refresh is not needed to change the configuration (E-45).
                (false, _, true) => St::Degraded,
                (false, _, false) => St::Failed,
            };
            r.add(
                "packages",
                st,
                if out.ok() {
                    "apk: incus, lxcfs, btrfs-progs".to_owned()
                } else if have {
                    format!("apk add: {}; using the installed Incus", out.why())
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
    // Incus 7 and later refuse cgroup v1 (E-04): an installed one cannot run
    // here, whatever installed it.
    for (what, ver) in [
        ("client", client_version(&v.stdout)),
        (
            "server",
            v.stdout
                .lines()
                .find_map(|l| l.strip_prefix("Server version:"))
                .map(|x| x.trim().to_owned()),
        ),
    ] {
        let major = ver
            .as_deref()
            .and_then(|x| x.split('.').next()?.parse::<u32>().ok());
        if !host.cgroup_v2 && major.is_some_and(|m| m >= 7) {
            r.add(
                "incus version",
                St::Failed,
                format!("the {what} is Incus {}, which refuses cgroup v1, and this host has cgroup v1; install the 6.0 LTS line (E-04)", ver.unwrap_or_default()),
            );
            return;
        }
    }
    match client_version(&v.stdout) {
        Some(ver) if lacks_idmap_fix(&ver) => r.add("incus version", St::Degraded, format!("{ver}: never uses idmapped mounts, so each new isolated instance rewrites its rootfs (about 2-3 s, E-20); 6.0.1 and later fix it")),
        Some(ver) => r.add("incus version", St::Ok, ver),
        None => r.add("incus version", St::Failed, "incus is not installed or does not run"),
    }
}

/// The count apt prints as `N newly installed`.
pub fn apt_installed(stdout: &str) -> u32 {
    stdout
        .lines()
        .find_map(|l| {
            let (head, _) = l.split_once(" newly installed")?;
            head.rsplit(", ").next()?.trim().parse().ok()
        })
        .unwrap_or(0)
}

/// Why apt cannot reach an `http://` source through a proxy that only
/// tunnels (CONNECT), when that is the likely cause (E-44).
fn apt_http_hint() -> Option<String> {
    let proxied = ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"]
        .iter()
        .any(|k| crate::run::env(k).is_some());
    let mut files = vec![PathBuf::from("/etc/apt/sources.list")];
    if let Ok(d) = fs::read_dir("/etc/apt/sources.list.d") {
        files.extend(d.flatten().map(|e| e.path()));
    }
    let http = files.iter().any(|f| {
        fs::read_to_string(f)
            .unwrap_or_default()
            .lines()
            .any(|l| !l.trim_start().starts_with('#') && l.contains("http://"))
    });
    (proxied && http).then(|| "the apt sources use http://, and a proxy that only tunnels (CONNECT) cannot carry it; switch them to https:// (E-44)".to_owned())
}

fn apt(host: &Host, channel: &str, runner: &dyn Runner, r: &mut Report) {
    let upd = runner.run(&long(Cmd::new("apt-get").args(["-q", "update"])));
    if !upd.ok() {
        let mut why = format!("apt-get update: {}", upd.why());
        if let Some(h) = apt_http_hint() {
            why = format!("{why}; {h}");
        }
        // With Incus already installed, a package refresh is not needed to
        // change the configuration or the credentials (E-45).
        if !crate::layout::detect().flavor.is_empty() && host::which("incus").is_some() {
            r.add(
                "apt",
                St::Degraded,
                format!("{why}; using the installed Incus"),
            );
            return;
        }
        r.add("apt", St::Failed, why);
        return;
    }
    // --no-upgrade: a second setup must not upgrade incus under running
    // sessions; upgrades are the operator's (docs/operations.md).
    let base = runner.run(&long(Cmd::new("apt-get").args([
        "-q",
        "-y",
        "--no-upgrade",
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
    // A suite missing from the compiled-in list (a release newer than this
    // program) is used when Zabbly's repository has it (E-52).
    let probed;
    let suite = match host.zabbly_suite() {
        Some(s) => Some(s),
        None => {
            probed = host.suite_candidates().into_iter().find(|c| {
                runner
                    .run(&Cmd::new("curl").args([
                        "-fsI",
                        "-o",
                        "/dev/null",
                        &format!("https://pkgs.zabbly.com/incus/{channel}/dists/{c}/Release"),
                    ]))
                    .ok()
            });
            probed
        }
    };
    if let Some(suite) = suite {
        let key = Path::new("/etc/apt/keyrings/zabbly.asc");
        if !key.exists() {
            let tmp = "/etc/apt/keyrings/zabbly.asc.part";
            let _ = fs::create_dir_all("/etc/apt/keyrings");
            let got = runner.run(&Cmd::new("curl").args(["-fsSL", "-o", tmp, ZABBLY_KEY_URL]));
            let fpr = runner.run(&Cmd::new("gpg").args(["--show-keys", "--with-colons", tmp]));
            // Exactly the one key, not just a keyring that contains it among
            // others (E-55).
            let ok = got.ok() && key_is_only(&fpr.stdout, ZABBLY_FPR);
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
    let inc = runner.run(&long(Cmd::new("apt-get").args([
        "-q",
        "-y",
        "--no-upgrade",
        "install",
        "incus",
    ])));
    let installed = apt_installed(&base.stdout) + apt_installed(&inc.stdout);
    r.add(
        "packages",
        match (inc.ok(), installed) {
            (false, _) => St::Failed,
            (true, 0) => St::Ok,
            (true, _) => St::Changed,
        },
        if inc.ok() {
            format!("incus, lxcfs, btrfs-progs ({installed} newly installed)")
        } else {
            format!("apt-get install incus: {}", inc.why())
        },
    );
}

/// Create `path` as a directory owned by `uid:gid` with mode `mode`,
/// refusing to follow a symlink. setup runs as root and `path` is under
/// errand's own home, which the errand account owns and could fill with a
/// symlink to a root-owned file; chown must not follow it (E-54).
pub fn safe_dir(path: &Path, uid: u32, gid: u32, mode: u32) -> Result<(), String> {
    if let Ok(md) = fs::symlink_metadata(path)
        && md.file_type().is_symlink()
    {
        fs::remove_file(path)
            .map_err(|e| format!("cannot remove the symlink {}: {e}", path.display()))?;
    }
    fs::create_dir_all(path).map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    // lchown, not chown: path is now a real directory, and nothing we would
    // follow replaced it between the check and here.
    std::os::unix::fs::lchown(path, Some(uid), Some(gid))
        .map_err(|e| format!("cannot chown {}: {e}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|e| format!("cannot set the mode of {}: {e}", path.display()))?;
    Ok(())
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

/// How many isolated instances root's subordinate ids hold: each takes a
/// block of 65536, and the first block is the host's own.
pub fn isolated_capacity(text: &str) -> u64 {
    let total: u64 = text
        .lines()
        .filter(|l| l.split(':').next() == Some("root"))
        .filter_map(|l| l.split(':').nth(2)?.trim().parse::<u64>().ok())
        .sum();
    (total / 65536).saturating_sub(1)
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
    for g in [up::ADMIN_GROUP, up::USER_GROUP] {
        match up::ensure_group(g, runner) {
            Ok(true) => r.add("groups", St::Changed, format!("created {g} (E-35)")),
            Ok(false) => {}
            Err(e) => {
                r.add("groups", St::Failed, e);
                return None;
            }
        }
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
    let cap = isolated_capacity(&fs::read_to_string("/etc/subuid").unwrap_or_default());
    r.add(
        "subordinate ids",
        match (cap < 64, changed) {
            (true, _) => St::Degraded,
            (false, true) => St::Changed,
            (false, false) => St::Ok,
        },
        if cap < 64 {
            format!("root's ranges in /etc/subuid hold {cap} isolated instances; every session instance, stopped ones too, needs its own block of 65536: give root a larger range, such as root:1000000:1000000000 (E-53)")
        } else {
            format!("root may map {u} ({uid}:{gid}) into its guests; room for {cap} instances")
        },
    );
    for d in ["projects", "state"] {
        if let Err(e) = safe_dir(&Path::new(&cfg.errand.home).join(d), uid, gid, 0o750) {
            r.add("errand home", St::Failed, e);
            return None;
        }
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
    // An earlier bootstrap.sh installed a copy in /usr/local/sbin, which
    // comes first on root's PATH and would shadow every later version.
    let old = Path::new("/usr/local/sbin/errand-box");
    if old.exists() {
        c |= link(SELF_BIN, old).unwrap_or(false);
    }
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

/// OpenRC runs every executable `/etc/local.d/*.start` at boot, through
/// its `local` service.
pub const LOCAL_START: &str = "/etc/local.d/errand-box.start";
/// The OpenRC services of Alpine's incus and lxcfs packages. errand-box's
/// s6 runs these daemons; a second incusd on the same socket fails (E-36).
const OPENRC_TWINS: &[&str] = &["incusd", "incus-user", "lxcfs"];

/// The boot step: systemd starts its units; OpenRC gets a `local` script
/// that runs `errand-box up`; a host with no init gets nothing and is
/// reported (E-02, E-36).
fn boot_hook(host: &Host, runner: &dyn Runner, r: &mut Report) {
    if host.systemd {
        return;
    }
    if !host.openrc {
        r.add(
            "boot",
            St::Degraded,
            "nothing on this host runs errand-box at boot; run `errand-box up` after every restart (E-02)",
        );
        return;
    }
    let text = format!(
        "#!/bin/sh\n# Written by errand-box setup: bring the daemons back at boot (E-36).\nmkdir -p {log}\n{SELF_BIN} up > {log}/boot.log 2>&1 &\n",
        log = up::LOG_DIR
    );
    let mut notes = Vec::new();
    let mut st = St::Ok;
    match up::write_if_changed(Path::new(LOCAL_START), &text, 0o755) {
        Ok(true) => {
            st = St::Changed;
            notes.push(format!("wrote {LOCAL_START}"));
        }
        Ok(false) => {}
        Err(e) => {
            r.add("boot", St::Failed, e);
            return;
        }
    }
    if !runlevels_with("local").iter().any(|l| l == "default") {
        let out = runner.run(&Cmd::new("rc-update").args(["add", "local", "default"]));
        if !out.ok() {
            r.add(
                "boot",
                St::Failed,
                format!("rc-update add local: {}", out.why()),
            );
            return;
        }
        st = St::Changed;
        notes.push("added local to the default runlevel".into());
    }
    for svc in OPENRC_TWINS {
        let levels = runlevels_with(svc);
        if levels.is_empty() {
            continue;
        }
        let _ = runner.run(&Cmd::new("rc-service").args([*svc, "stop"]));
        st = St::Changed;
        for lv in levels {
            let out = runner.run(&Cmd::new("rc-update").args(["del", svc, &lv]));
            notes.push(if out.ok() {
                format!("removed OpenRC's {svc} from {lv}: s6 runs it")
            } else {
                format!("cannot remove OpenRC's {svc} from {lv}: {}", out.why())
            });
        }
    }
    if notes.is_empty() {
        notes.push(format!("{LOCAL_START} runs `errand-box up`"));
    }
    r.add("boot", st, format!("openrc: {}", notes.join("; ")));
}

/// The OpenRC runlevels that start `svc`.
fn runlevels_with(svc: &str) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir("/etc/runlevels")
        .map(|d| {
            d.flatten()
                .filter(|lv| lv.path().join(svc).exists())
                .map(|lv| lv.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
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
        // The .config and .config/errand directories, made safe so a
        // planted symlink cannot redirect root's chown (E-54).
        safe_dir(dir.parent().unwrap(), uid, gid, 0o700)?;
        safe_dir(dir, uid, gid, 0o700)?;
        if fs::symlink_metadata(&dst).is_ok_and(|m| m.file_type().is_symlink()) {
            fs::remove_file(&dst).map_err(|e| e.to_string())?;
        }
        let changed = up::write_if_changed(&dst, &text, 0o600)?;
        std::os::unix::fs::lchown(&dst, Some(uid), Some(gid)).map_err(|e| e.to_string())?;
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
    // At least three sessions' quota: a pool the size of one session's
    // quota is full as soon as one session fills its disk.
    let disk = crate::config::incus_bytes(&cfg.boxes.disk).unwrap_or(20 << 30);
    let floor = (disk.saturating_mul(3) >> 30).max(20);
    format!("{}GiB", ((free / 2) >> 30).max(floor))
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
        // The broker link needs proxy devices. Allowing them also lets the
        // errand account point one at any host socket, which is more than
        // the restricted project otherwise grants it (E-61, T-020).
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
        // A copy between projects of one server needs a network listener;
        // export and import do not (E-26). The project alias moves only
        // once the image is in the project, so a failure keeps the old one.
        if pinc.query(&format!("/1.0/images/{fp}")).is_err()
            && let Err(e) = copy_image_into(&admin, &pinc, &cfg.image.alias)
        {
            r.add("project image", St::Failed, e);
            return None;
        }
        if let Err(e) = pinc.move_alias(&cfg.image.alias, fp) {
            r.add("project image", St::Failed, e);
            return None;
        }
        // The golden instance is remade from the new image below; copies
        // of the old one keep their own volumes.
        if let Some(o) = old {
            let _ = pinc.run(["image", "delete", &o]);
        }
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
        // incus-user gives the project's profile a root disk; one that has
        // none gets it added rather than set.
        let verb = if prof.pointer("/devices/root").is_some() {
            ["profile", "device", "set", "default", "root"].to_vec()
        } else {
            [
                "profile", "device", "add", "default", "root", "disk", "path=/",
            ]
            .to_vec()
        };
        let mut args: Vec<String> = verb.into_iter().map(str::to_owned).collect();
        args.push(format!("pool={}", cfg.engine.pool));
        args.push(format!("size={}", cfg.boxes.disk));
        let out = pinc.run(args);
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
        (
            "limits.processes",
            if cfg.boxes.pids == 0 {
                String::new()
            } else {
                cfg.boxes.pids.to_string()
            },
        ),
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

/// Export `alias` from the default project and import it into `pinc`'s.
/// The export's file names follow the server's compression and the image's
/// shape (one file, or metadata and rootfs), so they are read back from a
/// directory of their own.
fn copy_image_into(admin: &Incus, pinc: &Incus, alias: &str) -> Result<(), String> {
    let dir = Path::new(paths::SYSTEM_STATE)
        .join("images")
        .join(format!("export-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let res = (|| {
        let base = dir.join("guest").to_string_lossy().into_owned();
        let ex = admin.runner.run(
            &admin
                .cmd(["image", "export", alias, &base])
                .timeout(Duration::from_secs(1800)),
        );
        if !ex.ok() {
            return Err(format!("export: {}", ex.why()));
        }
        let mut files: Vec<String> = fs::read_dir(&dir)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path().to_string_lossy().into_owned())
            .collect();
        // The metadata tarball sorts before the rootfs (guest.tar.* < guest.*).
        files.sort_by_key(|f| (!f.contains(".tar"), f.clone()));
        if files.is_empty() || files.len() > 2 {
            return Err(format!(
                "export wrote {} files to {}",
                files.len(),
                dir.display()
            ));
        }
        let mut args = vec!["image".to_owned(), "import".to_owned()];
        args.extend(files);
        let im = pinc
            .runner
            .run(&pinc.cmd(args).timeout(Duration::from_secs(1800)));
        if im.ok() {
            Ok(())
        } else {
            Err(format!("import: {}", im.why()))
        }
    })();
    let _ = fs::remove_dir_all(&dir);
    res
}

/// The name a new golden instance is made under before it replaces the old
/// one, so sessions always have one to copy.
pub const GOLDEN_NEXT: &str = "eb-golden-next";

fn golden(cfg: &Config, project: &str, fp: &str, runner: &dyn Runner, r: &mut Report) -> bool {
    let pinc = Incus::new(
        Endpoint {
            project: Some(project.to_owned()),
            ..Endpoint::default()
        },
        runner,
    );
    let had = match pinc.instance(GOLDEN) {
        Ok(Some(i)) if i.config.get(IMAGE_KEY).map(String::as_str) == Some(fp) => {
            r.add(
                "golden instance",
                St::Ok,
                format!("{GOLDEN}, never started, so copies need no rootfs rewrite"),
            );
            return true;
        }
        Ok(found) => found.is_some(),
        Err(e) => {
            r.add("golden instance", St::Failed, e);
            return false;
        }
    };
    let _ = pinc.delete(GOLDEN_NEXT);
    let made = pinc
        .init(
            &cfg.image.alias,
            GOLDEN_NEXT,
            &[(IMAGE_KEY.to_owned(), fp.to_owned())],
        )
        .and_then(|()| if had { pinc.delete(GOLDEN) } else { Ok(()) })
        .and_then(|()| pinc.rename(GOLDEN_NEXT, GOLDEN));
    match made {
        Ok(()) => {
            r.add(
                "golden instance",
                St::Changed,
                format!("{GOLDEN} from {} (never started)", cfg.image.alias),
            );
            true
        }
        Err(e) => {
            let _ = pinc.delete(GOLDEN_NEXT);
            let kept = matches!(pinc.instance(GOLDEN), Ok(Some(_)));
            r.add(
                "golden instance",
                St::Failed,
                if kept {
                    format!("{e}; the old {GOLDEN} is kept, so sessions still start")
                } else {
                    e
                },
            );
            false
        }
    }
}

pub fn setup(args: &Args, runner: &dyn Runner) -> Report {
    let mut r = Report {
        live: args.live,
        ..Report::default()
    };
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
    // What `up` falls back to if the live file is broken later (E-42).
    let applied = fs::read_to_string(&args.config).unwrap_or_default();
    let _ = fs::create_dir_all(paths::SYSTEM_STATE);
    if let Err(e) = up::write_if_changed(Path::new(crate::config::APPLIED_PATH), &applied, 0o644) {
        r.add("config", St::Degraded, format!("cannot keep a copy: {e}"));
    }
    if cfg.engine.mode != "root" {
        r.add("engine", St::Failed, "engine.mode = rootless is not set up by this command yet; docs/research/rootless.md has the measured procedure and TODO/setup.md entry T-010 the plan");
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
    boot_hook(&host, runner, &mut r);
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
    if cfg.image.source == "release" {
        r.add(
            "guest image",
            St::Failed,
            "image.source = \"release\" needs a release that publishes the image, and none does yet (T-014); use \"build\", or `errand-box image import DIR` with a distrobuilder build",
        );
        return r;
    }
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
    // The errand account's own state directory, so its launcher can create
    // locks there; root's reap locks the same files.
    let lock_dir = format!("{}/.local/state/errand-box/locks", cfg.errand.home);
    let mut d = PathBuf::from(&cfg.errand.home);
    for part in [".local", "state", "errand-box", "locks"] {
        d.push(part);
        // Symlink-safe: the chain is under errand's own home (E-54).
        if let Err(e) = safe_dir(&d, uid, gid, 0o700) {
            r.add("locks", St::Degraded, e);
            break;
        }
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
        lock_dir: lock_dir.clone(),
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
        // The first `up` ran before the sandbox existed; this row replaces
        // its errand row.
        r.rows.retain(|x| x.0 != "errand");
        let (step, st, d) = row.clone();
        r.add(&step, st, d);
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
    let dir = format!("{}/incusd", up::SCAN_DIR);
    let svstat = || {
        runner
            .run(&Cmd::new(&up::s6_bin("s6-svstat")).arg(&dir))
            .stdout
    };
    let before = crate::supervise::svstat_pid(&svstat());
    let out = if host.systemd {
        runner.run(&Cmd::new("systemctl").args(["restart", "incus.service"]))
    } else {
        // s6-supervise sends SIGTERM to the pid it started (E-12); incusd
        // leaves its guests running on SIGTERM.
        runner.run(&Cmd::new(&up::s6_bin("s6-svc")).args(["-t", &dir]))
    };
    // The old incusd answers until it has stopped: wait for the new pid
    // first, or the check below can pass on the daemon that is going away.
    if !host.systemd && before.is_some() {
        let t = std::time::Instant::now();
        while crate::supervise::svstat_pid(&svstat()) == before
            && t.elapsed() < Duration::from_secs(60)
        {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    let ok = out.ok() && up::wait_incus(runner, Duration::from_secs(180)).is_ok();
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
    fn a_keyring_with_extra_keys_is_refused() {
        let one = format!(
            "pub:-:255:22:...:...::::::::::\nfpr:::::::::{ZABBLY_FPR}:\nsub:-:255:18:...\nfpr:::::::::DEADBEEF:\n"
        );
        assert!(key_is_only(&one, ZABBLY_FPR));
        let other = "0".repeat(40);
        let two = format!("{one}pub:-:255:22:...\nfpr:::::::::{other}:\n");
        assert!(!key_is_only(&two, ZABBLY_FPR));
        assert!(!key_is_only("", ZABBLY_FPR));
    }

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
    fn apt_counts_what_it_installed() {
        assert_eq!(
            apt_installed("0 upgraded, 3 newly installed, 0 to remove and 5 not upgraded.\n"),
            3
        );
        assert_eq!(
            apt_installed("0 upgraded, 0 newly installed, 0 to remove and 0 not upgraded.\n"),
            0
        );
        assert_eq!(apt_installed("incus is already the newest version\n"), 0);
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
    fn safe_dir_replaces_a_symlink_and_leaves_its_target() {
        let base = std::env::temp_dir().join(format!("eb-safedir-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let target = base.join("target");
        fs::write(&target, "x").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        let link = base.join("projects");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let (uid, gid) = (paths::euid(), unsafe { libc::getegid() });
        safe_dir(&link, uid, gid, 0o750).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().is_dir());
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o644
        );
        assert_eq!(
            fs::metadata(&link).unwrap().permissions().mode() & 0o777,
            0o750
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn isolated_capacity_counts_root_blocks() {
        assert_eq!(isolated_capacity("root:1000000:1000000000\n"), 15257);
        assert_eq!(isolated_capacity("root:100000:65536\nroot:994:1\n"), 0);
        assert_eq!(isolated_capacity("ubuntu:100000:65536000\n"), 0);
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
