//! `/etc/errand-box/runtime.toml`: what `setup` decided, for the bailey shim.
//!
//! errand runs the shim with a cleared environment, so the shim cannot take
//! its settings from the environment. setup writes this file once; the shim
//! reads it on every call.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Unknown keys are ignored, not refused: a file a newer errand-box wrote
/// must still work when the program is rolled back.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Runtime {
    /// `root` or `rootless`.
    pub mode: String,
    pub incus_bin: String,
    /// `INCUS_DIR` of the rootless daemon; empty for root mode.
    #[serde(default)]
    pub incus_dir: String,
    /// The project the shim names on every call. Under incus-user it is the
    /// account's own, `user-<uid>`, the only one its certificate may use.
    #[serde(default)]
    pub project: String,
    /// The stopped, never-started instance every session is copied from.
    pub golden: String,
    /// The uid and gid the agent runs as inside the guest.
    pub guest_uid: u32,
    pub guest_gid: u32,
    /// Seconds for a clean stop before a forced one.
    pub stop_seconds: u64,
    /// Where system services inside the guest reach the broker.
    pub guest_proxy_port: u16,
    /// `tcp`: the proxy device dials host 127.0.0.1 itself (root mode).
    /// `unix`: the shim relays through a unix socket, because a rootless
    /// incusd lives in a network namespace of its own (E-23).
    pub broker_link: String,
    /// Host directories errand grants as the agent's runtime (the host
    /// kage); the guest has its own, so these grants are not attached.
    #[serde(default)]
    pub host_runtime_dirs: Vec<String>,
    /// The project as root sees it, for `maint` and `reap` run as root.
    #[serde(default)]
    pub admin_project: String,
    /// errand's `stateDir`; `gc` deletes an instance whose session
    /// directory is gone.
    #[serde(default)]
    pub state_root: String,
    /// The session locks. The launcher, its watchdog and root's `reap` must
    /// lock the same file, so it is fixed here rather than taken from the
    /// caller's own state directory.
    #[serde(default)]
    pub lock_dir: String,
}

pub const PATH: &str = "/etc/errand-box/runtime.toml";

pub fn path() -> PathBuf {
    std::env::var_os("ERRAND_BOX_RUNTIME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(PATH))
}

pub fn load() -> Result<Runtime, String> {
    load_from(&path())
}

pub fn load_from(p: &Path) -> Result<Runtime, String> {
    let text = std::fs::read_to_string(p).map_err(|e| {
        format!(
            "cannot read {}: {e}; run `errand-box setup` first",
            p.display()
        )
    })?;
    toml::from_str(&text).map_err(|e| format!("{}: {}", p.display(), e.message()))
}

pub fn render(r: &Runtime) -> String {
    let mut s =
        String::from("# Written by `errand-box setup`. Read by the bailey shim on every call.\n");
    s.push_str(&toml::to_string(r).expect("runtime serializes"));
    s
}

impl Runtime {
    pub fn endpoint(&self) -> crate::incus::Endpoint {
        crate::incus::Endpoint {
            bin: self.incus_bin.clone(),
            incus_dir: (!self.incus_dir.is_empty()).then(|| self.incus_dir.clone()),
            project: (!self.project.is_empty()).then(|| self.project.clone()),
        }
    }

    /// The endpoint for root: the same daemon, with the project named.
    pub fn admin_endpoint(&self) -> crate::incus::Endpoint {
        let mut e = self.endpoint();
        if !self.admin_project.is_empty() {
            e.project = Some(self.admin_project.clone());
        }
        e
    }

    /// The endpoint for whoever runs this program.
    pub fn endpoint_for_caller(&self) -> crate::incus::Endpoint {
        if crate::paths::euid() == 0 {
            self.admin_endpoint()
        } else {
            self.endpoint()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let r = Runtime {
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
            host_runtime_dirs: vec!["/usr/local/lib/errand-box/bin".into()],
            admin_project: "user-994".into(),
            state_root: "/var/lib/errand/state".into(),
            lock_dir: "/var/lib/errand/.local/state/errand-box/locks".into(),
        };
        let text = render(&r);
        let back: Runtime = toml::from_str(&text).unwrap();
        assert_eq!(back, r);
        // A key from a newer errand-box does not stop this one.
        let newer: Runtime = toml::from_str(&format!("{text}\nfuture_key = 1\n")).unwrap();
        assert_eq!(newer, r);
        assert_eq!(back.endpoint().project, None);
        assert_eq!(back.admin_endpoint().project.as_deref(), Some("user-994"));
    }
}
