//! The policy file errand writes for each session, read as errand-box needs it.
//!
//! errand writes it at `<stateDir>/<id>/policy.toml` for bailey (the schema is
//! in docs/bailey-contract.md section 6). errand-box does not confine a host
//! process with it. It reads three things from it: which host directories the
//! session sees and where, which environment the agent gets, and the file
//! size limit. The guest is a whole system with its own `/usr` and `/etc`, so
//! the grants of host system paths and the execute grants have no meaning
//! here and are dropped.
//!
//! The parser is strict. A key errand-box does not know is an error, so a
//! newer errand that adds a key fails errand's start-up probe with a message
//! instead of running sessions under a policy that is only half applied.

use std::collections::BTreeMap;
use std::path::{Component, Path};
use toml::{Table, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    /// The host path.
    pub path: String,
    /// Where the session sees it; `None` means at the same path.
    pub at: Option<String>,
}

impl Grant {
    pub fn inside(&self) -> &str {
        self.at.as_deref().unwrap_or(&self.path)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub path: String,
    pub access: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    pub reset: bool,
    pub read: Vec<Grant>,
    pub write: Vec<Grant>,
    pub execute: Vec<Grant>,
    pub devices: Vec<Device>,
    pub pass: Vec<String>,
    pub set: BTreeMap<String, String>,
    pub file_max: Option<u64>,
    pub tmp_size: Option<u64>,
    pub shm_size: Option<u64>,
    pub tmp_dir: Option<String>,
    /// `(host pattern, port)` pairs errand allows; the broker enforces them.
    pub egress: Vec<(String, u16)>,
}

/// A host directory to attach to the instance as a disk device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount {
    pub source: String,
    pub path: String,
    pub readonly: bool,
}

/// The fixed paths a session sees, per errand's backend contract.
pub const WORKSPACE: &str = "/workspace";
pub const STATE: &str = "/state";

/// Host system paths errand grants for a confined host process. The guest has
/// its own copies, so these grants are dropped.
const SYSTEM_PREFIXES: &[&str] = &[
    "/usr",
    "/lib",
    "/lib32",
    "/lib64",
    "/libx32",
    "/bin",
    "/sbin",
    "/etc",
    "/proc",
    "/sys",
    "/dev",
    "/var/lib/ca-certificates",
];

pub fn parse(text: &str) -> Result<Policy, String> {
    let t: Table = text
        .parse()
        .map_err(|e: toml::de::Error| format!("the policy is not valid TOML: {}", e.message()))?;
    let mut p = Policy::default();
    for (k, v) in &t {
        match k.as_str() {
            "filesystem" => parse_fs(table(v, "filesystem")?, &mut p)?,
            "device" => p.devices = parse_devices(v)?,
            "env" => parse_env(table(v, "env")?, &mut p)?,
            "resources" => parse_resources(table(v, "resources")?, &mut p)?,
            "network" => parse_network(table(v, "network")?, &mut p)?,
            other => return Err(unknown("", other)),
        }
    }
    Ok(p)
}

fn unknown(section: &str, key: &str) -> String {
    let at = if section.is_empty() {
        String::new()
    } else {
        format!("[{section}].")
    };
    format!(
        "the policy has the key {at}{key}, which this errand-box does not apply; update errand-box"
    )
}

fn table<'a>(v: &'a Value, name: &str) -> Result<&'a Table, String> {
    v.as_table()
        .ok_or_else(|| format!("[{name}] is not a table"))
}

fn string(v: &Value, what: &str) -> Result<String, String> {
    v.as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{what} is not a string"))
}

fn abs_path(s: &str, what: &str) -> Result<String, String> {
    let p = Path::new(s);
    if !p.is_absolute()
        || s.contains('\0')
        || p.components().any(|c| matches!(c, Component::ParentDir))
    {
        return Err(format!(
            "{what} must be an absolute path without `..`: {s:?}"
        ));
    }
    Ok(s.to_owned())
}

fn parse_grants(v: &Value, what: &str) -> Result<Vec<Grant>, String> {
    let arr = v
        .as_array()
        .ok_or_else(|| format!("{what} is not an array"))?;
    let mut out = Vec::new();
    for item in arr {
        match item {
            Value::String(s) => out.push(Grant {
                path: abs_path(s, what)?,
                at: None,
            }),
            Value::Table(t) => {
                let mut g = Grant {
                    path: String::new(),
                    at: None,
                };
                for (k, v) in t {
                    match k.as_str() {
                        "path" => g.path = abs_path(&string(v, "a grant path")?, what)?,
                        "at" => g.at = Some(abs_path(&string(v, "a grant `at`")?, what)?),
                        other => return Err(unknown(what, other)),
                    }
                }
                if g.path.is_empty() {
                    return Err(format!("a grant in {what} has no path"));
                }
                out.push(g);
            }
            _ => {
                return Err(format!(
                    "{what} holds a value that is neither a path nor a table"
                ));
            }
        }
    }
    Ok(out)
}

fn parse_fs(t: &Table, p: &mut Policy) -> Result<(), String> {
    for (k, v) in t {
        match k.as_str() {
            "reset" => p.reset = v.as_bool().ok_or("filesystem.reset is not a boolean")?,
            "read" => p.read = parse_grants(v, "filesystem.read")?,
            "write" => p.write = parse_grants(v, "filesystem.write")?,
            "execute" => p.execute = parse_grants(v, "filesystem.execute")?,
            other => return Err(unknown("filesystem", other)),
        }
    }
    Ok(())
}

fn parse_devices(v: &Value) -> Result<Vec<Device>, String> {
    let arr = v.as_array().ok_or("[[device]] is not an array of tables")?;
    let mut out = Vec::new();
    for item in arr {
        let t = item.as_table().ok_or("a [[device]] entry is not a table")?;
        let mut d = Device {
            path: String::new(),
            access: String::new(),
        };
        for (k, v) in t {
            match k.as_str() {
                "path" => d.path = string(v, "device.path")?,
                "access" => d.access = string(v, "device.access")?,
                other => return Err(unknown("device", other)),
            }
        }
        if !d.path.starts_with("/dev/") || !matches!(d.access.as_str(), "r" | "w" | "rw") {
            return Err(format!(
                "a [[device]] entry is not a /dev path with r, w or rw: {d:?}"
            ));
        }
        out.push(d);
    }
    Ok(out)
}

pub fn valid_env_name(n: &str) -> bool {
    let mut cs = n.chars();
    matches!(cs.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && cs.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn parse_env(t: &Table, p: &mut Policy) -> Result<(), String> {
    for (k, v) in t {
        match k.as_str() {
            "pass" => {
                let arr = v.as_array().ok_or("env.pass is not an array")?;
                for n in arr {
                    let n = string(n, "an env.pass entry")?;
                    if !valid_env_name(&n) {
                        return Err(format!("env.pass names an invalid variable: {n:?}"));
                    }
                    p.pass.push(n);
                }
            }
            "set" => {
                let st = v.as_table().ok_or("env.set is not a table")?;
                for (name, val) in st {
                    if !valid_env_name(name) {
                        return Err(format!("env.set names an invalid variable: {name:?}"));
                    }
                    p.set.insert(name.clone(), string(val, "an env.set value")?);
                }
            }
            other => return Err(unknown("env", other)),
        }
    }
    Ok(())
}

/// A size as errand writes it: digits, then an optional k, m, g or t, with an
/// optional `b` or `ib`. Powers of 1024.
pub fn parse_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return Err(format!("not a size: {s:?}"));
    }
    let n: u64 = digits.parse().map_err(|_| format!("not a size: {s:?}"))?;
    let unit = s[digits.len()..].trim().to_ascii_lowercase();
    let mul: u64 = match unit.as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1 << 10,
        "m" | "mb" | "mib" => 1 << 20,
        "g" | "gb" | "gib" => 1 << 30,
        "t" | "tb" | "tib" => 1 << 40,
        _ => return Err(format!("unknown size unit in {s:?}")),
    };
    n.checked_mul(mul)
        .ok_or_else(|| format!("size out of range: {s:?}"))
}

fn parse_resources(t: &Table, p: &mut Policy) -> Result<(), String> {
    for (k, v) in t {
        let s = || string(v, &format!("resources.{k}"));
        match k.as_str() {
            "file_max" => p.file_max = Some(parse_size(&s()?)?),
            "tmp_size" => p.tmp_size = Some(parse_size(&s()?)?),
            "shm_size" => p.shm_size = Some(parse_size(&s()?)?),
            "tmp_dir" => p.tmp_dir = Some(abs_path(&s()?, "resources.tmp_dir")?),
            other => return Err(unknown("resources", other)),
        }
    }
    Ok(())
}

fn parse_network(t: &Table, p: &mut Policy) -> Result<(), String> {
    for (k, v) in t {
        match k.as_str() {
            "egress_allow" => {
                let arr = v.as_array().ok_or("network.egress_allow is not an array")?;
                for item in arr {
                    let e = item
                        .as_table()
                        .ok_or("a network.egress_allow entry is not a table")?;
                    let mut host = None;
                    let mut port = None;
                    for (ek, ev) in e {
                        match ek.as_str() {
                            "host" => host = Some(string(ev, "egress host")?),
                            "port" => {
                                let n = ev.as_integer().ok_or("egress port is not an integer")?;
                                port = Some(
                                    u16::try_from(n)
                                        .map_err(|_| format!("egress port out of range: {n}"))?,
                                );
                            }
                            other => return Err(unknown("network.egress_allow", other)),
                        }
                    }
                    match (host, port) {
                        (Some(h), Some(pt)) => p.egress.push((h, pt)),
                        _ => {
                            return Err(
                                "a network.egress_allow entry needs host and port".to_owned()
                            );
                        }
                    }
                }
            }
            other => return Err(unknown("network", other)),
        }
    }
    Ok(())
}

fn is_system(path: &str) -> bool {
    SYSTEM_PREFIXES.iter().any(|pre| {
        path == *pre
            || path
                .strip_prefix(pre)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

impl Policy {
    /// The host directory errand grants at `inside` with write access.
    pub fn write_grant_at(&self, inside: &str) -> Option<&Grant> {
        self.write.iter().find(|g| g.inside() == inside)
    }

    /// The session's id: the name of its state directory, which errand
    /// creates as `<stateDir>/<id>`.
    pub fn session_id(&self) -> Result<String, String> {
        let st = self
            .write_grant_at(STATE)
            .ok_or("the policy grants no /state directory")?;
        Path::new(&st.path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_owned)
            .ok_or_else(|| format!("the state directory has no name: {}", st.path))
    }

    /// The host directories to attach. `skip` names host paths that are the
    /// agent's runtime on the host (the guest has its own kage), which errand
    /// grants for a confined host process.
    pub fn mounts(&self, skip: &dyn Fn(&str) -> bool) -> Result<Vec<Mount>, String> {
        let ws = self
            .write_grant_at(WORKSPACE)
            .ok_or("the policy grants no /workspace directory")?;
        let st = self
            .write_grant_at(STATE)
            .ok_or("the policy grants no /state directory")?;
        let mut out = vec![
            Mount {
                source: ws.path.clone(),
                path: WORKSPACE.to_owned(),
                readonly: false,
            },
            Mount {
                source: st.path.clone(),
                path: STATE.to_owned(),
                readonly: false,
            },
        ];
        let mut seen: Vec<String> = out.iter().map(|m| m.path.clone()).collect();
        for g in &self.write {
            let at = g.inside().to_owned();
            if seen.contains(&at) || is_system(&at) || skip(&g.path) {
                continue;
            }
            seen.push(at.clone());
            out.push(Mount {
                source: g.path.clone(),
                path: at,
                readonly: false,
            });
        }
        for g in &self.read {
            let at = g.inside().to_owned();
            if seen.contains(&at) || is_system(&at) || is_system(&g.path) || skip(&g.path) {
                continue;
            }
            // A read grant of a path inside a granted write tree adds nothing.
            if out.iter().any(|m| at.starts_with(&format!("{}/", m.path))) {
                continue;
            }
            seen.push(at.clone());
            out.push(Mount {
                source: g.path.clone(),
                path: at,
                readonly: true,
            });
        }
        Ok(out)
    }

    /// The agent's environment: every `pass` name with its value from `from`
    /// (names `from` lacks are left out, as bailey does), then `set`.
    pub fn agent_env(&self, from: &dyn Fn(&str) -> Option<String>) -> BTreeMap<String, String> {
        let mut env = BTreeMap::new();
        for n in &self.pass {
            if let Some(v) = from(n) {
                env.insert(n.clone(), v);
            }
        }
        for (k, v) in &self.set {
            env.insert(k.clone(), v.clone());
        }
        env
    }
}

/// errand's sample policy, for other modules' tests.
#[cfg(test)]
pub fn tests_sample() -> String {
    tests::SAMPLE.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// errand's output for one session, as docs/bailey-contract.md 6.3 gives it.
    pub const SAMPLE: &str = r#"# Generated per session by errand. Do not edit.
[filesystem]
reset = true
read = ["/usr", "/lib", "/lib64", "/bin", "/sbin", "/proc", "/etc/ld.so.cache", "/etc/ld.so.conf", "/etc/ld.so.conf.d", "/etc/ssl", "/etc/ca-certificates.conf", "/etc/ca-certificates", "/var/lib/ca-certificates", "/etc/pki", "/etc/nsswitch.conf", "/etc/services", "/etc/protocols", { path = "/srv/errand/state/resolv.conf", at = "/etc/resolv.conf" }, { path = "/srv/errand/projects/demo", at = "/workspace" }, { path = "/srv/errand/state/abc123", at = "/state" }, "/usr/local/bin"]
write = [{ path = "/srv/errand/projects/demo", at = "/workspace" }, { path = "/srv/errand/state/abc123", at = "/state" }]
execute = ["/usr", "/lib", "/lib64", "/bin", "/sbin", "/state/home/bin", "/usr/local/bin", { path = "/srv/errand/projects/demo", at = "/workspace" }]
[[device]]
path = "/dev/null"
access = "rw"
[[device]]
path = "/dev/tty"
access = "rw"

[env]
pass = ["OPENAI_API_KEY", "XDG_CACHE_HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME"]
set = { PATH = "/state/home/bin:/usr/local/bin:/usr/bin:/bin", HOME = "/state/home", HTTPS_PROXY = "http://169.254.169.1:41234", NO_PROXY = "169.254.169.1" }

[resources]
file_max = "1g"
tmp_size = "512m"
shm_size = "256m"

[network]
egress_allow = [{ host = "*", port = 443 }]
"#;

    #[test]
    fn parses_errands_policy() {
        let p = parse(SAMPLE).unwrap();
        assert!(p.reset);
        assert_eq!(p.devices.len(), 2);
        assert_eq!(p.file_max, Some(1 << 30));
        assert_eq!(p.tmp_size, Some(512 << 20));
        assert_eq!(p.egress, vec![("*".to_owned(), 443)]);
        assert_eq!(p.session_id().unwrap(), "abc123");
    }

    #[test]
    fn mounts_are_workspace_and_state_only() {
        let p = parse(SAMPLE).unwrap();
        let m = p.mounts(&|_| false).unwrap();
        assert_eq!(
            m,
            vec![
                Mount {
                    source: "/srv/errand/projects/demo".into(),
                    path: "/workspace".into(),
                    readonly: false
                },
                Mount {
                    source: "/srv/errand/state/abc123".into(),
                    path: "/state".into(),
                    readonly: false
                },
            ]
        );
    }

    #[test]
    fn operator_extras_become_mounts_and_runtime_dirs_are_skipped() {
        let text = SAMPLE
            .replace(
                r#"write = [{ path"#,
                r#"write = [{ path = "/srv/cache", at = "/cache" }, { path"#,
            )
            .replace(
                r#""/usr/local/bin"]"#,
                r#""/usr/local/bin", "/srv/errand/.local/kage", "/srv/data"]"#,
            );
        let p = parse(&text).unwrap();
        let m = p
            .mounts(&|path| path.starts_with("/srv/errand/.local"))
            .unwrap();
        let paths: Vec<_> = m.iter().map(|m| (m.path.as_str(), m.readonly)).collect();
        assert_eq!(
            paths,
            vec![
                ("/workspace", false),
                ("/state", false),
                ("/cache", false),
                ("/srv/data", true)
            ]
        );
    }

    #[test]
    fn an_unknown_key_is_refused() {
        let e = parse("[resources]\nmemory = \"1g\"\n").unwrap_err();
        assert!(e.contains("resources].memory"), "{e}");
        let e = parse("[hooks]\npre = \"x\"\n").unwrap_err();
        assert!(e.contains("hooks"), "{e}");
    }

    #[test]
    fn relative_and_dotdot_paths_are_refused() {
        assert!(parse("[filesystem]\nwrite = [\"rel\"]\n").is_err());
        assert!(
            parse("[filesystem]\nwrite = [{ path = \"/a/../b\", at = \"/workspace\" }]\n").is_err()
        );
    }

    #[test]
    fn bad_env_names_are_refused() {
        assert!(parse("[env]\npass = [\"A-B\"]\n").is_err());
        assert!(parse("[env]\nset = { \"1X\" = \"v\" }\n").is_err());
    }

    #[test]
    fn agent_env_takes_pass_from_the_run_and_set_last() {
        let p = parse(SAMPLE).unwrap();
        let env = p.agent_env(&|n| (n == "OPENAI_API_KEY").then(|| "nonce-1".to_owned()));
        assert_eq!(env["OPENAI_API_KEY"], "nonce-1");
        assert!(!env.contains_key("XDG_DATA_HOME"));
        assert_eq!(env["HOME"], "/state/home");
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("1g").unwrap(), 1 << 30);
        assert_eq!(parse_size("512m").unwrap(), 512 << 20);
        assert_eq!(parse_size("20GiB").unwrap(), 20 << 30);
        assert_eq!(parse_size("7").unwrap(), 7);
        assert!(parse_size("x").is_err());
        assert!(parse_size("1q").is_err());
    }

    #[test]
    fn no_state_means_no_session() {
        assert!(
            parse("[filesystem]\nreset = true\n")
                .unwrap()
                .session_id()
                .is_err()
        );
    }
}
