//! The operator's day-to-day commands: `logs`, `shell`, `image import` and
//! `uninstall`. Each says what it does before it does anything that cannot
//! be undone.

use crate::config::Config;
use crate::host::Host;
use crate::incus::{Endpoint, Incus};
use crate::report::{Report, St};
use crate::run::{Cmd, Runner};
use crate::{image, paths, session, setup, up};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Replace this process with `c`, on the terminal. Returns only on failure.
fn exec(c: &Cmd) -> i32 {
    let mut p = std::process::Command::new(&c.program);
    p.args(&c.args).envs(&c.env);
    let e = p.exec();
    eprintln!("errand-box: cannot run {}: {e}", c.program);
    1
}

/// The systemd units behind a daemon's s6 name.
pub fn units_of(name: &str) -> &'static [&'static str] {
    match name {
        "incusd" | "incus" => &["incus.service"],
        "incus-user" => &["incus-user.service"],
        "lxcfs" => &[
            "incus-lxcfs.service",
            "errand-box-lxcfs.service",
            "lxcfs.service",
        ],
        "errand" => &["errand-box-errand.service"],
        "maint" | "errand-box-maint" => &["errand-box-maint.service"],
        _ => &[],
    }
}

/// `errand-box logs [NAME] [-f] [-n N]`: a daemon's log, from s6-log's
/// files or the journal. With no name, the last line of each.
pub fn logs(host: &Host, name: Option<&str>, follow: bool, lines: u32) -> i32 {
    let n = lines.to_string();
    if host.systemd {
        let Some(name) = name else {
            eprintln!("errand-box: name a daemon: incusd, incus-user, lxcfs, errand or maint");
            return 2;
        };
        let units = units_of(name);
        if units.is_empty() {
            eprintln!("errand-box: no daemon {name:?}: incusd, incus-user, lxcfs, errand or maint");
            return 2;
        }
        let mut c = Cmd::new("journalctl").args(["--no-pager", "-n", &n]);
        for u in units {
            c = c.args(["-u", u]);
        }
        if follow {
            c = c.arg("-f");
        }
        return exec(&c);
    }
    let Some(name) = name else {
        let mut names: Vec<String> = std::fs::read_dir(up::LOG_DIR)
            .map(|d| {
                d.flatten()
                    .filter(|e| e.path().join("current").exists())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        if names.is_empty() {
            eprintln!("errand-box: no logs in {}", up::LOG_DIR);
            return 1;
        }
        for n in names {
            let text = std::fs::read_to_string(Path::new(up::LOG_DIR).join(&n).join("current"))
                .unwrap_or_default();
            let last = text
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("");
            println!("{n:<18} {}", last.chars().take(160).collect::<String>());
        }
        return 0;
    };
    let name = if name == "maint" {
        "errand-box-maint"
    } else {
        name
    };
    let file = Path::new(up::LOG_DIR).join(name).join("current");
    if !file.exists() {
        eprintln!("errand-box: no log at {}", file.display());
        return 1;
    }
    let mut c = Cmd::new("tail").args(["-n", &n]);
    if follow {
        c = c.arg("-F");
    }
    exec(&c.arg(file.to_string_lossy()))
}

/// The instance for a session id or an instance name.
pub fn instance_for(target: &str) -> String {
    if target.starts_with(session::PREFIX) {
        target.to_owned()
    } else {
        session::instance_name(target)
    }
}

/// `errand-box shell SESSION [--root]`: a login shell in a running session's
/// instance, as the agent or as the guest's root.
pub fn shell(rt: &crate::runtime::Runtime, runner: &dyn Runner, target: &str, root: bool) -> i32 {
    let inc = Incus::new(rt.endpoint_for_caller(), runner);
    let name = instance_for(target);
    match inc.instance(&name) {
        Ok(Some(i)) if i.running() => {}
        Ok(Some(_)) => {
            eprintln!("errand-box: {name} is stopped; it runs only while its session does");
            return 1;
        }
        Ok(None) => {
            eprintln!("errand-box: no instance {name}; `errand-box status` lists the sessions");
            return 1;
        }
        Err(e) => {
            eprintln!("errand-box: {e}");
            return 1;
        }
    }
    let tail: Vec<&str> = if root {
        vec!["--", "sh", "-c", "exec bash -l 2>/dev/null || exec sh -l"]
    } else {
        // su gives the agent its groups (docker), which --user would not
        // (E-29).
        vec!["--", "su", "-l", "-s", "/bin/bash", "agent"]
    };
    let mut args = vec!["exec", "-t", &name];
    args.extend(tail);
    exec(&inc.cmd(args))
}

/// The digest a distrobuilder build recorded in `DIR/inputs`, if any.
fn recorded_build_inputs(dir: &Path) -> Option<String> {
    let t = std::fs::read_to_string(dir.join("inputs")).ok()?;
    let d = t.trim();
    (d.len() == 64 && d.chars().all(|c| c.is_ascii_hexdigit())).then(|| d.to_owned())
}

/// `errand-box image import DIR`: the image scripts/build-image.sh built,
/// checked against its SHA256SUMS, imported under the configured alias. The
/// alias moves only once the image is in, so a failure keeps the old one.
pub fn image_import(
    cfg: &Config,
    runner: &dyn Runner,
    dir: &Path,
    r: &mut Report,
) -> Result<(), String> {
    let files = ["incus.tar.xz", "rootfs.squashfs"];
    let sums = std::fs::read_to_string(dir.join("SHA256SUMS"))
        .map_err(|e| format!("{}: {e}", dir.join("SHA256SUMS").display()))?;
    for f in files {
        let want = sums
            .lines()
            .find_map(|l| {
                let (d, n) = l.split_once(char::is_whitespace)?;
                (n.trim().trim_start_matches('*') == f).then(|| d.to_owned())
            })
            .ok_or_else(|| format!("SHA256SUMS lists no {f}"))?;
        let out = runner.run(
            &Cmd::new("sha256sum")
                .arg(dir.join(f).to_string_lossy())
                .timeout(Duration::from_secs(600)),
        );
        let got = out
            .stdout
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_owned();
        if !out.ok() || got != want {
            return Err(format!(
                "{f}: digest {got:?} does not match SHA256SUMS ({want})"
            ));
        }
    }
    r.add(
        "digests",
        St::Ok,
        "incus.tar.xz and rootfs.squashfs match SHA256SUMS",
    );
    let inc = Incus::new(Endpoint::default(), runner);
    let out = runner.run(
        &inc.cmd([
            "image".to_owned(),
            "import".to_owned(),
            dir.join(files[0]).to_string_lossy().into_owned(),
            dir.join(files[1]).to_string_lossy().into_owned(),
        ])
        .timeout(Duration::from_secs(1800)),
    );
    let fp = out
        .stdout
        .lines()
        .chain(out.stderr.lines())
        .find_map(|l| l.split("fingerprint:").nth(1))
        .map(|s| s.trim().to_owned())
        .filter(|s| s.len() >= 12 && s.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| format!("incus image import: {}", out.why()))?;
    let mut props = vec![(
        "description".to_owned(),
        "errand-box guest, Debian 13, distrobuilder".to_owned(),
    )];
    match recorded_build_inputs(dir) {
        Some(d) => props.push((image::INPUTS_KEY.to_owned(), d)),
        None => r.add(
            "inputs",
            St::Degraded,
            "DIR/inputs is missing: setup cannot tell whether this image is current, and rebuilds it (E-31)",
        ),
    }
    for (k, v) in &props {
        let o = inc.run(["image", "set-property", &fp, k, v]);
        if !o.ok() {
            return Err(format!("cannot set {k} on {fp}: {}", o.why()));
        }
    }
    let old = image::alias_target(&inc, &cfg.image.alias);
    inc.move_alias(&cfg.image.alias, &fp)?;
    if let Some(o) = old.filter(|o| *o != fp) {
        let _ = inc.run(["image", "delete", &o]);
    }
    r.add(
        "guest image",
        St::Changed,
        format!(
            "{} is {}; `errand-box setup` puts it in errand's project and remakes the golden instance",
            cfg.image.alias,
            &fp[..12]
        ),
    );
    Ok(())
}

/// What `uninstall` removes: files and directories, in order.
pub fn uninstall_paths(purge: bool) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = [
        "/usr/local/lib/errand-box",
        "/usr/local/bin/errand-box",
        "/usr/local/sbin/errand-box",
        crate::runtime::PATH,
        up::PROXY_ENV,
        setup::LOCAL_START,
        up::SCAN_DIR,
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    for u in [
        "errand-box-errand.service",
        "errand-box-maint.service",
        "errand-box-lxcfs.service",
    ] {
        v.push(Path::new(up::UNIT_DIR).join(u));
    }
    if purge {
        for p in [
            paths::SYSTEM_STATE,
            up::LOG_DIR,
            "/etc/errand-box",
            paths::RUN_DIR,
        ] {
            v.push(PathBuf::from(p));
        }
    }
    v
}

/// `errand-box uninstall [--purge] [--yes]`. Without `--yes` it prints the
/// plan and changes nothing. It removes errand-box's daemons, programs and
/// files; with `--purge` also its instances, images, build bridge, state
/// and logs. Incus, its packages, the storage pool, the errand account and
/// errand's home stay.
pub fn uninstall(cfg: &Config, host: &Host, runner: &dyn Runner, purge: bool, yes: bool) -> Report {
    let mut r = Report::live();
    if paths::euid() != 0 {
        r.add("root", St::Failed, "errand-box uninstall needs root");
        return r;
    }
    let rt = crate::runtime::load().ok();
    if !yes {
        if purge {
            let what = rt
                .as_ref()
                .map(|rt| format!("every {}* instance in project {}, the golden instance included, and image {}", session::PREFIX, rt.admin_project, cfg.image.alias))
                .unwrap_or_else(|| "errand-box's instances (none found: no runtime.toml)".into());
            r.add("would delete", St::Skipped, what);
            r.add(
                "would delete",
                St::Skipped,
                format!("the build bridge {}", image::BUILD_NET),
            );
        }
        r.add("would stop", St::Skipped, if host.systemd {
            "errand-box-errand, errand-box-maint and errand-box-lxcfs units; Incus's own units stay".to_owned()
        } else {
            "every daemon under errand-box's s6, incusd included: nothing runs Incus afterwards".to_owned()
        });
        for p in uninstall_paths(purge).iter().filter(|p| p.exists()) {
            r.add("would remove", St::Skipped, p.display().to_string());
        }
        r.add(
            "kept",
            St::Skipped,
            format!(
                "Incus and its packages, the pool {}, the account {} and {}",
                cfg.engine.pool, cfg.errand.user, cfg.errand.home
            ),
        );
        r.add(
            "plan",
            St::Ok,
            "nothing changed; run again with --yes to do it",
        );
        return r;
    }
    if purge && let Some(rt) = &rt {
        let inc = Incus::new(rt.admin_endpoint(), runner);
        match inc.instances(session::PREFIX) {
            Ok(all) => {
                for i in all {
                    let st = match inc.delete(&i.name) {
                        Ok(()) => St::Changed,
                        Err(_) => St::Failed,
                    };
                    r.add("instance", st, i.name);
                }
            }
            Err(e) => r.add("instances", St::Failed, e),
        }
        let _ = inc.run(["image", "delete", &cfg.image.alias]);
        let admin = Incus::new(Endpoint::default(), runner);
        let _ = admin.run(["image", "delete", &cfg.image.alias]);
        let _ = admin.run(["network", "delete", image::BUILD_NET]);
        r.add(
            "images and bridge",
            St::Changed,
            format!(
                "{} and {} deleted where they existed",
                cfg.image.alias,
                image::BUILD_NET
            ),
        );
    }
    if host.systemd {
        for u in [
            "errand-box-errand.service",
            "errand-box-maint.service",
            "errand-box-lxcfs.service",
        ] {
            let _ = runner.run(&Cmd::new("systemctl").args(["disable", "--now", u]));
        }
        r.add(
            "units",
            St::Changed,
            "errand-box's units stopped and disabled",
        );
    } else if Path::new(up::SCAN_DIR).exists() {
        // Each service is stopped by its supervisor, which signals its own
        // child by pid (E-12); then s6-svscan by its recorded pid.
        if let Ok(d) = std::fs::read_dir(up::SCAN_DIR) {
            for e in d.flatten() {
                let _ = runner.run(
                    &Cmd::new(&up::s6_bin("s6-svc"))
                        .args(["-wD", "-T", "30000", "-d"])
                        .arg(e.path().to_string_lossy())
                        .timeout(Duration::from_secs(40)),
                );
            }
        }
        let _ = runner.run(&Cmd::new(&up::s6_bin("s6-svscanctl")).args(["-t", up::SCAN_DIR]));
        r.add(
            "daemons",
            St::Changed,
            "every daemon under errand-box's s6 stopped, incusd included",
        );
    }
    for p in uninstall_paths(purge) {
        let res = if p.is_dir() && !p.is_symlink() {
            std::fs::remove_dir_all(&p)
        } else {
            std::fs::remove_file(&p)
        };
        match res {
            Ok(()) => r.add("removed", St::Changed, p.display().to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => r.add("removed", St::Failed, format!("{}: {e}", p.display())),
        }
    }
    if host.systemd {
        let _ = runner.run(&Cmd::new("systemctl").arg("daemon-reload"));
    }
    r.add(
        "kept",
        St::Ok,
        format!(
            "Incus and its packages, the pool {}, the account {} and {}",
            cfg.engine.pool, cfg.errand.user, cfg.errand.home
        ),
    );
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_resolve_to_instances() {
        assert_eq!(instance_for("abc123"), "eb-abc123");
        assert_eq!(instance_for("eb-abc123"), "eb-abc123");
        assert_eq!(units_of("maint"), &["errand-box-maint.service"]);
        assert!(units_of("nope").is_empty());
    }

    #[test]
    fn uninstall_keeps_state_without_purge() {
        let keep = uninstall_paths(false);
        assert!(!keep.iter().any(|p| p == Path::new(paths::SYSTEM_STATE)));
        assert!(
            keep.iter()
                .any(|p| p == Path::new("/usr/local/lib/errand-box"))
        );
        assert!(
            uninstall_paths(true)
                .iter()
                .any(|p| p == Path::new("/etc/errand-box"))
        );
    }

    #[test]
    fn build_inputs_are_read_only_when_well_formed() {
        let d = std::env::temp_dir().join(format!("eb-inputs-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("inputs"), format!("{}\n", "a".repeat(64))).unwrap();
        assert_eq!(recorded_build_inputs(&d), Some("a".repeat(64)));
        std::fs::write(d.join("inputs"), "short\n").unwrap();
        assert_eq!(recorded_build_inputs(&d), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
