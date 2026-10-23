//! Where the installed Incus keeps its programs. The Zabbly packages install
//! under /opt/incus; distributions use their own paths. errand-box reads the
//! filesystem instead of assuming one.

use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    /// `zabbly`, `distro`, or empty when no incusd was found.
    pub flavor: String,
    /// The daemon program (for Zabbly, the wrapper that sets its paths).
    pub incusd: String,
    pub incus_user: String,
    /// The lxcfs program and the environment it needs.
    pub lxcfs: String,
    pub lxcfs_env: Vec<(String, String)>,
    /// Where the installed LXC mount hook expects lxcfs (E-07).
    pub lxcfs_mount: String,
}

fn first(cands: &[&str]) -> String {
    cands
        .iter()
        .find(|p| Path::new(p).exists())
        .map(|s| (*s).to_owned())
        .unwrap_or_default()
}

pub fn detect() -> Layout {
    if Path::new("/opt/incus/lib/systemd/incusd").exists() {
        return Layout {
            flavor: "zabbly".into(),
            incusd: "/opt/incus/lib/systemd/incusd".into(),
            incus_user: first(&["/opt/incus/lib/systemd/incus-user"]),
            lxcfs: first(&["/opt/incus/bin/lxcfs"]),
            lxcfs_env: vec![("LD_LIBRARY_PATH".into(), "/opt/incus/lib".into())],
            lxcfs_mount: hook_mount(&["/opt/incus/share/lxcfs/lxc.mount.hook"]),
        };
    }
    let incusd = first(&[
        "/usr/libexec/incus/incusd",
        "/usr/lib/incus/incusd",
        "/usr/sbin/incusd",
        "/usr/bin/incusd",
    ]);
    if incusd.is_empty() {
        return Layout::default();
    }
    Layout {
        flavor: "distro".into(),
        incusd,
        incus_user: first(&[
            "/usr/libexec/incus/incus-user",
            "/usr/lib/incus/incus-user",
            "/usr/sbin/incus-user",
            "/usr/bin/incus-user",
        ]),
        lxcfs: first(&["/usr/bin/lxcfs", "/usr/sbin/lxcfs"]),
        lxcfs_env: Vec::new(),
        lxcfs_mount: hook_mount(&[
            "/usr/share/lxcfs/lxc.mount.hook",
            "/usr/share/lxc/hooks/lxcfs",
        ]),
    }
}

/// The lxcfs directory a mount hook binds into guests. Ubuntu and Debian
/// hooks read /var/lib/lxcfs; the Zabbly hook reads /var/lib/incus-lxcfs.
pub fn hook_mount(hooks: &[&str]) -> String {
    for h in hooks {
        if let Ok(text) = std::fs::read_to_string(h) {
            return mount_from_hook(&text);
        }
    }
    "/var/lib/lxcfs".into()
}

pub fn mount_from_hook(text: &str) -> String {
    if text.contains("/var/lib/incus-lxcfs/proc") {
        "/var/lib/incus-lxcfs".into()
    } else {
        "/var/lib/lxcfs".into()
    }
}

/// The major version of an lxcfs program, from `lxcfs --version`.
pub fn lxcfs_major(version_output: &str) -> Option<u32> {
    let v = version_output.trim().trim_matches('"');
    v.split('.').next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_paths() {
        assert_eq!(
            mount_from_hook("if [ -d /var/lib/incus-lxcfs/proc/ ]; then"),
            "/var/lib/incus-lxcfs"
        );
        assert_eq!(
            mount_from_hook("if [ -d /var/lib/lxcfs/proc/ ]; then"),
            "/var/lib/lxcfs"
        );
    }

    #[test]
    fn lxcfs_versions() {
        assert_eq!(lxcfs_major("\"6.0.6\"\n"), Some(6));
        assert_eq!(lxcfs_major("5.0.4"), Some(5));
        assert_eq!(lxcfs_major("garbage"), None);
    }
}
