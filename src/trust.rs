//! `bailey trust` and `bailey untrust`: records of the policy files errand
//! asked errand-box to apply.
//!
//! errand writes a session's policy into the session's state directory, which
//! the session itself can write. The record pins the digest errand's file had
//! at `trust`; `run` refuses a policy whose digest differs, so a policy the
//! session rewrote is never applied.

use crate::paths;
use crate::sha256;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

fn store() -> PathBuf {
    paths::state_dir().join("trust")
}

fn record_path(policy: &Path) -> PathBuf {
    store().join(sha256::digest_hex(policy.as_os_str().as_encoded_bytes()))
}

/// Refuse a file another user could have written: owned by someone other
/// than us or root, or writable by group or others, or in such a directory.
fn check_owner(p: &Path) -> Result<(), String> {
    let me = paths::euid();
    let md = fs::symlink_metadata(p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
    if !md.file_type().is_file() {
        return Err(format!("{} is not a regular file", p.display()));
    }
    let bad =
        |m: &fs::Metadata| (m.uid() != me && m.uid() != 0) || m.permissions().mode() & 0o022 != 0;
    if bad(&md) {
        return Err(format!(
            "{} is owned or writable by another user",
            p.display()
        ));
    }
    if let Some(dir) = p.parent() {
        let dm = fs::metadata(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        if bad(&dm) {
            return Err(format!(
                "{} is owned or writable by another user",
                dir.display()
            ));
        }
    }
    Ok(())
}

/// The bytes of a policy file, checked for ownership.
pub fn read_policy(p: &Path) -> Result<Vec<u8>, String> {
    check_owner(p)?;
    fs::read(p).map_err(|e| format!("cannot read {}: {e}", p.display()))
}

pub fn trust(p: &Path) -> Result<(), String> {
    let p = p
        .canonicalize()
        .map_err(|e| format!("cannot resolve {}: {e}", p.display()))?;
    let bytes = read_policy(&p)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| format!("{} is not UTF-8", p.display()))?;
    crate::policy::parse(text)?;
    let dir = store();
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("cannot set mode of {}: {e}", dir.display()))?;
    let rec = record_path(&p);
    let tmp = rec.with_extension("tmp");
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    writeln!(f, "{}\n{}", sha256::digest_hex(&bytes), p.display()).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &rec).map_err(|e| format!("cannot write {}: {e}", rec.display()))?;
    Ok(())
}

/// Remove the record. `Ok(false)` when there was none.
pub fn untrust(p: &Path) -> Result<bool, String> {
    let p = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    match fs::remove_file(record_path(&p)) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

/// The policy's bytes, if and only if they match the trusted digest.
pub fn verified(p: &Path) -> Result<Vec<u8>, String> {
    let p = p
        .canonicalize()
        .map_err(|e| format!("cannot resolve {}: {e}", p.display()))?;
    let bytes = read_policy(&p)?;
    let rec = fs::read_to_string(record_path(&p)).map_err(|_| {
        format!(
            "{} is not trusted; errand runs `bailey trust` before `bailey run`",
            p.display()
        )
    })?;
    let want = rec.lines().next().unwrap_or("");
    if want != sha256::digest_hex(&bytes) {
        return Err(format!(
            "{} changed after `bailey trust`; refusing it",
            p.display()
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // ERRAND_BOX_STATE is process-wide, so these tests take turns.
    pub static ENV: Mutex<()> = Mutex::new(());

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eb-trust-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o700)).unwrap();
        d
    }

    #[test]
    fn trust_then_verify_then_tamper() {
        let _g = ENV.lock().unwrap();
        let d = scratch("a");
        // SAFETY: tests that read this variable hold ENV.
        unsafe { std::env::set_var("ERRAND_BOX_STATE", d.join("state")) };
        let p = d.join("policy.toml");
        fs::write(&p, "[filesystem]\nreset = true\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(verified(&p).is_err());
        trust(&p).unwrap();
        assert!(verified(&p).is_ok());
        fs::write(&p, "[filesystem]\nreset = false\n").unwrap();
        let e = verified(&p).unwrap_err();
        assert!(e.contains("changed after"), "{e}");
        assert!(untrust(&p).unwrap());
        assert!(!untrust(&p).unwrap());
    }

    #[test]
    fn group_writable_policy_is_refused() {
        let _g = ENV.lock().unwrap();
        let d = scratch("b");
        unsafe { std::env::set_var("ERRAND_BOX_STATE", d.join("state")) };
        let p = d.join("policy.toml");
        fs::write(&p, "").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o620)).unwrap();
        assert!(trust(&p).unwrap_err().contains("writable by another user"));
    }

    #[test]
    fn an_invalid_policy_is_not_trusted() {
        let _g = ENV.lock().unwrap();
        let d = scratch("c");
        unsafe { std::env::set_var("ERRAND_BOX_STATE", d.join("state")) };
        let p = d.join("policy.toml");
        fs::write(&p, "[hooks]\nx = 1\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(trust(&p).is_err());
    }
}
