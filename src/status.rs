//! `errand-box status`: how the running system is, read-only and quick.
//!
//! `doctor` reports what the host is and what is installed; `status` reports
//! what runs: each daemon and how long it has been up, the pool and the
//! host disk, whether the image is current, the golden instance, and the
//! sessions. Every row is a fact read now; nothing is changed.

use crate::config::Config;
use crate::host::Host;
use crate::incus::{Endpoint, Incus};
use crate::report::{Report, St};
use crate::run::{Cmd, Runner};
use crate::up;
use std::path::Path;

/// A share of a filesystem or pool in use, as a row: degraded at 85 %,
/// failed at 95 %, where writes in guests start to fail.
pub fn usage_row(used: u64, total: u64) -> (St, String) {
    if total == 0 {
        return (St::Degraded, "size unknown".into());
    }
    let pct = used.saturating_mul(100) / total;
    let st = match pct {
        0..=84 => St::Ok,
        85..=94 => St::Degraded,
        _ => St::Failed,
    };
    (
        st,
        format!("{pct}% used, {} of {} GiB", used >> 30, total >> 30),
    )
}

/// Free and total bytes of the filesystem that holds `p`.
pub fn fs_space(p: &str) -> Option<(u64, u64)> {
    let c = std::ffi::CString::new(p).ok()?;
    // SAFETY: statvfs fills a struct we own.
    unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut s) != 0 {
            return None;
        }
        let f = s.f_frsize as u64;
        Some((s.f_bavail as u64 * f, s.f_blocks as u64 * f))
    }
}

/// The pid an `s6-svstat` line or a systemd unit names, and whether that
/// process runs a program a package upgrade has replaced since it started.
pub fn runs_replaced_program(pid: u32) -> bool {
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .map(|p| p.to_string_lossy().ends_with(" (deleted)"))
        .unwrap_or(false)
}

fn daemons(host: &Host, runner: &dyn Runner, r: &mut Report) {
    if host.systemd {
        for unit in [
            "incus.service",
            "incus-user.socket",
            "incus-lxcfs.service",
            "errand-box-lxcfs.service",
            "errand-box-maint.service",
            "errand-box-errand.service",
        ] {
            let out = runner.run(&Cmd::new("systemctl").args([
                "show",
                "-p",
                "ActiveState",
                "-p",
                "UnitFileState",
                "-p",
                "NRestarts",
                "-p",
                "MainPID",
                unit,
            ]));
            let p: std::collections::BTreeMap<&str, &str> = out
                .stdout
                .lines()
                .filter_map(|l| l.split_once('='))
                .collect();
            let file = p.get("UnitFileState").copied().unwrap_or("");
            if file.is_empty() || file == "disabled" {
                continue;
            }
            let active = p.get("ActiveState").copied().unwrap_or("");
            let restarts = p.get("NRestarts").copied().unwrap_or("0");
            let pid: u32 = p.get("MainPID").and_then(|x| x.parse().ok()).unwrap_or(0);
            let mut d = format!("{active}, {restarts} restarts");
            let mut st = if active == "active" {
                St::Ok
            } else {
                St::Failed
            };
            if pid > 0 && runs_replaced_program(pid) {
                st = St::Degraded;
                d.push_str(
                    "; runs a program replaced since it started: restart it when no session runs",
                );
            }
            r.add(unit, st, d);
        }
        return;
    }
    let Ok(dir) = std::fs::read_dir(up::SCAN_DIR) else {
        r.add(
            "supervisor",
            St::Failed,
            format!("{} is missing; run `errand-box setup`", up::SCAN_DIR),
        );
        return;
    };
    let mut names: Vec<String> = dir
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    for n in names {
        let state = runner
            .run(&Cmd::new(&up::s6_bin("s6-svstat")).arg(format!("{}/{n}", up::SCAN_DIR)))
            .stdout
            .trim()
            .to_owned();
        let wanted_down = Path::new(up::SCAN_DIR).join(&n).join("down").exists();
        let up_secs = crate::supervise::svstat_seconds(&state).unwrap_or(0);
        let (mut st, mut d) = if state.starts_with("up") {
            // Up for under a minute: it may be restarting in a loop that
            // `up` could not see in its own few seconds.
            if up_secs < 60 {
                (
                    St::Degraded,
                    format!("{state}; up for under a minute, see `errand-box logs {n}`"),
                )
            } else {
                (St::Ok, state.clone())
            }
        } else if wanted_down {
            (St::Skipped, state.clone())
        } else if state.is_empty() {
            (
                St::Failed,
                "s6-svscan is not running; run `errand-box up`".to_owned(),
            )
        } else {
            (St::Failed, format!("{state}; see `errand-box logs {n}`"))
        };
        if let Some(pid) = crate::supervise::svstat_pid(&state)
            && runs_replaced_program(pid)
        {
            st = St::Degraded;
            d.push_str(
                "; runs a program replaced since it started: restart it when no session runs",
            );
        }
        r.add(&n, st, d);
    }
}

pub fn status(cfg: &Config, host: &Host, runner: &dyn Runner) -> Report {
    let mut r = Report::default();
    daemons(host, runner, &mut r);
    let admin = Incus::new(Endpoint::default(), runner);
    match admin.server_version() {
        Ok(v) => r.add("incus", St::Ok, format!("server {v}")),
        Err(e) => {
            r.add("incus", St::Failed, format!("{e}; run `errand-box up`"));
            return r;
        }
    }
    let pool = &cfg.engine.pool;
    match admin.query(&format!("/1.0/storage-pools/{pool}/resources")) {
        Ok(v) => {
            let g = |k: &str| {
                v.pointer(&format!("/space/{k}"))
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0)
            };
            let (st, d) = usage_row(g("used"), g("total"));
            r.add("pool", st, format!("{pool}: {d}"));
        }
        Err(e) => r.add("pool", St::Failed, format!("{pool}: {e}")),
    }
    for p in ["/var/lib/incus", "/var/log"] {
        if let Some((free, total)) = fs_space(p) {
            let (st, d) = usage_row(total.saturating_sub(free), total);
            r.add(&format!("disk {p}"), st, d);
        }
    }
    match (
        crate::image::alias_target(&admin, &cfg.image.alias),
        crate::fetch::asset("kage", &cfg.kage.version, &host.arch),
    ) {
        (None, _) => r.add(
            "image",
            St::Failed,
            format!("{} is missing; run `errand-box setup`", cfg.image.alias),
        ),
        (Some(fp), Ok((kv, _))) => {
            let want = crate::image::inputs_digest(cfg, &kv);
            if crate::image::recorded_inputs(&admin, &fp).as_deref() == Some(want.as_str()) {
                r.add(
                    "image",
                    St::Ok,
                    format!("{} {}, current", cfg.image.alias, &fp[..12.min(fp.len())]),
                );
            } else {
                r.add(
                    "image",
                    St::Degraded,
                    format!(
                        "{} {} was built from other inputs; `errand-box setup` rebuilds it (E-31)",
                        cfg.image.alias,
                        &fp[..12.min(fp.len())]
                    ),
                );
            }
        }
        (Some(fp), Err(e)) => r.add("image", St::Degraded, format!("{fp}: {e}")),
    }
    let rt = match crate::runtime::load() {
        Ok(rt) => rt,
        Err(e) => {
            r.add("runtime.toml", St::Failed, e);
            return r;
        }
    };
    let inc = Incus::new(rt.endpoint_for_caller(), runner);
    match inc.instance(&rt.golden) {
        Ok(Some(_)) => r.add("golden", St::Ok, rt.golden.clone()),
        _ => r.add(
            "golden",
            St::Failed,
            format!("{} is missing; run `errand-box setup`", rt.golden),
        ),
    }
    match inc.instances(crate::session::PREFIX) {
        Ok(all) => {
            let sessions: Vec<_> = all
                .iter()
                .filter(|i| i.name != rt.golden && i.name != crate::setup::GOLDEN_NEXT)
                .collect();
            let running: Vec<_> = sessions.iter().filter(|i| i.running()).collect();
            let orphans = running
                .iter()
                .filter(|i| {
                    !i.config
                        .get(crate::session::LEASE_KEY)
                        .is_some_and(|l| crate::procs::lease_alive(l))
                })
                .count();
            let names: Vec<&str> = running.iter().map(|i| i.name.as_str()).take(10).collect();
            r.add(
                "sessions",
                if orphans > 0 { St::Degraded } else { St::Ok },
                format!(
                    "{} running{}, {} stopped{}",
                    running.len(),
                    if names.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", names.join(", "))
                    },
                    sessions.len() - running.len(),
                    if orphans > 0 {
                        format!("; {orphans} running without a launcher, which maint stops (E-27)")
                    } else {
                        String::new()
                    }
                ),
            );
        }
        Err(e) => r.add("sessions", St::Failed, e),
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_thresholds() {
        let g = 1u64 << 30;
        assert_eq!(usage_row(50 * g, 100 * g).0, St::Ok);
        assert_eq!(usage_row(85 * g, 100 * g).0, St::Degraded);
        assert_eq!(usage_row(95 * g, 100 * g).0, St::Failed);
        assert_eq!(usage_row(1, 0).0, St::Degraded);
        assert!(
            usage_row(50 * g, 100 * g)
                .1
                .starts_with("50% used, 50 of 100 GiB")
        );
    }

    #[test]
    fn this_process_runs_its_own_program() {
        assert!(!runs_replaced_program(std::process::id()));
    }
}
