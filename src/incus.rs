//! The Incus client, driven as a program.
//!
//! errand-box calls the `incus` command line, never the socket directly, so
//! the operator can repeat any call by hand. Every call goes through
//! `run::Runner`, with stdin closed and a deadline (E-11).

use crate::run::{Cmd, Out, Runner};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

/// How to reach one Incus daemon and project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    /// The client program, `incus` unless set.
    pub bin: String,
    /// `INCUS_DIR` for the client: set for the rootless engine, whose daemon
    /// keeps its socket under the operator's own state directory.
    pub incus_dir: Option<String>,
    /// The project, or none for the client's default. Under incus-user the
    /// daemon fixes the project from the client certificate.
    pub project: Option<String>,
}

impl Default for Endpoint {
    fn default() -> Self {
        Endpoint {
            bin: "incus".to_owned(),
            incus_dir: None,
            project: None,
        }
    }
}

/// The state of an instance, as far as errand-box needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance {
    pub name: String,
    pub status: String,
    pub config: BTreeMap<String, String>,
    pub devices: BTreeMap<String, BTreeMap<String, String>>,
}

impl Instance {
    pub fn running(&self) -> bool {
        // Incus reports "Ready" for a running guest that has signalled
        // readiness; it is up, so it counts as running (E-60).
        self.status.eq_ignore_ascii_case("running") || self.status.eq_ignore_ascii_case("ready")
    }
}

pub struct Incus<'a> {
    pub ep: Endpoint,
    pub runner: &'a dyn Runner,
}

const SHORT: Duration = Duration::from_secs(60);
const LONG: Duration = Duration::from_secs(900);

impl<'a> Incus<'a> {
    pub fn new(ep: Endpoint, runner: &'a dyn Runner) -> Self {
        Incus { ep, runner }
    }

    /// A client call. The project flag goes first so it applies to every
    /// subcommand.
    pub fn cmd<I, S>(&self, args: I) -> Cmd
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut c = client_env(Cmd::new(&self.ep.bin).timeout(SHORT));
        if let Some(p) = &self.ep.project {
            c = c.arg("--project").arg(p.clone());
        }
        if let Some(d) = &self.ep.incus_dir {
            c = c.env("INCUS_DIR", d);
        }
        c.args(args)
    }

    pub fn run<I, S>(&self, args: I) -> Out
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.runner.run(&self.cmd(args))
    }

    fn run_long<I, S>(&self, args: I) -> Out
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.runner.run(&self.cmd(args).timeout(LONG))
    }

    /// `incus query`, with the project carried as a query parameter, since
    /// `query` does not apply `--project` to a raw path.
    pub fn query(&self, path: &str) -> Result<Value, String> {
        let path = match &self.ep.project {
            Some(p) if !path.contains("project=") => {
                let sep = if path.contains('?') { '&' } else { '?' };
                format!("{path}{sep}project={p}")
            }
            _ => path.to_owned(),
        };
        let mut c = client_env(Cmd::new(&self.ep.bin).timeout(SHORT))
            .arg("query")
            .arg(path);
        if let Some(d) = &self.ep.incus_dir {
            c = c.env("INCUS_DIR", d);
        }
        let out = self.runner.run(&c);
        if !out.ok() {
            return Err(out.why());
        }
        serde_json::from_str(&out.stdout).map_err(|e| format!("bad JSON from incus query: {e}"))
    }

    /// The server's version string, which also proves the daemon answers.
    pub fn server_version(&self) -> Result<String, String> {
        let v = self.query("/1.0")?;
        v.pointer("/environment/server_version")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| "the server did not report its version".to_owned())
    }

    /// One instance, or `Ok(None)` if it does not exist.
    pub fn instance(&self, name: &str) -> Result<Option<Instance>, String> {
        match self.query(&format!("/1.0/instances/{name}")) {
            Ok(v) => Ok(Some(parse_instance(&v)?)),
            Err(e) if e.contains("not found") || e.contains("Not Found") => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Every instance whose name starts with `prefix`.
    pub fn instances(&self, prefix: &str) -> Result<Vec<Instance>, String> {
        let v = self.query("/1.0/instances?recursion=1")?;
        let arr = v.as_array().ok_or("instance list is not an array")?;
        let mut out = Vec::new();
        for item in arr {
            let i = parse_instance(item)?;
            if i.name.starts_with(prefix) {
                out.push(i);
            }
        }
        Ok(out)
    }

    pub fn copy(&self, src: &str, dst: &str, config: &[(String, String)]) -> Result<(), String> {
        let mut args = vec!["copy".to_owned(), src.to_owned(), dst.to_owned()];
        for (k, v) in config {
            args.push("--config".to_owned());
            args.push(format!("{k}={v}"));
        }
        check(self.run_long(args))
    }

    pub fn init(&self, image: &str, name: &str, config: &[(String, String)]) -> Result<(), String> {
        let mut args = vec![
            "init".to_owned(),
            "-q".to_owned(),
            image.to_owned(),
            name.to_owned(),
        ];
        for (k, v) in config {
            args.push("--config".to_owned());
            args.push(format!("{k}={v}"));
        }
        check(self.run_long(args))
    }

    pub fn start(&self, name: &str) -> Result<(), String> {
        check(self.run_long(["start", name]))
    }

    /// Stop: clean first, then forced. Returns once the instance is down or
    /// both attempts failed.
    pub fn stop(&self, name: &str, clean_secs: u64) -> Result<(), String> {
        let clean = self.run(["stop", name, "--timeout", &clean_secs.to_string()]);
        if clean.ok() {
            return Ok(());
        }
        let forced = self.run(["stop", name, "--force"]);
        if forced.ok() {
            return Ok(());
        }
        // An instance that is already stopped makes both calls fail.
        match self.instance(name) {
            Ok(Some(i)) if !i.running() => Ok(()),
            Ok(None) => Ok(()),
            _ => Err(forced.why()),
        }
    }

    pub fn delete(&self, name: &str) -> Result<(), String> {
        check(self.run(["delete", "--force", name]))
    }

    /// Rename a stopped instance.
    pub fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        check(self.run(["rename", from, to]))
    }

    /// Point `alias` at `target`, replacing what it pointed at: the new
    /// alias is made under a spare name first, so the old one stays until
    /// the new one exists.
    pub fn move_alias(&self, alias: &str, target: &str) -> Result<(), String> {
        let spare = format!("{alias}-next");
        let _ = self.run(["image", "alias", "delete", &spare]);
        check(self.run(["image", "alias", "create", &spare, target]))?;
        let _ = self.run(["image", "alias", "delete", alias]);
        check(self.run(["image", "alias", "rename", &spare, alias]))
    }

    pub fn config_set(&self, name: &str, kv: &[(String, String)]) -> Result<(), String> {
        if kv.is_empty() {
            return Ok(());
        }
        let mut args = vec!["config".to_owned(), "set".to_owned(), name.to_owned()];
        for (k, v) in kv {
            args.push(format!("{k}={v}"));
        }
        check(self.run(args))
    }

    pub fn config_unset(&self, name: &str, key: &str) -> Result<(), String> {
        check(self.run(["config", "unset", name, key]))
    }

    pub fn device_add(
        &self,
        name: &str,
        dev: &str,
        kind: &str,
        props: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        let mut args = vec![
            "config".to_owned(),
            "device".to_owned(),
            "add".to_owned(),
            name.to_owned(),
            dev.to_owned(),
            kind.to_owned(),
        ];
        args.extend(props.iter().map(|(k, v)| format!("{k}={v}")));
        check(self.run(args))
    }

    pub fn device_remove(&self, name: &str, dev: &str) -> Result<(), String> {
        check(self.run(["config", "device", "remove", name, dev]))
    }

    /// Write `bytes` to `path` inside the instance, through the API rather
    /// than argv, so a secret never shows in the host's process table.
    pub fn file_push(
        &self,
        name: &str,
        path: &str,
        bytes: Vec<u8>,
        mode: &str,
        uid: u32,
        gid: u32,
    ) -> Result<(), String> {
        let c = self
            .cmd([
                "file".to_owned(),
                "push".to_owned(),
                "-".to_owned(),
                format!("{name}{path}"),
                "--mode".to_owned(),
                mode.to_owned(),
                "--uid".to_owned(),
                uid.to_string(),
                "--gid".to_owned(),
                gid.to_string(),
            ])
            .stdin(bytes);
        check(self.runner.run(&c))
    }

    /// Run a short command inside an instance and return its output.
    pub fn exec(&self, name: &str, argv: &[&str]) -> Out {
        let mut args = vec!["exec".to_owned(), name.to_owned(), "--".to_owned()];
        args.extend(argv.iter().map(|s| (*s).to_owned()));
        self.run(args)
    }
}

/// The client's configuration directory. errand runs the session launcher
/// with `HOME` inside the session's state directory, which the session can
/// write; a client that read its config from there would act on files the
/// guest planted. So every call names errand-box's own directory, and `HOME`
/// comes from the passwd entry.
pub fn conf_dir() -> std::path::PathBuf {
    crate::paths::state_dir().join("incus-client")
}

pub fn client_env(c: Cmd) -> Cmd {
    let c = c.env("INCUS_CONF", &conf_dir().to_string_lossy());
    match crate::paths::home_of(crate::paths::euid()) {
        Some(h) => c.env("HOME", &h.to_string_lossy()),
        None => c,
    }
}

fn check(out: Out) -> Result<(), String> {
    if out.ok() { Ok(()) } else { Err(out.why()) }
}

fn str_map(v: Option<&Value>) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    if let Some(Value::Object(o)) = v {
        for (k, val) in o {
            if let Some(s) = val.as_str() {
                m.insert(k.clone(), s.to_owned());
            }
        }
    }
    m
}

pub fn parse_instance(v: &Value) -> Result<Instance, String> {
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .ok_or("instance without a name")?
        .to_owned();
    let status = v
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let mut devices = BTreeMap::new();
    if let Some(Value::Object(d)) = v.get("devices") {
        for (k, dv) in d {
            devices.insert(k.clone(), str_map(Some(dv)));
        }
    }
    Ok(Instance {
        name,
        status,
        config: str_map(v.get("config")),
        devices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::Fake;

    fn ep() -> Endpoint {
        Endpoint {
            project: Some("user-994".to_owned()),
            ..Endpoint::default()
        }
    }

    #[test]
    fn project_flag_comes_first() {
        let f = Fake::new();
        let i = Incus::new(ep(), &f);
        let _ = i.start("eb-a");
        assert_eq!(f.lines(), vec!["incus --project user-994 start eb-a"]);
    }

    #[test]
    fn query_carries_the_project() {
        let f = Fake::new();
        f.on("incus query", 0, r#"{"name":"eb-a","status":"Running","config":{"k":"v"},"devices":{"ws":{"type":"disk"}}}"#);
        let i = Incus::new(ep(), &f);
        let inst = i.instance("eb-a").unwrap().unwrap();
        assert!(inst.running());
        assert!(
            parse_instance(
                &serde_json::json!({"name":"x","status":"Ready","config":{},"devices":{}})
            )
            .unwrap()
            .running()
        );
        assert_eq!(inst.config["k"], "v");
        assert_eq!(inst.devices["ws"]["type"], "disk");
        assert_eq!(
            f.lines(),
            vec!["incus query /1.0/instances/eb-a?project=user-994"]
        );
    }

    #[test]
    fn a_missing_instance_is_none() {
        let f = Fake::new();
        let i = Incus::new(Endpoint::default(), &f);
        f.on("incus query", 1, "");
        // The fake puts the reason on stdout; the real client puts it on stderr.
        f.on(
            "incus query /1.0/instances/x",
            1,
            "Error: Instance not found",
        );
        assert_eq!(i.instance("x").unwrap(), None);
    }

    #[test]
    fn file_push_sends_bytes_on_stdin_not_argv() {
        let f = Fake::new();
        let i = Incus::new(Endpoint::default(), &f);
        i.file_push(
            "eb-a",
            "/run/errand-box/env.sh",
            b"export K='secret'\n".to_vec(),
            "0600",
            1000,
            1000,
        )
        .unwrap();
        let calls = f.calls.lock().unwrap();
        assert!(!calls[0].display().contains("secret"));
        assert_eq!(calls[0].stdin.as_deref(), Some(&b"export K='secret'\n"[..]));
    }

    #[test]
    fn rootless_endpoint_sets_incus_dir() {
        let f = Fake::new();
        let e = Endpoint {
            incus_dir: Some("/var/lib/u/.local/share/errand-box/incus".to_owned()),
            ..Endpoint::default()
        };
        let i = Incus::new(e, &f);
        let _ = i.run(["info"]);
        let calls = f.calls.lock().unwrap();
        assert_eq!(
            calls[0].env["INCUS_DIR"],
            "/var/lib/u/.local/share/errand-box/incus"
        );
    }

    #[test]
    fn the_client_never_reads_config_from_the_callers_home() {
        let f = Fake::new();
        let i = Incus::new(Endpoint::default(), &f);
        let _ = i.run(["info"]);
        let _ = i.query("/1.0");
        for c in f.calls.lock().unwrap().iter() {
            assert!(c.env["INCUS_CONF"].ends_with("/incus-client"), "{c:?}");
        }
    }
}
