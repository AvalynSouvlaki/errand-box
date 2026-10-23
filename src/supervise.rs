//! The daemons errand-box keeps running, and the two ways it keeps them up.
//!
//! - A host whose PID 1 is systemd gets systemd units.
//! - A host with no init (E-01) gets s6: `s6-svscan` watches one directory
//!   per service and restarts a service that exits. `errand-box up` starts
//!   `s6-svscan` when it is not running, at boot where something runs `up`,
//!   and on first use where nothing does.
//!
//! Every service is stopped through its supervisor, which signals its own
//! child by pid. Nothing here signals a process by name: every guest's LXC
//! monitor is also called `incusd`, so a kill by name stops every guest
//! (E-12).

use std::fmt::Write as _;

/// One supervised program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    /// argv; the first element is an absolute path.
    pub argv: Vec<String>,
    /// Extra environment, beyond a fixed PATH.
    pub env: Vec<(String, String)>,
    /// Run as this account through `errand-box __as`, if set.
    pub user: Option<String>,
    /// A file of `KEY=VALUE` lines read at start, if it exists.
    pub env_file: Option<String>,
    /// Shell commands run before the program, each allowed to fail.
    pub prepare: Vec<String>,
    pub description: String,
    /// Installed but not started (s6: a `down` file; systemd: not enabled).
    pub down: bool,
}

pub const PATH_VALUE: &str = "/usr/local/lib/errand-box/shim:/usr/local/lib/errand-box/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

fn sh_word(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-=:,@%+".contains(c))
    {
        s.to_owned()
    } else {
        crate::session::sh_quote(s)
    }
}

/// The s6 `run` script of a service.
pub fn s6_run(s: &Service, errand_box: &str) -> String {
    let mut out = String::from("#!/bin/sh\n");
    let _ = writeln!(
        out,
        "# {} (written by errand-box setup; edits are lost on the next setup)",
        s.description
    );
    out.push_str("exec 2>&1\n");
    let _ = writeln!(out, "export PATH={}", sh_word(PATH_VALUE));
    if let Some(f) = &s.env_file {
        let _ = writeln!(
            out,
            "if [ -r {f} ]; then set -a; . {f}; set +a; fi",
            f = sh_word(f)
        );
    }
    for (k, v) in &s.env {
        let _ = writeln!(out, "export {k}={}", sh_word(v));
    }
    for p in &s.prepare {
        let _ = writeln!(out, "{p} || true");
    }
    let mut argv: Vec<String> = Vec::new();
    if let Some(u) = &s.user {
        argv.extend([
            errand_box.to_owned(),
            "__as".to_owned(),
            u.clone(),
            "--".to_owned(),
        ]);
    }
    argv.extend(s.argv.iter().cloned());
    let words: Vec<String> = argv.iter().map(|a| sh_word(a)).collect();
    let _ = writeln!(out, "exec {}", words.join(" "));
    out
}

/// The `log/run` of a service under s6: its output goes to `<dir>/<name>`,
/// in at most ten files of 1 MB, each line stamped with the time. A service
/// that restarts every second then fills 10 MB, not the disk.
pub fn s6_log_run(name: &str, s6_log: &str, dir: &str) -> String {
    let d = sh_word(&format!("{dir}/{name}"));
    format!(
        "#!/bin/sh\n# The log of {name} (written by errand-box setup).\nmkdir -p {d}\nexec {} -b n10 s1000000 T {d}\n",
        sh_word(s6_log)
    )
}

/// The pid in an `s6-svstat` line. Older s6 prints `up (pid 123) 4 seconds`;
/// newer s6 prints `up (pid 123 pgid 123) 4 seconds`, so take the first
/// number after `(pid `, not everything up to the `)` (E-62).
pub fn svstat_pid(state: &str) -> Option<u32> {
    state
        .split("(pid ")
        .nth(1)?
        .split([' ', ')'])
        .next()?
        .trim()
        .parse()
        .ok()
}

/// The seconds in an `s6-svstat` line.
pub fn svstat_seconds(state: &str) -> Option<u64> {
    let w: Vec<&str> = state.split_whitespace().collect();
    w.iter()
        .position(|x| matches!(x.trim_end_matches(','), "seconds" | "second"))
        .and_then(|i| w.get(i.wrapping_sub(1))?.parse().ok())
}

/// A systemd unit for a service.
pub fn systemd_unit(s: &Service, errand_box: &str, after: &[&str], before: &[&str]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Written by errand-box setup; edits are lost on the next setup."
    );
    let _ = writeln!(out, "[Unit]\nDescription={}", s.description);
    if !after.is_empty() {
        let _ = writeln!(out, "After={}\nWants={}", after.join(" "), after.join(" "));
    }
    if !before.is_empty() {
        let _ = writeln!(out, "Before={}", before.join(" "));
    }
    let _ = writeln!(out, "\n[Service]");
    let _ = writeln!(out, "Environment=PATH={PATH_VALUE}");
    if let Some(f) = &s.env_file {
        let _ = writeln!(out, "EnvironmentFile=-{f}");
    }
    for (k, v) in &s.env {
        let _ = writeln!(out, "Environment={k}={v}");
    }
    for p in &s.prepare {
        let _ = writeln!(
            out,
            "ExecStartPre=-/bin/sh -c {}",
            crate::session::sh_quote(p)
        );
    }
    let mut argv: Vec<String> = Vec::new();
    if let Some(u) = &s.user {
        argv.extend([
            errand_box.to_owned(),
            "__as".to_owned(),
            u.clone(),
            "--".to_owned(),
        ]);
    }
    argv.extend(s.argv.iter().cloned());
    let _ = writeln!(out, "ExecStart={}", argv.join(" "));
    // incusd leaves its guests running on SIGTERM; KillMode=process keeps
    // systemd from signalling the guests' monitors along with it.
    out.push_str(
        "KillMode=process\nRestart=always\nRestartSec=2\n\n[Install]\nWantedBy=multi-user.target\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc() -> Service {
        Service {
            name: "errand".into(),
            argv: vec!["/usr/local/lib/errand-box/bin/errand".into(), "run".into()],
            env: vec![("HOME".into(), "/var/lib/errand".into())],
            user: Some("errand".into()),
            env_file: Some("/etc/errand-box/errand.env".into()),
            prepare: vec!["mkdir -p /x".into()],
            description: "errand chat agent".into(),
            down: false,
        }
    }

    #[test]
    fn s6_log_is_bounded() {
        let l = s6_log_run("errand", "/usr/bin/s6-log", "/var/log/errand-box");
        assert!(l.contains("exec /usr/bin/s6-log -b n10 s1000000 T /var/log/errand-box/errand\n"));
        assert!(l.contains("mkdir -p /var/log/errand-box/errand\n"));
    }

    #[test]
    fn svstat_lines() {
        assert_eq!(svstat_pid("up (pid 8877) 0 seconds"), Some(8877));
        assert_eq!(svstat_pid("up (pid 8877 pgid 8877) 0 seconds"), Some(8877));
        assert_eq!(
            svstat_seconds("up (pid 8877 pgid 8877) 28 seconds"),
            Some(28)
        );
        assert_eq!(svstat_seconds("up (pid 8877) 1284 seconds"), Some(1284));
        assert_eq!(
            svstat_seconds("up (pid 8877) 1 second, normally down"),
            Some(1)
        );
        assert_eq!(svstat_pid("down (exitcode 1) 0 seconds, want up"), None);
    }

    #[test]
    fn prepare_lines_may_fail() {
        assert!(s6_run(&svc(), "/e").contains("mkdir -p /x || true\n"));
        assert!(
            systemd_unit(&svc(), "/e", &[], &[])
                .contains("ExecStartPre=-/bin/sh -c 'mkdir -p /x'\n")
        );
    }

    #[test]
    fn s6_script_drops_privileges_and_execs() {
        let s = s6_run(&svc(), "/usr/local/lib/errand-box/bin/errand-box");
        assert!(s.starts_with("#!/bin/sh\n"));
        assert!(s.contains("exec /usr/local/lib/errand-box/bin/errand-box __as errand -- /usr/local/lib/errand-box/bin/errand run\n"));
        assert!(s.contains("export HOME=/var/lib/errand\n"));
        assert!(s.contains("if [ -r /etc/errand-box/errand.env ]; then set -a; . /etc/errand-box/errand.env; set +a; fi\n"));
    }

    #[test]
    fn systemd_unit_keeps_guests_on_stop() {
        let u = systemd_unit(
            &svc(),
            "/usr/local/lib/errand-box/bin/errand-box",
            &["incus.service"],
            &["x.service"],
        );
        assert!(u.contains("Before=x.service\n"));
        assert!(u.contains("KillMode=process\n"));
        assert!(u.contains("After=incus.service\n"));
        assert!(u.contains("ExecStart=/usr/local/lib/errand-box/bin/errand-box __as errand -- /usr/local/lib/errand-box/bin/errand run\n"));
    }

    #[test]
    fn odd_words_are_quoted() {
        assert_eq!(sh_word("a b"), "'a b'");
        assert_eq!(sh_word("/x/y-z"), "/x/y-z");
    }
}
