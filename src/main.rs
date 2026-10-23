//! errand-box: one program with three faces.
//! - `errand-box <command>`: the operator's tool (`errand-box help`).
//! - `bailey <verb>`: what errand runs; setup links this program as
//!   `bailey` on errand's PATH (src/shim.rs).
//! - `errand-box __watchdog` and `__as`: started by errand-box itself.

mod config;
mod errandcfg;
mod fetch;
mod host;
mod image;
mod incus;
mod layout;
mod maint;
mod paths;
mod policy;
mod procs;
mod relay;
mod report;
mod run;
mod runtime;
mod session;
mod setup;
mod sha256;
mod shim;
mod supervise;
mod trust;
mod up;

use report::{Report, St};
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let argv0 = args.first().map(String::as_str).unwrap_or("errand-box");
    let name = Path::new(argv0)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("errand-box");
    let runner = run::System;
    let code = if name == "bailey" {
        shim::main(&args[1..], &runner)
    } else {
        cli(&args[1..], &runner)
    };
    std::process::exit(code);
}

const USAGE: &str = "usage: errand-box <command> [options]

  setup [--config F] [--env F] [--creds F] [--skip-image]
                        install and configure everything (root; idempotent)
  up [--config F]       bring daemons, devices and settings back (root; idempotent;
                        run at boot, and after any restart)
  doctor [--config F]   report the host and what is degraded; changes nothing
  image build [--config F]
                        build the guest image on this host (root)
  maint [--once|--loop] reap orphaned sessions, fstrim the pool, delete old instances
  gc                    delete stopped session instances past box.keep_days
  reap                  stop session instances whose launcher is gone
  errand-config --env F --creds F --home DIR --out F
                        write the config.json setup would give errand (mode
                        0600), and nothing else; tests check it against errand
  bailey <verb> ...     the interface errand calls (doctor, trust, untrust, run)
  version               print the version
  help                  this

The configuration defaults to /etc/errand-box/errand-box.toml; a missing file
means the defaults. Exit codes: 0 done, 1 something failed, 2 usage error.
";

/// errand's config.json from the two env files, written to `out` with mode
/// 0600. The values are never printed.
fn errand_config(env: &str, creds: &str, home: &str, out: &str) -> Result<(), String> {
    let read = |p: &str| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"));
    let settings = errandcfg::parse_env(&read(env)?)?;
    let secrets = errandcfg::parse_env(&read(creds)?)?;
    let json = errandcfg::build(&settings, &secrets, home)?;
    let text = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())? + "\n";
    if let Some(dir) = std::path::Path::new(out).parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    up::write_if_changed(std::path::Path::new(out), &text, 0o600).map(|_| ())
}

/// `--name value` from `args`, and the rest.
fn opt(args: &[String], name: &str) -> (Option<String>, Vec<String>) {
    let mut val = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == name && i + 1 < args.len() {
            val = Some(args[i + 1].clone());
            i += 2;
            continue;
        }
        if let Some(v) = args[i].strip_prefix(&format!("{name}=")) {
            val = Some(v.to_owned());
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    (val, rest)
}

fn load_config(args: &[String]) -> Result<(config::Config, Vec<String>), String> {
    let (c, rest) = opt(args, "--config");
    let path = PathBuf::from(c.unwrap_or_else(|| config::DEFAULT_PATH.to_owned()));
    Ok((config::load(&path)?, rest))
}

fn finish(r: &Report) -> i32 {
    print!("{}", r.render());
    r.code()
}

fn cli(args: &[String], runner: &dyn run::Runner) -> i32 {
    let Some(cmd) = args.first() else {
        eprint!("{USAGE}");
        return 2;
    };
    let rest = &args[1..];
    match cmd.as_str() {
        "bailey" => shim::main(rest, runner),
        "setup" => {
            let (c, rest) = opt(rest, "--config");
            let (env, rest) = opt(&rest, "--env");
            let (creds, rest) = opt(&rest, "--creds");
            let skip_image = rest.iter().any(|a| a == "--skip-image");
            if let Some(bad) = rest.iter().find(|a| *a != "--skip-image") {
                eprintln!("errand-box setup: unknown option {bad:?}");
                return 2;
            }
            let a = setup::Args {
                config: PathBuf::from(c.unwrap_or_else(|| config::DEFAULT_PATH.to_owned())),
                env: env.map(PathBuf::from),
                creds: creds.map(PathBuf::from),
                skip_image,
            };
            finish(&setup::setup(&a, runner))
        }
        "up" => match load_config(rest) {
            Ok((cfg, _)) => {
                let mut r = Report::default();
                up::up(&cfg, &host::detect(), runner, &mut r);
                finish(&r)
            }
            Err(e) => {
                eprintln!("errand-box: {e}");
                1
            }
        },
        "doctor" => match load_config(rest) {
            Ok((cfg, _)) => finish(&doctor(&cfg, runner)),
            Err(e) => {
                eprintln!("errand-box: {e}");
                1
            }
        },
        "image" => match rest.first().map(String::as_str) {
            Some("build") => match load_config(&rest[1..]) {
                Ok((cfg, _)) => {
                    let mut r = Report::default();
                    let cache = Path::new(paths::SYSTEM_STATE).join("cache");
                    if let Err(e) = image::build(&cfg, &host::detect(), runner, &mut r, &cache) {
                        r.add("guest image", St::Failed, e);
                    }
                    finish(&r)
                }
                Err(e) => {
                    eprintln!("errand-box: {e}");
                    1
                }
            },
            _ => {
                eprintln!("usage: errand-box image build [--config F]");
                2
            }
        },
        "maint" | "gc" => {
            let (cfg, rest) = match load_config(rest) {
                Ok(x) => x,
                Err(e) => {
                    eprintln!("errand-box: {e}");
                    return 1;
                }
            };
            let rt = match runtime::load() {
                Ok(r) => r,
                Err(e) if cmd == "maint" && rest.iter().any(|a| a == "--loop") => {
                    // Under the supervisor before setup has finished: wait
                    // instead of exiting, so the supervisor does not restart
                    // this every second.
                    eprintln!("errand-box maint: {e}; waiting");
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(30));
                        if let Ok(r) = runtime::load() {
                            break r;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("errand-box: {e}");
                    return 1;
                }
            };
            if cmd == "gc" {
                return match maint::gc(&rt, &cfg, runner, &mut std::io::stdout()) {
                    Ok(n) => {
                        println!("deleted {n} instance(s)");
                        0
                    }
                    Err(e) => {
                        eprintln!("errand-box: {e}");
                        1
                    }
                };
            }
            if rest.iter().any(|a| a == "--loop") {
                maint::run_loop(&rt, &cfg, runner);
            }
            let mut last = (0, 0);
            maint::once(&rt, &cfg, runner, &mut std::io::stdout(), &mut last);
            0
        }
        "reap" => match runtime::load()
            .and_then(|rt| session::reap(&rt, runner, &mut std::io::stdout()))
        {
            Ok(n) => {
                println!("reaped {n} instance(s)");
                0
            }
            Err(e) => {
                eprintln!("errand-box: {e}");
                1
            }
        },
        "errand-config" => {
            let (env, r1) = opt(rest, "--env");
            let (creds, r2) = opt(&r1, "--creds");
            let (home, r3) = opt(&r2, "--home");
            let (out, r4) = opt(&r3, "--out");
            match (env, creds, home, out, r4.is_empty()) {
                (Some(env), Some(creds), Some(home), Some(out), true) => {
                    match errand_config(&env, &creds, &home, &out) {
                        Ok(()) => 0,
                        Err(e) => {
                            eprintln!("errand-box: {e}");
                            1
                        }
                    }
                }
                _ => {
                    eprintln!(
                        "usage: errand-box errand-config --env F --creds F --home DIR --out F"
                    );
                    2
                }
            }
        }
        "__as" => match rest {
            [user, sep, argv @ ..] if sep == "--" && !argv.is_empty() => {
                eprintln!("errand-box __as: {}", procs::exec_as(user, argv));
                1
            }
            _ => 2,
        },
        "__watchdog" => match (rest, runtime::load()) {
            ([name, lease], Ok(rt)) => session::watchdog(&rt, runner, name, lease),
            _ => 2,
        },
        "version" | "--version" => {
            println!("errand-box {}", env!("CARGO_PKG_VERSION"));
            0
        }
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            0
        }
        other => {
            eprintln!("errand-box: unknown command {other:?}\n{USAGE}");
            2
        }
    }
}

/// A read-only report: the host's facts, what is degraded, and whether the
/// pieces setup installs are present and answering.
fn doctor(cfg: &config::Config, runner: &dyn run::Runner) -> Report {
    let mut r = Report::default();
    let h = host::detect();
    r.add("host", St::Ok, h.describe());
    let (ch, note) = h.incus_channel(&cfg.engine.incus);
    r.add(
        "incus line",
        if note.is_some() { St::Degraded } else { St::Ok },
        note.unwrap_or(ch),
    );
    r.add("init", if h.systemd { St::Ok } else { St::Degraded }, if h.systemd { "systemd" } else { "no init: s6 supervises the daemons; run `errand-box up` after every restart (E-01, E-02)" });
    if !h.apparmor {
        r.add("apparmor", St::Degraded, "absent: Incus loses its AppArmor profile; seccomp and isolated uid ranges remain (E-17)");
    }
    if !h.pid1_personality_readable {
        r.add(
            "lxcfs 6",
            St::Degraded,
            "/proc/1/personality is unreadable here, so lxcfs 6 refuses every /proc file (E-08)",
        );
    }
    for d in ["/dev/fuse", "/dev/net/tun"] {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(d) {
            Ok(m) if m.permissions().mode() & 0o777 == 0o666 => r.add(d, St::Ok, "0666"),
            Ok(m) => r.add(
                d,
                St::Degraded,
                format!(
                    "mode {:o}; `errand-box up` sets 0666 (E-03)",
                    m.permissions().mode() & 0o777
                ),
            ),
            Err(_) => r.add(d, St::Degraded, "missing"),
        }
    }
    let lay = layout::detect();
    r.add(
        "incus install",
        if lay.flavor.is_empty() {
            St::Failed
        } else {
            St::Ok
        },
        if lay.flavor.is_empty() {
            "not installed; run `errand-box setup`".to_owned()
        } else {
            format!("{} ({})", lay.incusd, lay.flavor)
        },
    );
    match incus::Incus::new(incus::Endpoint::default(), runner).server_version() {
        Ok(v) => r.add("incus answers", St::Ok, v),
        Err(e) => r.add(
            "incus answers",
            St::Failed,
            format!("{e}; run `errand-box up`"),
        ),
    }
    match runtime::load() {
        Ok(rt) => {
            let inc = incus::Incus::new(rt.endpoint_for_caller(), runner);
            match inc.instance(&rt.golden) {
                Ok(Some(_)) => r.add("golden instance", St::Ok, rt.golden.clone()),
                _ => r.add(
                    "golden instance",
                    St::Failed,
                    format!("{} missing; run `errand-box setup`", rt.golden),
                ),
            }
        }
        Err(e) => r.add("runtime.toml", St::Failed, e),
    }
    let errand_cfg = Path::new(&cfg.errand.home).join(".config/errand/config.json");
    r.add(
        "errand config",
        if errand_cfg.exists() {
            St::Ok
        } else {
            St::Degraded
        },
        if errand_cfg.exists() {
            errand_cfg.display().to_string()
        } else {
            "not written; errand stays stopped".to_owned()
        },
    );
    r
}
