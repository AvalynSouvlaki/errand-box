//! The `bailey` errand calls: the same verbs and flags, over Incus.
//!
//! errand finds this program as `bailey` on its PATH (setup links it there)
//! and uses it as docs/bailey-contract.md describes:
//! - `doctor`: must exit 0 and print `landlock: ABI`; errand reads three
//!   lines of it;
//! - `trust <policy>`, `untrust <policy>`;
//! - `run ... -- true`: errand's start-up probe and per-session check;
//! - `run ... -- kage rpc ...`: the session itself, with ACP on stdin and
//!   stdout, so this program writes only to stderr.

use crate::incus::Incus;
use crate::policy;
use crate::run::Runner;
use crate::runtime::{self, Runtime};
use crate::session;
use crate::trust;
use std::path::PathBuf;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct RunArgs {
    pub isolate: bool,
    pub egress_proxy: Option<(String, u16)>,
    pub proxy_net: bool,
    pub config: Option<PathBuf>,
    pub profile: Option<String>,
    pub quiet: bool,
    pub command: Vec<String>,
}

pub fn parse_run(args: &[String]) -> Result<RunArgs, String> {
    let mut r = RunArgs::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let mut value = |name: &str| -> Result<String, String> {
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match a {
            "--" => {
                r.command = args[i + 1..].to_vec();
                break;
            }
            "--isolate" => r.isolate = true,
            "--proxy-net" => r.proxy_net = true,
            "--quiet" | "-q" => r.quiet = true,
            "--config" => r.config = Some(PathBuf::from(value("--config")?)),
            "--profile" => {
                let p = value("--profile")?;
                if !matches!(p.as_str(), "ai-agent" | "untrusted") {
                    return Err(format!(
                        "unknown profile {p:?}; errand uses ai-agent or untrusted"
                    ));
                }
                r.profile = Some(p);
            }
            "--egress-proxy" => {
                let v = value("--egress-proxy")?;
                let (addr, port) = v.rsplit_once(':').ok_or("--egress-proxy needs ADDR:PORT")?;
                let port: u16 = port
                    .parse()
                    .map_err(|_| format!("bad port in --egress-proxy {v}"))?;
                if addr != session::BROKER_ADDR {
                    return Err(format!(
                        "--egress-proxy {addr}: this engine maps only errand's broker address {}",
                        session::BROKER_ADDR
                    ));
                }
                r.egress_proxy = Some((addr.to_owned(), port));
            }
            other => return Err(format!("unknown option {other:?}")),
        }
        i += 1;
    }
    if r.command.is_empty() {
        return Err("no command after --".into());
    }
    Ok(r)
}

/// The Landlock ABI of the running kernel, or 0.
fn landlock_abi() -> i64 {
    // landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION) is
    // syscall 444 on every architecture with the common table.
    // SAFETY: a version query passes no pointers the kernel writes.
    let r = unsafe { libc::syscall(444, std::ptr::null::<u8>(), 0usize, 1u32) };
    if r < 0 { 0 } else { r }
}

/// The doctor report, as lines. errand reads `landlock: ABI`,
/// `user namespaces:` and `cgroup delegation:`; the rest is for people.
/// The limits Incus holds on a session instance, read from the golden one's
/// profile, so `doctor` states what is really enforced, not what setup
/// means to set.
fn instance_limits(rt: &Runtime, runner: &dyn Runner) -> String {
    let inc = Incus::new(rt.endpoint(), runner);
    let v = match inc.query("/1.0/profiles/default") {
        Ok(v) => v,
        Err(_) => return "unknown".to_owned(),
    };
    let get = |k: &str| {
        v.pointer(&format!("/config/{}", k.replace('.', "~1")))
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
    };
    let mut parts = Vec::new();
    parts.push(match get("limits.memory") {
        Some(m) => format!("memory {m}"),
        None => "memory unset".to_owned(),
    });
    parts.push(match get("limits.cpu") {
        Some(c) => format!("cpu {c}"),
        None => "cpu unset".to_owned(),
    });
    parts.push(match get("limits.processes") {
        Some(p) => format!("processes {p}"),
        None => "processes unlimited".to_owned(),
    });
    parts.join(", ")
}

pub fn doctor_lines(rt: &Runtime, runner: &dyn Runner) -> (Vec<String>, bool) {
    let inc = Incus::new(rt.endpoint(), runner);
    let abi = landlock_abi();
    let mut lines = vec![
        format!(
            "errand-box {} (bailey interface; sessions are Incus system containers)",
            env!("CARGO_PKG_VERSION")
        ),
        "kernel:".to_owned(),
        format!(
            "  landlock: ABI {} (not used: each session is a whole guest)",
            if abi > 0 {
                abi.to_string()
            } else {
                "none".to_owned()
            }
        ),
        "  user namespaces: yes".to_owned(),
        format!(
            "  cgroup delegation: memory, pids, cpu (per-instance limits: {})",
            instance_limits(rt, runner)
        ),
        "engine:".to_owned(),
    ];
    let mut ok = true;
    match inc.server_version() {
        Ok(v) => lines.push(format!("  incus: {v} ({} mode)", rt.mode)),
        Err(e) => {
            ok = false;
            lines.push(format!("  incus: not reachable: {e}"));
        }
    }
    if ok {
        match inc.instance(&rt.golden) {
            Ok(Some(_)) => lines.push(format!("  golden instance: {}", rt.golden)),
            Ok(None) => {
                ok = false;
                lines.push(format!(
                    "  golden instance: {} is missing; run `errand-box setup`",
                    rt.golden
                ));
            }
            Err(e) => {
                ok = false;
                lines.push(format!("  golden instance: {e}"));
            }
        }
    }
    (lines, ok)
}

pub fn main(args: &[String], runner: &dyn Runner) -> i32 {
    let Some(verb) = args.first() else {
        eprintln!(
            "usage: bailey doctor | trust <policy> | untrust <policy> | run [options] -- <command>"
        );
        return 2;
    };
    let rest = &args[1..];
    match verb.as_str() {
        "doctor" => {
            let rt = match runtime::load() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("errand-box: {e}");
                    return 1;
                }
            };
            let (lines, ok) = doctor_lines(&rt, runner);
            for l in lines {
                println!("{l}");
            }
            if ok { 0 } else { 1 }
        }
        "trust" => match rest {
            [p] => match trust::trust(&PathBuf::from(p)) {
                Ok(()) => {
                    println!("trusted {p}");
                    0
                }
                Err(e) => {
                    eprintln!("errand-box: {e}");
                    1
                }
            },
            _ => {
                eprintln!("usage: bailey trust <policy>");
                2
            }
        },
        "untrust" => match rest {
            [p] => match trust::untrust(&PathBuf::from(p)) {
                Ok(true) => {
                    println!("untrusted {p}");
                    0
                }
                Ok(false) => {
                    eprintln!("errand-box: `{p}` was not trusted");
                    1
                }
                Err(e) => {
                    eprintln!("errand-box: {e}");
                    1
                }
            },
            _ => {
                eprintln!("usage: bailey untrust <policy>");
                2
            }
        },
        "run" => run(rest, runner),
        "--version" | "version" => {
            println!(
                "errand-box {} (bailey interface)",
                env!("CARGO_PKG_VERSION")
            );
            0
        }
        other => {
            eprintln!("errand-box: unknown bailey command {other:?}");
            2
        }
    }
}

fn run(args: &[String], runner: &dyn Runner) -> i32 {
    let r = match parse_run(args) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("errand-box: {e}");
            return 2;
        }
    };
    let Some(cfg) = &r.config else {
        eprintln!("errand-box: run needs --config <policy>; errand always passes it");
        return 2;
    };
    let pol = match trust::verified(cfg)
        .and_then(|b| String::from_utf8(b).map_err(|_| "the policy is not UTF-8".to_owned()))
        .and_then(|t| policy::parse(&t))
    {
        Ok(p) => p,
        Err(e) => {
            eprintln!("errand-box: {e}");
            return 1;
        }
    };
    let rt = match runtime::load() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("errand-box: {e}");
            return 1;
        }
    };
    if r.command == ["true"] {
        return check(&rt, runner, &pol, r.isolate, r.quiet);
    }
    let broker = r.egress_proxy.as_ref().map(|(_, p)| *p);
    let from_env = |n: &str| std::env::var(n).ok();
    session::run(&rt, runner, &pol, broker, &r.command, &from_env, r.quiet)
}

/// errand's two short calls run `true`. The shape probe (no `--isolate`)
/// proves the policy parses and the engine answers. The per-session check
/// (`--isolate`) also creates or updates the session's instance, so the
/// session run that follows only has to start it.
fn check(
    rt: &Runtime,
    runner: &dyn Runner,
    pol: &policy::Policy,
    isolate: bool,
    quiet: bool,
) -> i32 {
    let inc = Incus::new(rt.endpoint(), runner);
    if let Err(e) = inc.server_version() {
        eprintln!("errand-box: the Incus engine does not answer: {e}");
        return 1;
    }
    if let Err(e) = pol.mounts(&|_| false) {
        eprintln!("errand-box: {e}");
        return 1;
    }
    if !isolate {
        return 0;
    }
    match session::prepare(&inc, rt, pol, None, None) {
        Ok(i) => {
            if !quiet {
                eprintln!("errand-box: {} is ready", i.name);
            }
            0
        }
        Err(e) => {
            eprintln!("errand-box: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn errands_session_argv() {
        let r = parse_run(&v("--isolate --egress-proxy 169.254.169.1:41234 --config /s/abc/policy.toml --profile ai-agent -- kage rpc --yolo -m p/m")).unwrap();
        assert!(r.isolate);
        assert_eq!(r.egress_proxy, Some(("169.254.169.1".into(), 41234)));
        assert_eq!(r.config, Some(PathBuf::from("/s/abc/policy.toml")));
        assert_eq!(r.command, v("kage rpc --yolo -m p/m"));
    }

    #[test]
    fn errands_probe_and_check_argv() {
        let r = parse_run(&v("--config /s/probe/shape.toml --quiet -- true")).unwrap();
        assert!(!r.isolate && r.quiet);
        assert_eq!(r.command, v("true"));
        let r = parse_run(&v(
            "--isolate --config /s/a/policy.toml --profile untrusted --quiet -- true",
        ))
        .unwrap();
        assert!(r.isolate);
        assert_eq!(r.profile.as_deref(), Some("untrusted"));
    }

    #[test]
    fn options_after_the_separator_belong_to_the_command() {
        let r = parse_run(&v("--config c -- kage --quiet --isolate")).unwrap();
        assert_eq!(r.command, v("kage --quiet --isolate"));
        assert!(!r.quiet);
    }

    #[test]
    fn refusals() {
        assert!(parse_run(&v("--config c")).is_err());
        assert!(parse_run(&v("--frobnicate -- true")).is_err());
        assert!(parse_run(&v("--profile admin -- true")).is_err());
        assert!(
            parse_run(&v("--egress-proxy 10.0.0.1:80 -- true"))
                .unwrap_err()
                .contains("169.254.169.1")
        );
        assert!(parse_run(&v("--egress-proxy 169.254.169.1:x -- true")).is_err());
        assert!(parse_run(&v("--config")).is_err());
    }

    #[test]
    fn doctor_reports_what_errand_parses() {
        let f = crate::run::Fake::new();
        f.on(
            "incus query /1.0",
            0,
            r#"{"environment":{"server_version":"6.0.6"}}"#,
        );
        f.on(
            "incus query /1.0/instances/eb-golden",
            0,
            r#"{"name":"eb-golden","status":"Stopped"}"#,
        );
        let rt = Runtime {
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
        };
        let (lines, ok) = doctor_lines(&rt, &f);
        assert!(ok, "{lines:?}");
        let text = lines.join("\n");
        // errand trims each line, then matches these prefixes.
        assert!(
            lines.iter().any(|l| l.trim().starts_with("landlock: ABI")),
            "{text}"
        );
        assert!(lines.iter().any(|l| l.trim() == "user namespaces: yes"));
        assert!(
            lines
                .iter()
                .any(|l| l.trim().starts_with("cgroup delegation:") && !l.trim().ends_with("no"))
        );
    }

    #[test]
    fn doctor_fails_when_the_golden_instance_is_missing() {
        let f = crate::run::Fake::new();
        f.on(
            "incus query /1.0",
            0,
            r#"{"environment":{"server_version":"6.0.6"}}"#,
        );
        f.on(
            "incus query /1.0/instances/",
            1,
            "Error: Instance not found",
        );
        let rt: Runtime = toml::from_str(
            "mode='root'\nincus_bin='incus'\ngolden='eb-golden'\nguest_uid=1000\nguest_gid=1000\nstop_seconds=4\nguest_proxy_port=3128\nbroker_link='tcp'\n",
        )
        .unwrap();
        let (lines, ok) = doctor_lines(&rt, &f);
        assert!(!ok);
        assert!(lines.join("\n").contains("missing"));
    }
}
