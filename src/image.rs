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
            env.insert("EB_PROXY".into(), format!("http://{listen}"));
            r.add(
                "build proxy",
                St::Changed,
                format!("relay {listen} -> {auth} for this build (E-10)"),
            );
        } else {
            env.insert("EB_PROXY".into(), u.clone());
        }
    }

    let name = format!("eb-build-{}", std::process::id());
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
        return Err(format!("cannot launch {BASE}: {}", out.why()));
    }
    let result = provision(&inc, &name, &kage, cfg, &env);
    if let Err(e) = result {
        let _ = inc.delete(&name);
        return Err(e);
    }
    inc.stop(&name, 20)?;
    let desc = format!("description=errand-box guest, Debian 13, kage {kv}");
    let inputs = format!("{INPUTS_KEY}={}", inputs_digest(cfg, &kv));
    let old = alias_target(&inc, &cfg.image.alias);
    let _ = inc.run(["image", "alias", "delete", &cfg.image.alias]);
    let out = inc.runner.run(
        &inc.cmd([
            "publish",
            "-q",
            &name,
            "--alias",
            &cfg.image.alias,
            &desc,
            &inputs,
        ])
        .timeout(Duration::from_secs(1800)),
    );
    let _ = inc.delete(&name);
    if !out.ok() {
        return Err(format!("cannot publish the image: {}", out.why()));
    }
    // The image this one replaces; instances made from it keep their own
    // volumes.
    if let Some(o) =
        old.filter(|o| alias_target(&inc, &cfg.image.alias).as_deref() != Some(o.as_str()))
    {
        let _ = inc.run(["image", "delete", &o]);
    }
    r.add(
        "guest image",
        St::Changed,
        format!("built {} in {} s", cfg.image.alias, t0.elapsed().as_secs()),
    );
    Ok(())
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
            return Err(format!("{name} got no network on {BUILD_NET}"));
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
    let mut args: Vec<String> = vec!["exec".into(), name.into()];
    for (k, v) in env {
        args.push("--env".into());
        args.push(format!("{k}={v}"));
    }
    args.extend([
        "--".into(),
        "/bin/sh".into(),
        "/run/eb-build/provision.sh".into(),
    ]);
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
