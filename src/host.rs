//! What the host is, measured rather than assumed. Every fact here is read
//! from the running system, so `doctor` and `setup` report what is true on
//! the day they run.

use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Host {
    pub arch: String,
    pub os_id: String,
    pub os_like: String,
    pub os_codename: String,
    /// The Ubuntu or Debian release a derivative is built on
    /// (UBUNTU_CODENAME, DEBIAN_CODENAME), when os-release names one.
    pub base_codenames: Vec<String>,
    /// `glibc` or `musl`, from the dynamic loader present.
    pub libc: String,
    /// `apt`, `apk`, `dnf`, `pacman`, `zypper` or empty.
    pub pkg: String,
    /// True when PID 1 is systemd.
    pub systemd: bool,
    /// True when OpenRC brought the host up (it leaves /run/openrc). Its
    /// `local` service then runs `errand-box up` at boot (E-36).
    pub openrc: bool,
    pub cgroup_v2: bool,
    pub kvm: bool,
    pub wsl: bool,
    pub apparmor: bool,
    /// True when root may lower an oom score (CAP_SYS_RESOURCE in CapEff).
    pub cap_sys_resource: bool,
    /// False when `/proc/1/personality` cannot be read, which makes lxcfs 6
    /// refuse every /proc file (E-08).
    pub pid1_personality_readable: bool,
    pub cpus: u64,
    pub mem_bytes: u64,
}

fn read(p: &str) -> String {
    fs::read_to_string(p).unwrap_or_default()
}

fn os_release_field(text: &str, key: &str) -> String {
    text.lines()
        .find_map(|l| l.strip_prefix(&format!("{key}=")))
        .map(|v| v.trim().trim_matches('"').to_owned())
        .unwrap_or_default()
}

pub fn detect() -> Host {
    let osr = read("/etc/os-release");
    let status = read("/proc/self/status");
    let cap_eff = status
        .lines()
        .find_map(|l| l.strip_prefix("CapEff:"))
        .and_then(|v| u64::from_str_radix(v.trim(), 16).ok())
        .unwrap_or(0);
    Host {
        arch: std::env::consts::ARCH.to_owned(),
        os_id: os_release_field(&osr, "ID"),
        os_like: os_release_field(&osr, "ID_LIKE"),
        os_codename: os_release_field(&osr, "VERSION_CODENAME"),
        base_codenames: ["UBUNTU_CODENAME", "DEBIAN_CODENAME"]
            .iter()
            .map(|k| os_release_field(&osr, k))
            .filter(|c| !c.is_empty())
            .collect(),
        libc: detect_libc(),
        pkg: detect_pkg(),
        systemd: read("/proc/1/comm").trim() == "systemd",
        openrc: read("/proc/1/comm").trim() != "systemd" && Path::new("/run/openrc").is_dir(),
        cgroup_v2: Path::new("/sys/fs/cgroup/cgroup.controllers").exists(),
        kvm: Path::new("/dev/kvm").exists(),
        wsl: {
            let v = read("/proc/version").to_ascii_lowercase();
            v.contains("microsoft") || v.contains("wsl")
        },
        apparmor: read("/sys/module/apparmor/parameters/enabled").trim() == "Y",
        cap_sys_resource: cap_eff & (1 << 24) != 0,
        pid1_personality_readable: fs::read("/proc/1/personality").is_ok(),
        cpus: std::thread::available_parallelism()
            .map(|n| n.get() as u64)
            .unwrap_or(1),
        mem_bytes: read("/proc/meminfo")
            .lines()
            .find_map(|l| l.strip_prefix("MemTotal:"))
            .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
            .map(|kb| kb * 1024)
            .unwrap_or(0),
    }
}

fn detect_libc() -> String {
    let musl = fs::read_dir("/lib")
        .map(|d| {
            d.flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("ld-musl-"))
        })
        .unwrap_or(false);
    if musl { "musl".into() } else { "glibc".into() }
}

fn detect_pkg() -> String {
    for (bin, name) in [
        ("apt-get", "apt"),
        ("apk", "apk"),
        ("dnf", "dnf"),
        ("pacman", "pacman"),
        ("zypper", "zypper"),
    ] {
        if which(bin).is_some() {
            return name.into();
        }
    }
    String::new()
}

/// The first executable named `name` on PATH.
pub fn which(name: &str) -> Option<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join(name);
        if let Ok(md) = fs::metadata(&p)
            && md.is_file()
            && md.permissions().mode() & 0o111 != 0
        {
            return Some(p);
        }
    }
    None
}

impl Host {
    /// The Incus line to install: `stable` where it can run, else the 6.0
    /// LTS line, which is the last one that accepts cgroup v1 (E-04).
    pub fn incus_channel(&self, wanted: &str) -> (String, Option<String>) {
        match wanted {
            "auto" if self.cgroup_v2 => ("stable".into(), None),
            "auto" => (
                "lts-6.0".into(),
                Some("cgroup v1 host: Incus 7.0 and later refuse cgroup v1, so the 6.0 LTS line is pinned (supported until June 2029)".into()),
            ),
            "stable" if !self.cgroup_v2 => (
                "lts-6.0".into(),
                Some("engine.incus = stable was asked for, but this is a cgroup v1 host; stable refuses to run here, so 6.0 LTS is used".into()),
            ),
            other => (other.to_owned(), None),
        }
    }

    /// The Debian-family codename for the Zabbly repository, if this host
    /// has one Zabbly builds for.
    pub fn zabbly_suite(&self) -> Option<&str> {
        let ok = ["bookworm", "trixie", "jammy", "noble", "plucky", "questing"];
        self.suite_candidates().into_iter().find(|c| ok.contains(c))
    }

    /// The codenames a Zabbly suite may be named after, most specific
    /// first: the host's own, then the release a derivative is built on
    /// (Linux Mint and Pop!_OS name Ubuntu's in UBUNTU_CODENAME).
    pub fn suite_candidates(&self) -> Vec<&str> {
        let mut v = Vec::new();
        for c in std::iter::once(&self.os_codename).chain(&self.base_codenames) {
            if !c.is_empty() && !v.contains(&c.as_str()) {
                v.push(c.as_str());
            }
        }
        v
    }

    /// `systemd`, `openrc`, or `none`: what starts errand-box at boot.
    pub fn init(&self) -> &'static str {
        if self.systemd {
            "systemd"
        } else if self.openrc {
            "openrc"
        } else {
            "none"
        }
    }

    pub fn describe(&self) -> String {
        format!(
            "{} {} ({}), {}, init {}, cgroup {}, kvm {}, wsl {}, {} CPUs, {} MiB",
            self.os_id,
            self.os_codename,
            self.arch,
            self.libc,
            self.init(),
            if self.cgroup_v2 { "v2" } else { "v1" },
            yes(self.kvm),
            yes(self.wsl),
            self.cpus,
            self.mem_bytes >> 20
        )
    }
}

pub fn yes(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_derivative_gets_the_suite_of_its_base() {
        let mint = Host {
            os_codename: "wilma".into(),
            base_codenames: vec!["noble".into()],
            ..Host::default()
        };
        assert_eq!(mint.zabbly_suite(), Some("noble"));
        assert_eq!(mint.suite_candidates(), vec!["wilma", "noble"]);
        let future = Host {
            os_codename: "zesty-zebra".into(),
            ..Host::default()
        };
        assert_eq!(future.zabbly_suite(), None);
        assert_eq!(future.suite_candidates(), vec!["zesty-zebra"]);
    }

    #[test]
    fn channel_follows_cgroup_version() {
        let v1 = Host::default();
        let v2 = Host {
            cgroup_v2: true,
            ..Host::default()
        };
        assert_eq!(v2.incus_channel("auto").0, "stable");
        let (c, note) = v1.incus_channel("auto");
        assert_eq!(c, "lts-6.0");
        assert!(note.unwrap().contains("cgroup v1"));
        assert_eq!(v1.incus_channel("stable").0, "lts-6.0");
        assert_eq!(v2.incus_channel("lts-6.0").0, "lts-6.0");
    }

    #[test]
    fn os_release_parsing() {
        let t = "ID=ubuntu\nVERSION_CODENAME=noble\nID_LIKE=\"debian\"\n";
        assert_eq!(os_release_field(t, "ID"), "ubuntu");
        assert_eq!(os_release_field(t, "ID_LIKE"), "debian");
        assert_eq!(os_release_field(t, "VERSION_CODENAME"), "noble");
        assert_eq!(os_release_field(t, "NOPE"), "");
    }

    #[test]
    fn detect_runs_here() {
        let h = detect();
        assert!(h.cpus >= 1);
        assert!(!h.arch.is_empty());
    }
}
