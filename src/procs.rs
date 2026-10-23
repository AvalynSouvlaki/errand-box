//! Small process helpers: identity of a pid across reuse, file locks, and
//! signal masks.

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::Path;

/// The start time of a process in clock ticks since boot (field 22 of
/// /proc/<pid>/stat). With the pid it names one process even after the pid
/// is reused.
pub fn start_time(pid: i32) -> Option<u64> {
    parse_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?).map(|(_, st)| st)
}

/// The state letter and the start time from a `/proc/<pid>/stat` line.
fn parse_stat(s: &str) -> Option<(char, u64)> {
    // The command name is in parentheses and may hold spaces; fields after
    // the last ')' are space separated, starting at field 3.
    let mut rest = s[s.rfind(')')? + 1..].split_whitespace();
    let state = rest.next()?.chars().next()?;
    Some((state, rest.nth(18)?.parse().ok()?))
}

/// `<pid>:<start time>` of the current process.
pub fn self_lease() -> String {
    let pid = std::process::id() as i32;
    format!("{pid}:{}", start_time(pid).unwrap_or(0))
}

/// True if the process a lease names still runs.
pub fn lease_alive(lease: &str) -> bool {
    let Some((pid, st)) = lease.split_once(':') else {
        return false;
    };
    let (Ok(pid), Ok(st)) = (pid.parse::<i32>(), st.parse::<u64>()) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // A zombie keeps its pid and start time until its parent reaps it, and
    // where PID 1 does not reap, that is never (E-33).
    match std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .as_deref()
        .and_then(parse_stat)
    {
        Some((state, start)) => start == st && !matches!(state, 'Z' | 'X'),
        None => false,
    }
}

/// An exclusive lock on a file, held until the value is dropped.
pub struct Lock {
    _f: File,
}

pub fn lock(path: &Path, wait: bool) -> Result<Lock, String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("cannot create {}: {e}", d.display()))?;
    }
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    // root and the errand account share lock files (session::lock_path): a
    // file root creates goes to the directory's owner, or the account could
    // not open it again.
    if let Some(md) = path.parent().and_then(|d| std::fs::metadata(d).ok()) {
        use std::os::unix::fs::MetadataExt;
        if crate::paths::euid() == 0 && md.uid() != 0 {
            let _ = std::os::unix::fs::fchown(&f, Some(md.uid()), Some(md.gid()));
        }
    }
    let op = if wait {
        libc::LOCK_EX
    } else {
        libc::LOCK_EX | libc::LOCK_NB
    };
    // SAFETY: flock on a descriptor we own.
    let rc = unsafe { libc::flock(f.as_raw_fd(), op) };
    if rc != 0 {
        return Err(format!(
            "{} is locked by another errand-box process",
            path.display()
        ));
    }
    Ok(Lock { _f: f })
}

/// The termination signals the shim handles itself.
pub fn term_signals() -> libc::sigset_t {
    // SAFETY: sigset operations on a zeroed set we own.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for s in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP, libc::SIGQUIT] {
            libc::sigaddset(&mut set, s);
        }
        set
    }
}

/// Block `set` in the calling thread; threads started later inherit it.
pub fn block(set: &libc::sigset_t) {
    // SAFETY: pthread_sigmask with a valid set.
    unsafe {
        libc::pthread_sigmask(libc::SIG_BLOCK, set, std::ptr::null_mut());
    }
}

/// Wait for one signal of `set`; returns its number.
pub fn wait_signal(set: &libc::sigset_t) -> i32 {
    let mut sig: libc::c_int = 0;
    // SAFETY: sigwait with a valid set and an out pointer we own.
    unsafe {
        libc::sigwait(set, &mut sig);
    }
    sig
}

/// For `Command::pre_exec`: an empty signal mask and default dispositions,
/// so a child does not inherit the shim's blocked signals.
pub fn reset_signals_in_child() -> std::io::Result<()> {
    // SAFETY: async-signal-safe calls between fork and exec.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::pthread_sigmask(libc::SIG_SETMASK, &set, std::ptr::null_mut());
        for s in [
            libc::SIGTERM,
            libc::SIGINT,
            libc::SIGHUP,
            libc::SIGQUIT,
            libc::SIGPIPE,
        ] {
            libc::signal(s, libc::SIG_DFL);
        }
    }
    Ok(())
}

/// The uid, gid and home of an account.
pub fn account(name: &str) -> Option<(u32, u32, String)> {
    let cname = std::ffi::CString::new(name).ok()?;
    // SAFETY: getpwnam_r writes into buffers we own; read only on success.
    unsafe {
        let mut pwd: libc::passwd = std::mem::zeroed();
        let mut buf = vec![0 as libc::c_char; 16384];
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let rc = libc::getpwnam_r(
            cname.as_ptr(),
            &mut pwd,
            buf.as_mut_ptr(),
            buf.len(),
            &mut result,
        );
        if rc != 0 || result.is_null() {
            return None;
        }
        let home = std::ffi::CStr::from_ptr(pwd.pw_dir)
            .to_string_lossy()
            .into_owned();
        Some((pwd.pw_uid, pwd.pw_gid, home))
    }
}

/// True when the group database has `name`.
pub fn group_exists(name: &str) -> bool {
    let Ok(cname) = std::ffi::CString::new(name) else {
        return false;
    };
    // SAFETY: getgrnam_r writes into buffers we own; read only on success.
    unsafe {
        let mut grp: libc::group = std::mem::zeroed();
        let mut buf = vec![0 as libc::c_char; 65536];
        let mut result: *mut libc::group = std::ptr::null_mut();
        let rc = libc::getgrnam_r(
            cname.as_ptr(),
            &mut grp,
            buf.as_mut_ptr(),
            buf.len(),
            &mut result,
        );
        rc == 0 && !result.is_null()
    }
}

/// Become `user` (groups, gid, uid, in that order) and exec `argv` with a
/// fresh environment: the caller's PATH, LANG and TERM, plus HOME, USER and
/// LOGNAME of the account. Returns only on failure.
pub fn exec_as(user: &str, argv: &[String]) -> String {
    use std::os::unix::process::CommandExt;
    let Some((uid, gid, home)) = account(user) else {
        return format!("no account {user:?}");
    };
    let Some(prog) = argv.first() else {
        return "no command".to_owned();
    };
    let cname = std::ffi::CString::new(user).unwrap_or_default();
    // SAFETY: plain credential calls; each result is checked.
    unsafe {
        if libc::initgroups(cname.as_ptr(), gid) != 0
            || libc::setgid(gid) != 0
            || libc::setuid(uid) != 0
        {
            return format!("cannot become {user}: {}", std::io::Error::last_os_error());
        }
        if uid != 0 && libc::setuid(0) == 0 {
            return "dropping privileges did not hold".to_owned();
        }
    }
    let mut c = std::process::Command::new(prog);
    c.args(&argv[1..]);
    c.env_clear();
    for k in [
        "PATH",
        "LANG",
        "LC_ALL",
        "TERM",
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "NO_PROXY",
        "https_proxy",
        "http_proxy",
        "no_proxy",
    ] {
        if let Some(v) = std::env::var_os(k) {
            c.env(k, v);
        }
    }
    c.env("HOME", &home).env("USER", user).env("LOGNAME", user);
    c.current_dir(&home);
    let e = c.exec();
    format!("cannot run {prog}: {e}")
}

pub fn kill(pid: i32, sig: i32) {
    // SAFETY: kill has no memory effects.
    unsafe {
        libc::kill(pid, sig);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zombie_lease_is_dead() {
        let tail = "1 4033 4033 0 -1 4194560 75 0 0 0 0 0 0 0 20 0 1 0 312318 0 0";
        assert_eq!(
            parse_stat(&format!("4033 (s6-svscan) S {tail}")),
            Some(('S', 312318))
        );
        assert_eq!(
            parse_stat(&format!("4033 (s6 svscan) Z {tail}")),
            Some(('Z', 312318))
        );
        assert_eq!(parse_stat("4033 (x"), None);
        // The live half: this process is alive, and a made-up start time is not.
        let me = self_lease();
        assert!(lease_alive(&me));
        assert!(!lease_alive(&format!("{}:1", std::process::id())));
    }

    #[test]
    fn own_lease_is_alive_and_a_fake_one_is_not() {
        assert!(lease_alive(&self_lease()));
        assert!(!lease_alive("1:0"));
        assert!(!lease_alive("garbage"));
        assert!(!lease_alive(&format!("{}:1", std::process::id())));
    }

    #[test]
    fn a_second_lock_fails_without_waiting() {
        let p = std::env::temp_dir().join(format!("eb-lock-{}", std::process::id()));
        let _a = lock(&p, false).unwrap();
        // flock locks are per open file description, so a second open in
        // the same process conflicts too.
        assert!(lock(&p, false).is_err());
    }
}
