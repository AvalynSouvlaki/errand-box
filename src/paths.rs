//! Where errand-box keeps its own state, decided from the passwd entry of the
//! effective user and never from `HOME` or `XDG_*`. errand runs the session
//! launcher with `HOME` and `XDG_*` pointed at session paths
//! (docs/bailey-contract.md section 5.2), so those variables cannot locate
//! errand-box's trust records.

use std::ffi::CStr;
use std::path::PathBuf;

pub fn euid() -> u32 {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}

/// The passwd home of a uid.
pub fn home_of(uid: u32) -> Option<PathBuf> {
    // SAFETY: getpwuid_r writes into the buffers we own; the result pointer is
    // read only when the call reports success.
    unsafe {
        let mut pwd: libc::passwd = std::mem::zeroed();
        let mut buf = vec![0 as libc::c_char; 16384];
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let rc = libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result);
        if rc != 0 || result.is_null() || pwd.pw_dir.is_null() {
            return None;
        }
        let dir = CStr::from_ptr(pwd.pw_dir).to_str().ok()?;
        Some(PathBuf::from(dir))
    }
}

/// errand-box's per-user state: trust records, locks, leases.
/// `ERRAND_BOX_STATE` overrides it, for tests only.
pub fn state_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("ERRAND_BOX_STATE") {
        return PathBuf::from(d);
    }
    let uid = euid();
    if uid == 0 {
        return PathBuf::from("/var/lib/errand-box");
    }
    home_of(uid)
        .unwrap_or_else(|| PathBuf::from("/nonexistent"))
        .join(".local/state/errand-box")
}

/// The fixed, system-wide places setup writes.
pub const SYSTEM_STATE: &str = "/var/lib/errand-box";
pub const RUN_DIR: &str = "/run/errand-box";
