//! `errand-box.toml`: what the operator may change. Every key has a default,
//! so an empty file is a valid configuration. Unknown keys are errors, so a
//! misspelt key is not silently ignored.

use serde::Deserialize;
use std::path::Path;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub engine: Engine,
    #[serde(rename = "box")]
    pub boxes: Boxes,
    pub errand: Errand,
    pub kage: Kage,
    pub image: Image,
    pub network: Network,
    pub maintenance: Maintenance,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Engine {
    /// `root`: incusd runs as root and errand reaches it through incus-user.
    /// `rootless`: incusd runs as root of the operator's own user namespace.
    pub mode: String,
    /// `auto`, `lts-6.0` or `stable`. `auto` is `stable` on a cgroup v2
    /// host and `lts-6.0` on a cgroup v1 host, where 7.0 and later refuse to
    /// run (E-04).
    pub incus: String,
    /// The storage pool sessions live on.
    pub pool: String,
    /// `btrfs` (default), `zfs`, or `dir`. `dir` does not enforce the disk
    /// quota (E-05), so setup reports it as degraded.
    pub pool_driver: String,
    /// The size of the loop file behind the pool: `auto` (half the free
    /// space of the filesystem that holds it) or a size such as `200GiB`.
    pub pool_size: String,
}

impl Default for Engine {
    fn default() -> Self {
        Engine {
            mode: "root".into(),
            incus: "auto".into(),
            pool: "errand-box".into(),
            pool_driver: "btrfs".into(),
            pool_size: "auto".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Boxes {
    /// The root disk quota of each session's instance.
    pub disk: String,
    /// The memory limit of each instance: a size, or a percentage of the
    /// host's memory, which Incus reads itself.
    pub memory: String,
    /// The CPU limit of each instance: a count, or a percentage of the host's
    /// CPUs, which errand-box turns into a count of at least one.
    pub cpus: String,
    /// The process (pids cgroup) limit of each instance: a count, or 0 for
    /// no limit. It bounds a fork bomb in one session to that session.
    pub pids: u32,
    /// Stopped instances unused for this many days are deleted by `gc`.
    pub keep_days: u32,
    /// Seconds a stopping session gets for a clean shutdown before it is
    /// forced. Keep it below errand's `sandbox.gracePeriodMs`.
    pub stop_seconds: u64,
}

impl Default for Boxes {
    fn default() -> Self {
        Boxes {
            disk: "20GiB".into(),
            memory: "80%".into(),
            cpus: "80%".into(),
            pids: 8192,
            keep_days: 14,
            stop_seconds: 4,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Errand {
    /// The host account errand runs as. It owns errand's projects and state.
    pub user: String,
    /// Its home. errand's `projectRoot` and `stateDir` live under it, which
    /// is also the only tree its Incus project may attach.
    pub home: String,
    /// `latest` (the newest version in pins.toml) or a version such as
    /// `v0.3.1`.
    pub version: String,
}

impl Default for Errand {
    fn default() -> Self {
        Errand {
            user: "errand".into(),
            home: "/var/lib/errand".into(),
            version: "latest".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Kage {
    pub version: String,
}

impl Default for Kage {
    fn default() -> Self {
        Kage {
            version: "latest".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Image {
    /// The Incus alias of the guest image.
    pub alias: String,
    /// `build`: build it on this host from the Debian 13 image of
    /// images.linuxcontainers.org; `release`: fetch the published image
    /// named in pins.toml and check its digest.
    pub source: String,
    /// Docker Hub mirror for Docker inside the guest (E-16). Empty: none.
    pub registry_mirror: String,
}

impl Default for Image {
    fn default() -> Self {
        Image {
            alias: "errand-box/debian-13".into(),
            source: "build".into(),
            registry_mirror: "https://mirror.gcr.io".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Network {
    /// The host's egress proxy for image builds: `auto` (from
    /// `HTTPS_PROXY`, read on every run, E-13), `none`, or a URL.
    pub proxy: String,
    /// The proxy's CA certificate, when it re-signs TLS. Empty: none.
    pub proxy_ca: String,
    /// The fixed port inside each guest where system services (apt, Docker)
    /// reach errand's broker.
    pub guest_proxy_port: u16,
}

impl Default for Network {
    fn default() -> Self {
        Network {
            proxy: "auto".into(),
            proxy_ca: String::new(),
            guest_proxy_port: 3128,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Maintenance {
    /// Hours between `fstrim` runs on the pool (E-25).
    pub fstrim_hours: u32,
    /// Seconds between checks for sessions whose launcher died (E-27).
    pub reap_seconds: u32,
}

impl Default for Maintenance {
    fn default() -> Self {
        Maintenance {
            fstrim_hours: 24,
            reap_seconds: 60,
        }
    }
}

pub const DEFAULT_PATH: &str = "/etc/errand-box/errand-box.toml";

pub fn parse(text: &str) -> Result<Config, String> {
    let c: Config =
        toml::from_str(text).map_err(|e| format!("errand-box.toml: {}", e.message()))?;
    c.validate()?;
    Ok(c)
}

/// The configuration at `path`. A missing file means the defaults only for
/// the default path: a `--config` that names a missing file is a mistake,
/// not a request for the defaults.
pub fn load(path: &Path) -> Result<Config, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => parse(&t).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && path == Path::new(DEFAULT_PATH) => {
            Ok(Config::default())
        }
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

/// The configuration the last successful setup applied. `up` falls back to
/// it when the live file no longer parses, so a typo does not stop the
/// daemons from coming back after a restart (E-42).
pub const APPLIED_PATH: &str = "/var/lib/errand-box/applied.toml";

/// A size as Incus reads it: a whole number and one of its exact suffixes.
/// kB, MB, GB, TB are powers of 1000; KiB, MiB, GiB, TiB powers of 1024.
/// Incus refuses any other spelling, so the configuration refuses it first.
pub fn incus_bytes(s: &str) -> Result<u64, String> {
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    let n: u64 = digits
        .parse()
        .map_err(|_| format!("not a size: {s:?}; write a number and a unit such as 20GiB"))?;
    let mul: u64 = match &s[digits.len()..] {
        "" | "B" => 1,
        "kB" => 1000,
        "MB" => 1000_u64.pow(2),
        "GB" => 1000_u64.pow(3),
        "TB" => 1000_u64.pow(4),
        "KiB" => 1 << 10,
        "MiB" => 1 << 20,
        "GiB" => 1 << 30,
        "TiB" => 1 << 40,
        other => {
            return Err(format!(
                "unknown size unit {other:?} in {s:?}; Incus takes B, kB, MB, GB, TB, KiB, MiB, GiB or TiB, spelled exactly"
            ));
        }
    };
    n.checked_mul(mul)
        .ok_or_else(|| format!("size out of range: {s:?}"))
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.engine.mode.as_str(), "root" | "rootless") {
            return Err(format!(
                "engine.mode must be root or rootless, not {:?}",
                self.engine.mode
            ));
        }
        if !matches!(self.engine.incus.as_str(), "auto" | "lts-6.0" | "stable") {
            return Err(format!(
                "engine.incus must be auto, lts-6.0 or stable, not {:?}",
                self.engine.incus
            ));
        }
        if !matches!(self.engine.pool_driver.as_str(), "btrfs" | "zfs" | "dir") {
            return Err(format!(
                "engine.pool_driver must be btrfs, zfs or dir, not {:?}",
                self.engine.pool_driver
            ));
        }
        if self.engine.pool_size != "auto" {
            incus_bytes(&self.engine.pool_size).map_err(|e| format!("engine.pool_size: {e}"))?;
        }
        let disk = incus_bytes(&self.boxes.disk).map_err(|e| format!("box.disk: {e}"))?;
        if disk < 4 << 30 {
            return Err(format!(
                "box.disk is {}; the guest image alone needs about 2 GiB, so use at least 4GiB (E-21)",
                self.boxes.disk
            ));
        }
        percent_or(&self.boxes.memory, incus_bytes).map_err(|e| format!("box.memory: {e}"))?;
        percent_or(&self.boxes.cpus, |s| {
            s.parse::<u64>().map_err(|e| e.to_string())
        })
        .map_err(|e| format!("box.cpus: {e}"))?;
        if !matches!(self.image.source.as_str(), "build" | "release") {
            return Err(format!(
                "image.source must be build or release, not {:?}",
                self.image.source
            ));
        }
        if !valid_name(&self.errand.user) || self.errand.user == "root" {
            return Err(format!(
                "errand.user is not a valid unprivileged account name: {:?}",
                self.errand.user
            ));
        }
        let home = Path::new(&self.errand.home);
        if !home.is_absolute() || home.components().count() < 2 {
            return Err(format!(
                "errand.home must be an absolute path below the root, not {:?}; setup chowns it to the errand account and gc reads below it",
                self.errand.home
            ));
        }
        for bad in [
            "/", "/root", "/etc", "/usr", "/var", "/boot", "/bin", "/sbin", "/lib", "/dev",
            "/proc", "/sys",
        ] {
            if home == Path::new(bad) {
                return Err(format!(
                    "errand.home must not be {bad}: setup chowns it to the errand account"
                ));
            }
        }
        Ok(())
    }

    /// The CPU limit as a count, for a host with `host_cpus` CPUs.
    pub fn box_cpus(&self, host_cpus: u64) -> u64 {
        match self.boxes.cpus.strip_suffix('%') {
            Some(p) => (host_cpus * p.trim().parse::<u64>().unwrap_or(80) / 100).max(1),
            None => self
                .boxes
                .cpus
                .parse::<u64>()
                .unwrap_or(1)
                .clamp(1, host_cpus.max(1)),
        }
    }
}

fn percent_or(s: &str, other: impl Fn(&str) -> Result<u64, String>) -> Result<(), String> {
    if let Some(p) = s.strip_suffix('%') {
        let n: u64 = p
            .trim()
            .parse()
            .map_err(|_| format!("not a percentage: {s:?}"))?;
        if n == 0 || n > 100 {
            return Err(format!("a percentage must be 1 to 100: {s:?}"));
        }
        return Ok(());
    }
    other(s).map(|_| ())
}

fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 32
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && !n.starts_with('-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_spelled_as_incus_spells_them() {
        assert_eq!(incus_bytes("20GiB"), Ok(20 << 30));
        assert_eq!(incus_bytes("20GB"), Ok(20_000_000_000));
        assert_eq!(incus_bytes("512MiB"), Ok(512 << 20));
        assert_eq!(incus_bytes("4096"), Ok(4096));
        for bad in ["20gib", "20G", "20 GiB", "GiB", "20gb", "-1GiB"] {
            assert!(incus_bytes(bad).is_err(), "{bad}");
        }
        assert!(parse("[box]\ndisk = \"20gib\"\n").is_err());
    }

    #[test]
    fn a_named_config_file_must_exist() {
        let missing = Path::new("/nonexistent/errand-box.toml");
        assert!(load(missing).unwrap_err().contains("cannot read"));
    }

    #[test]
    fn empty_file_is_the_defaults() {
        let c = parse("").unwrap();
        assert_eq!(c, Config::default());
        assert_eq!(c.boxes.disk, "20GiB");
        assert_eq!(c.engine.incus, "auto");
    }

    #[test]
    fn unknown_keys_are_errors() {
        assert!(
            parse("[box]\ndisks = \"10GiB\"\n")
                .unwrap_err()
                .contains("disks")
        );
        assert!(parse("[nope]\n").is_err());
    }

    #[test]
    fn a_disk_quota_below_the_image_is_refused() {
        assert!(
            parse("[box]\ndisk = \"1GiB\"\n")
                .unwrap_err()
                .contains("E-21")
        );
    }

    #[test]
    fn cpus_from_a_percentage() {
        let c = Config::default();
        assert_eq!(c.box_cpus(4), 3);
        assert_eq!(c.box_cpus(1), 1);
        let c = parse("[box]\ncpus = \"8\"\n").unwrap();
        assert_eq!(c.box_cpus(4), 4);
    }

    #[test]
    fn bad_values_are_refused() {
        assert!(parse("[engine]\nmode = \"magic\"\n").is_err());
        assert!(parse("[box]\nmemory = \"0%\"\n").is_err());
        assert!(parse("[box]\nmemory = \"lots\"\n").is_err());
        assert!(parse("[errand]\nhome = \"rel\"\n").is_err());
        assert!(parse("[errand]\nuser = \"Root\"\n").is_err());
        assert!(parse("[errand]\nuser = \"root\"\n").is_err());
        assert!(parse("[errand]\nhome = \"/\"\n").is_err());
        assert!(parse("[errand]\nhome = \"/root\"\n").is_err());
        assert!(parse("[errand]\nhome = \"/var/lib/errand\"\n").is_ok());
    }
}
