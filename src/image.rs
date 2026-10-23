//! `errand-box image build`: make the guest image on this host.
//!
//! It runs image/provision.sh (embedded in this program) in a fresh
//! images:debian/13 instance on a build-only bridge, then publishes the
//! result under the configured alias. Session instances never get a network
//! device; only this build instance does.

use crate::config::Config;
use crate::fetch;
use crate::host::Host;
use crate::incus::{Endpoint, Incus};
use crate::report::{Report, St};
use crate::run::Runner;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub const PROVISION: &str = include_str!("../image/provision.sh");
pub const LAUNCH: &str = include_str!("../image/launch");
pub const GUEST_INIT: &str = include_str!("../image/guest-init");
// At most 15 characters: it is also the name of the bridge interface.
pub const BUILD_NET: &str = "ebbuild0";
pub const BASE: &str = "images:debian/13";
/// The image property that records `inputs_digest` (E-31).
pub const INPUTS_KEY: &str = "errand-box.inputs";

/// A digest of everything `build` puts into the image: the embedded guest
/// files, the kage version and the settings provision.sh reads. setup
/// rebuilds the image when the published one records another.
pub fn inputs_digest(cfg: &Config, kage_version: &str) -> String {
    let port = cfg.network.guest_proxy_port.to_string();
    let parts = [
        PROVISION,
        LAUNCH,
        GUEST_INIT,
        kage_version,
        &cfg.image.registry_mirror,
        &port,
    ];
    let mut s = String::new();
    for p in parts {
        s.push_str(p);
        s.push('\0');
    }
    crate::sha256::digest_hex(s.as_bytes())
}

/// The inputs digest the image `fingerprint` records, if any.
pub fn recorded_inputs(inc: &Incus, fingerprint: &str) -> Option<String> {
    let v = inc.query(&format!("/1.0/images/{fingerprint}")).ok()?;
    v.pointer(&format!("/properties/{INPUTS_KEY}"))?
        .as_str()
        .map(str::to_owned)
}

pub fn alias_target(inc: &Incus, alias: &str) -> Option<String> {
    inc.query(&format!("/1.0/images/aliases/{alias}"))
        .ok()?
        .get("target")?
        .as_str()
        .map(str::to_owned)
}

/// True for a proxy authority on the host's loopback, which a guest cannot
/// reach without a relay (E-10).
pub fn is_loopback(authority: &str) -> bool {
    let host = authority
        .rsplit_once(':')
        .map(|(h, _)| h)
        .unwrap_or(authority);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host == "localhost" || host == "::1" || host.starts_with("127.")
}

fn bridge_address(inc: &Incus) -> Result<String, String> {
    let v = inc.query(&format!("/1.0/networks/{BUILD_NET}"))?;
    let cidr = v
        .pointer("/config/ipv4.address")
        .and_then(|x| x.as_str())
        .ok_or("the build bridge has no IPv4 address")?;
    Ok(cidr.split('/').next().unwrap_or(cidr).to_owned())
}

fn free_port(ip: &str) -> Result<u16, String> {
    let l =
        std::net::TcpListener::bind((ip, 0)).map_err(|e| format!("cannot bind on {ip}: {e}"))?;
    Ok(l.local_addr().map_err(|e| e.to_string())?.port())
}

pub fn build(
    cfg: &Config,
    host: &Host,
    runner: &dyn Runner,
    r: &mut Report,
    cache: &std::path::Path,
) -> Result<(), String> {
    let inc = Incus::new(Endpoint::default(), runner);
    let (kv, asset) = fetch::asset("kage", &cfg.kage.version, &host.arch)?;
    let kage = fetch::fetch(runner, &asset, cache)?;
    r.add(
        "kage for the guest",
        St::Ok,
        format!("{kv}, sha256 {} matches pins.toml", &asset.sha256[..12]),
    );

    if inc.query(&format!("/1.0/networks/{BUILD_NET}")).is_err() {
        let out = inc.run([
            "network",
            "create",
            BUILD_NET,
            "ipv4.address=auto",
            "ipv4.nat=true",
            "ipv6.address=none",
        ]);
        if !out.ok() {
            return Err(format!("cannot create the build bridge: {}", out.why()));
        }
        r.add("build bridge", St::Changed, format!("created {BUILD_NET}"));
    }
    let bridge = bridge_address(&inc)?;

    let upstream = match cfg.network.proxy.as_str() {
        "none" => None,
        "auto" => crate::run::env("HTTPS_PROXY").or_else(|| crate::run::env("https_proxy")),
        u => Some(u.to_owned()),
    };
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    env.insert(
        "EB_GUEST_PROXY_PORT".into(),
        cfg.network.guest_proxy_port.to_string(),
    );
    if !cfg.image.registry_mirror.is_empty() {
        env.insert(
            "EB_REGISTRY_MIRROR".into(),
            cfg.image.registry_mirror.clone(),
        );
    }
    if let Some(u) = &upstream {
        let auth = crate::relay::proxy_authority(u)
            .ok_or_else(|| format!("cannot read host and port from the proxy {u:?}"))?;
        if is_loopback(&auth) {
            let port = free_port(&bridge)?;
            let listen = format!("{bridge}:{port}");
            let target = auth.clone();
            let l2 = listen.clone();
            std::thread::spawn(move || {
                let _ = crate::relay::tcp_to_tcp(&l2, &target);
            });
            env.insert("EB_PROXY".into(), crate::relay::with_authority(u, &listen));
            r.add(
                "build proxy",
                St::Changed,
                format!("relay {listen} -> {auth} for this build (E-10)"),
            );
        } else {
            env.insert("EB_PROXY".into(), u.clone());
        }
    }

    sweep_builds(&inc, r)?;
    let name = format!("{BUILD_PREFIX}{}", std::process::id());
    let _ = inc.delete(&name);
    let t0 = Instant::now();
    let out = inc.runner.run(
        &inc.cmd([
            "launch",
            "-q",
            BASE,
            &name,
            "--network",
            BUILD_NET,
            "--storage",
            &cfg.engine.pool,
            "-c",
            "security.nesting=true",
        ])
        .timeout(Duration::from_secs(1200)),
    );
    if !out.ok() {
        let _ = inc.delete(&name);
        return Err(format!("cannot launch {BASE}: {}", out.why()));
    }
    // The build instance goes whatever happens below.
    let res = provision(&inc, &name, &kage, cfg, &env)
        .and_then(|()| inc.stop(&name, 20))
        .and_then(|()| publish(&inc, &name, cfg, &kv));
    let _ = inc.delete(&name);
    res?;
    r.add(
        "guest image",
        St::Changed,
        format!("built {} in {} s", cfg.image.alias, t0.elapsed().as_secs()),
    );
    Ok(())
}

/// Build instances are named this, then the pid of the errand-box that
/// made them.
pub const BUILD_PREFIX: &str = "eb-build-";

/// The pid in a build instance's name, if that process is an errand-box
/// that still runs.
fn live_builder(name: &str) -> Option<i32> {
    let pid: i32 = name.strip_prefix(BUILD_PREFIX)?.parse().ok()?;
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    (comm.trim() == "errand-box").then_some(pid)
}

/// Delete the build instances an interrupted build left (E-39), and refuse
/// to start while another build runs.
fn sweep_builds(inc: &Incus, r: &mut Report) -> Result<(), String> {
    let mut gone = Vec::new();
    for i in inc.instances(BUILD_PREFIX)? {
        if let Some(pid) = live_builder(&i.name).filter(|p| *p != std::process::id() as i32) {
            return Err(format!(
                "another image build (pid {pid}, instance {}) is running",
                i.name
            ));
        }
        if inc.delete(&i.name).is_ok() {
            gone.push(i.name);
        }
    }
    if !gone.is_empty() {
        r.add(
            "old build instances",
            St::Changed,
            format!("deleted {} (E-39)", gone.join(", ")),
        );
    }
    Ok(())
}

/// Publish the stopped build instance as the image, then move the alias to
/// it and delete the image it replaces. The old image keeps the alias until
/// the new one exists, so a failed publish leaves the host as it was.
fn publish(inc: &Incus, name: &str, cfg: &Config, kv: &str) -> Result<(), String> {
    let desc = format!("description=errand-box guest, Debian 13, kage {kv}");
    let inputs = format!("{INPUTS_KEY}={}", inputs_digest(cfg, kv));
    let old = alias_target(inc, &cfg.image.alias);
    let out = inc.runner.run(
        &inc.cmd(["publish", name, &desc, &inputs])
            .timeout(Duration::from_secs(1800)),
    );
    let fp = published_fingerprint(&out.stdout)
        .ok_or_else(|| format!("cannot publish the image: {}", out.why()))?;
    inc.move_alias(&cfg.image.alias, &fp).map_err(|e| {
        format!(
            "published {fp}, but cannot point {} at it: {e}",
            cfg.image.alias
        )
    })?;
    // Instances made from the replaced image keep their own volumes.
    if let Some(o) = old.filter(|o| *o != fp) {
        let _ = inc.run(["image", "delete", &o]);
    }
    Ok(())
}

/// The build settings as `KEY='value'` lines for sh. A value that cannot
/// sit in single quotes is refused.
pub fn build_env_file(env: &BTreeMap<String, String>) -> Result<String, String> {
    let mut s = String::new();
    for (k, v) in env {
        if v.contains('\'') || v.contains('\n') {
            return Err(format!("{k} has a quote or a newline"));
        }
        s.push_str(&format!("{k}='{v}'\n"));
    }
    Ok(s)
}

/// The fingerprint `incus publish` prints.
pub fn published_fingerprint(stdout: &str) -> Option<String> {
    let fp = stdout
        .lines()
        .find_map(|l| l.split("fingerprint:").nth(1))?
        .trim();
    (fp.len() >= 12 && fp.chars().all(|c| c.is_ascii_hexdigit())).then(|| fp.to_owned())
}

fn provision(
    inc: &Incus,
    name: &str,
    kage: &std::path::Path,
    cfg: &Config,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    // Wait for the guest's network: provision.sh needs it for apt.
    let t = Instant::now();
    loop {
        let out = inc.exec(name, &["sh", "-c", "ip -4 route show default | grep -q ."]);
        if out.ok() {
            break;
        }
        if t.elapsed() > Duration::from_secs(120) {
            return Err(format!(
                "{name} got no network on {BUILD_NET} in 120 s; a firewall that drops forwarded traffic blocks the build bridge: Docker sets the FORWARD policy to DROP, and ufw denies routing by default (E-47)"
            ));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let push =
        |path: &str, bytes: Vec<u8>, mode: &str| inc.file_push(name, path, bytes, mode, 0, 0);
    let _ = inc.exec(name, &["mkdir", "-p", "/run/eb-build"]);
    push(
        "/run/eb-build/provision.sh",
        PROVISION.as_bytes().to_vec(),
        "0755",
    )?;
    push("/run/eb-build/launch", LAUNCH.as_bytes().to_vec(), "0755")?;
    push(
        "/run/eb-build/guest-init",
        GUEST_INIT.as_bytes().to_vec(),
        "0755",
    )?;
    push(
        "/run/eb-build/kage.tar.xz",
        std::fs::read(kage).map_err(|e| e.to_string())?,
        "0644",
    )?;
    let ca = if cfg.network.proxy_ca.is_empty() {
        crate::run::env("PROXY_CA").unwrap_or_default()
    } else {
        cfg.network.proxy_ca.clone()
    };
    if !ca.is_empty() && env.contains_key("EB_PROXY") {
        let bytes =
            std::fs::read(&ca).map_err(|e| format!("cannot read the proxy CA {ca}: {e}"))?;
        push("/run/eb-build/proxy-ca.crt", bytes, "0644")?;
    }
    // The settings go in a file, not on the argv of `incus exec`, where any
    // local user could read a proxy's credentials (E-46). The distrobuilder
    // build reads the same file.
    push(
        "/run/eb-build/env",
        build_env_file(env)?.into_bytes(),
        "0600",
    )?;
    let args: Vec<String> = [
        "exec",
        name,
        "--",
        "/bin/sh",
        "-c",
        "set -a; . /run/eb-build/env; set +a; exec /bin/sh /run/eb-build/provision.sh",
    ]
    .map(str::to_owned)
    .to_vec();
    let out = inc
        .runner
        .run(&inc.cmd(args).timeout(Duration::from_secs(3600)));
    if !out.ok() {
        let all = format!("{}\n{}", out.stdout, out.stderr);
        let mut tail: Vec<&str> = all.lines().rev().take(15).collect();
        tail.reverse();
        return Err(format!("provision.sh failed:\n{}", tail.join("\n")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_proxies_need_a_relay() {
        assert!(is_loopback("127.0.0.1:45823"));
        assert!(is_loopback("localhost:3128"));
        assert!(is_loopback("[::1]:3128"));
        assert!(!is_loopback("10.0.0.5:3128"));
        assert!(!is_loopback("proxy.corp:8080"));
    }

    #[test]
    fn the_distrobuilder_build_records_the_same_inputs() {
        let root = env!("CARGO_MANIFEST_DIR");
        let out = std::process::Command::new("sh")
            .arg(format!("{root}/scripts/build-image.sh"))
            .arg("--print-inputs")
            .env("KAGE_VERSION", "v0.5.0")
            .env_remove("EB_REGISTRY_MIRROR")
            .env_remove("EB_GUEST_PROXY_PORT")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let cfg = crate::config::parse("").unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            inputs_digest(&cfg, "v0.5.0")
        );
    }

    #[test]
    fn build_settings_are_a_file() {
        let mut env = BTreeMap::new();
        env.insert("EB_PROXY".to_owned(), "http://u:p@10.0.0.1:3128".to_owned());
        env.insert("EB_GUEST_PROXY_PORT".to_owned(), "3128".to_owned());
        assert_eq!(
            build_env_file(&env).unwrap(),
            "EB_GUEST_PROXY_PORT='3128'\nEB_PROXY='http://u:p@10.0.0.1:3128'\n"
        );
        env.insert("X".to_owned(), "a'b".to_owned());
        assert!(build_env_file(&env).is_err());
    }

    #[test]
    fn the_published_fingerprint_is_read() {
        let fp = "3ee5b614605f".to_owned() + &"ab".repeat(26);
        assert_eq!(
            published_fingerprint(&format!("Instance published with fingerprint: {fp}\n"))
                .as_deref(),
            Some(fp.as_str())
        );
        assert_eq!(published_fingerprint("Error: nope\n"), None);
        assert_eq!(published_fingerprint("fingerprint: zz\n"), None);
    }

    #[test]
    fn embedded_files_are_scripts() {
        for f in [PROVISION, LAUNCH, GUEST_INIT] {
            assert!(f.starts_with("#!/bin/sh\n"));
        }
        assert!(LAUNCH.contains(crate::session::ENV_FILE));
        assert!(PROVISION.contains(crate::session::LAUNCHER));
    }

    #[test]
    fn the_inputs_digest_follows_every_input() {
        let a = crate::config::parse("").unwrap();
        let mut b = a.clone();
        b.image.registry_mirror = "https://mirror.example".into();
        let mut c = a.clone();
        c.network.guest_proxy_port += 1;
        let d = inputs_digest(&a, "v0.5.0");
        assert_eq!(d.len(), 64);
        assert_eq!(d, inputs_digest(&a, "v0.5.0"));
        for other in [
            inputs_digest(&a, "v0.5.1"),
            inputs_digest(&b, "v0.5.0"),
            inputs_digest(&c, "v0.5.0"),
        ] {
            assert_ne!(d, other);
        }
    }
}
