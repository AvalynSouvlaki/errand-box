//! Downloads, each checked against a digest from pins.toml before use.
//! Bytes with a wrong digest are deleted and never run (vendoring rule:
//! refuse unverified bytes).

use crate::run::{Cmd, Runner};
use crate::sha256;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const PINS: &str = include_str!("../pins.toml");

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub url: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub latest: String,
    pub versions: BTreeMap<String, BTreeMap<String, Asset>>,
}

pub fn pins() -> Result<BTreeMap<String, Tool>, String> {
    toml::from_str(PINS).map_err(|e| format!("pins.toml: {}", e.message()))
}

/// The asset of `tool` at `version` (`latest` allowed) for `arch`.
pub fn asset(tool: &str, version: &str, arch: &str) -> Result<(String, Asset), String> {
    let all = pins()?;
    let t = all
        .get(tool)
        .ok_or_else(|| format!("pins.toml has no {tool}"))?;
    let v = if version == "latest" {
        t.latest.clone()
    } else {
        version.to_owned()
    };
    let per_arch = t.versions.get(&v).ok_or_else(|| {
        format!(
            "{tool} {v} is not in pins.toml; pinned: {}",
            t.versions.keys().cloned().collect::<Vec<_>>().join(", ")
        )
    })?;
    let a = per_arch
        .get(arch)
        .ok_or_else(|| format!("{tool} {v} has no asset for {arch}"))?;
    Ok((v, a.clone()))
}

/// Download `a` into `cache` (named by digest) unless a good copy is there.
/// Tries curl, then wget.
pub fn fetch(runner: &dyn Runner, a: &Asset, cache: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(cache)
        .map_err(|e| format!("cannot create {}: {e}", cache.display()))?;
    let dest = cache.join(&a.sha256);
    if dest.exists() && sha256::file_hex(&dest).map_err(|e| e.to_string())? == a.sha256 {
        return Ok(dest);
    }
    let part = cache.join(format!("{}.part", a.sha256));
    let p = part.to_string_lossy().into_owned();
    let tries = [
        Cmd::new("curl").args(["-fsSL", "--retry", "3", "-o", &p, &a.url]),
        Cmd::new("wget").args(["-q", "-O", &p, &a.url]),
    ];
    let mut why = Vec::new();
    for c in tries {
        let c = c.timeout(Duration::from_secs(900));
        let out = runner.run(&c);
        if out.ok() {
            let got = sha256::file_hex(&part).map_err(|e| e.to_string())?;
            if got != a.sha256 {
                let _ = std::fs::remove_file(&part);
                return Err(format!(
                    "{}: digest {got} is not the pinned {}; the bytes were deleted",
                    a.url, a.sha256
                ));
            }
            std::fs::rename(&part, &dest).map_err(|e| e.to_string())?;
            return Ok(dest);
        }
        why.push(format!("{}: {}", c.program, out.why()));
    }
    let _ = std::fs::remove_file(&part);
    Err(format!("could not download {}: {}", a.url, why.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_parse_and_latest_resolves() {
        let all = pins().unwrap();
        for (name, t) in &all {
            assert!(
                t.versions.contains_key(&t.latest),
                "{name}: latest {} is not pinned",
                t.latest
            );
            for per_arch in t.versions.values() {
                for a in per_arch.values() {
                    assert_eq!(a.sha256.len(), 64, "{name}");
                    assert!(
                        a.sha256
                            .chars()
                            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
                    );
                    assert!(a.url.starts_with("https://"));
                }
            }
        }
        let (v, a) = asset("errand", "latest", "x86_64").unwrap();
        assert_eq!(v, all["errand"].latest);
        assert!(a.url.contains("x86_64"));
        assert!(
            asset("errand", "v0.0.0", "x86_64")
                .unwrap_err()
                .contains("not in pins.toml")
        );
        assert!(asset("errand", "latest", "mips").is_err());
    }

    #[test]
    fn a_wrong_digest_is_refused_and_deleted() {
        let dir = std::env::temp_dir().join(format!("eb-fetch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.bin");
        std::fs::write(&src, b"hello").unwrap();
        let a = Asset {
            url: format!("file://{}", src.display()),
            sha256: "0".repeat(64),
        };
        let e = fetch(&crate::run::System, &a, &dir.join("cache")).unwrap_err();
        assert!(
            e.contains("digest") || e.contains("could not download"),
            "{e}"
        );
        assert!(
            !dir.join("cache")
                .join(format!("{}.part", a.sha256))
                .exists()
        );
        let good = Asset {
            sha256: crate::sha256::digest_hex(b"hello"),
            ..a
        };
        if crate::host::which("curl").is_some() {
            let p = fetch(&crate::run::System, &good, &dir.join("cache")).unwrap();
            assert_eq!(std::fs::read(p).unwrap(), b"hello");
        }
    }
}
