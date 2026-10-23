//! `errand-box maint`: the periodic work, run by the supervisor.
//! - reap: stop session instances whose launcher and watchdog are both gone
//!   (E-27);
//! - fstrim: a btrfs pool on a loop file gives freed space back to the host
//!   only when trimmed (E-25);
//! - gc: delete stopped session instances unused for `box.keep_days`, or
//!   whose session errand deleted.

use crate::config::Config;
use crate::incus::Incus;
use crate::run::{Cmd, Runner};
use crate::runtime::Runtime;
use crate::session;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Instances `gc` deletes: stopped `eb-` instances other than the golden
/// one, unused for `keep_days`, or whose session directory is gone.
pub fn stale(
    rt: &Runtime,
    insts: &[crate::incus::Instance],
    keep_days: u32,
    now: u64,
    exists: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    let mut out = Vec::new();
    for i in insts {
        if i.name == rt.golden || i.running() || !i.name.starts_with(session::PREFIX) {
            continue;
        }
        let last = i
            .config
            .get(session::LAST_USED_KEY)
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        let old = last > 0 && now.saturating_sub(last) > u64::from(keep_days) * 86400;
        let gone = match (i.config.get(session::SESSION_KEY), rt.state_root.is_empty()) {
            (Some(id), false) => !exists(&format!("{}/{id}", rt.state_root)),
            _ => false,
        };
        if old || gone {
            out.push(i.name.clone());
        }
    }
    out
}

pub fn gc(
    rt: &Runtime,
    cfg: &Config,
    runner: &dyn Runner,
    out: &mut dyn Write,
) -> Result<usize, String> {
    let inc = Incus::new(rt.endpoint_for_caller(), runner);
    let insts = inc.instances(session::PREFIX)?;
    let doomed = stale(rt, &insts, cfg.boxes.keep_days, now(), &|p| {
        Path::new(p).exists()
    });
    for n in &doomed {
        let _ = writeln!(out, "deleting {n}");
        inc.delete(n)?;
    }
    Ok(doomed.len())
}

pub fn fstrim(cfg: &Config, runner: &dyn Runner, out: &mut dyn Write) -> Result<(), String> {
    let mount = format!("/var/lib/incus/storage-pools/{}", cfg.engine.pool);
    if !Path::new(&mount).exists() {
        return Err(format!("{mount} does not exist"));
    }
    let o = runner.run(
        &Cmd::new("fstrim")
            .args(["-v", &mount])
            .timeout(Duration::from_secs(1800)),
    );
    let _ = writeln!(
        out,
        "fstrim {mount}: {}",
        if o.ok() {
            o.stdout.trim().to_owned()
        } else {
            o.why()
        }
    );
    if o.ok() { Ok(()) } else { Err(o.why()) }
}

/// One pass. `last` holds the time of the last fstrim and gc.
pub fn once(
    rt: &Runtime,
    cfg: &Config,
    runner: &dyn Runner,
    out: &mut dyn Write,
    last: &mut (u64, u64),
) {
    match session::reap(rt, runner, out) {
        Ok(n) if n > 0 => {
            let _ = writeln!(out, "reaped {n}");
        }
        Ok(_) => {}
        Err(e) => {
            let _ = writeln!(out, "reap: {e}");
        }
    }
    let t = now();
    if cfg.maintenance.fstrim_hours > 0
        && t.saturating_sub(last.0) >= u64::from(cfg.maintenance.fstrim_hours) * 3600
    {
        if let Err(e) = fstrim(cfg, runner, out) {
            let _ = writeln!(out, "fstrim: {e}");
        }
        last.0 = t;
    }
    if t.saturating_sub(last.1) >= 86400 {
        match gc(rt, cfg, runner, out) {
            Ok(n) if n > 0 => {
                let _ = writeln!(out, "deleted {n} old instance(s)");
            }
            Ok(_) => {}
            Err(e) => {
                let _ = writeln!(out, "gc: {e}");
            }
        }
        last.1 = t;
    }
}

pub fn run_loop(rt: &Runtime, cfg: &Config, runner: &dyn Runner) -> ! {
    // The first trim and gc wait a while after boot, so start-up stays quick.
    let mut last = (now(), now().saturating_sub(86400 - 600));
    loop {
        once(rt, cfg, runner, &mut std::io::stdout(), &mut last);
        std::thread::sleep(Duration::from_secs(u64::from(
            cfg.maintenance.reap_seconds.max(5),
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::incus::Instance;
    use std::collections::BTreeMap;

    fn inst(name: &str, status: &str, last: Option<u64>, session: Option<&str>) -> Instance {
        let mut config = BTreeMap::new();
        if let Some(l) = last {
            config.insert(session::LAST_USED_KEY.to_owned(), l.to_string());
        }
        if let Some(s) = session {
            config.insert(session::SESSION_KEY.to_owned(), s.to_owned());
        }
        Instance {
            name: name.into(),
            status: status.into(),
            config,
            devices: BTreeMap::new(),
        }
    }

    #[test]
    fn stale_picks_old_and_orphaned_stopped_instances_only() {
        let rt: Runtime = toml::from_str(
            "mode='root'\nincus_bin='incus'\ngolden='eb-golden'\nguest_uid=1000\nguest_gid=1000\nstop_seconds=4\nguest_proxy_port=3128\nbroker_link='tcp'\nstate_root='/s'\n",
        )
        .unwrap();
        let day = 86400;
        let now = 100 * day;
        let insts = vec![
            inst("eb-golden", "Stopped", Some(0), None),
            inst("eb-old", "Stopped", Some(now - 20 * day), Some("a")),
            inst("eb-new", "Stopped", Some(now - day), Some("b")),
            inst("eb-gone", "Stopped", Some(now - day), Some("c")),
            inst("eb-running-old", "Running", Some(now - 20 * day), Some("a")),
            inst("other", "Stopped", Some(0), None),
        ];
        let exists = |p: &str| p != "/s/c";
        assert_eq!(
            stale(&rt, &insts, 14, now, &exists),
            vec!["eb-old".to_owned(), "eb-gone".to_owned()]
        );
    }
}
