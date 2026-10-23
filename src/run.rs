//! Runs external commands with stdin closed and a deadline.
//!
//! Every external program errand-box calls goes through `Runner`, so tests can
//! replace it with a script. Two rules hold for every call:
//! - stdin is `/dev/null` unless the caller gives bytes. The Incus client
//!   reads instance YAML from a stdin that is not a terminal and blocks until
//!   it closes (docs/edge-cases.md, E-11).
//! - a deadline bounds the call; on expiry the child is killed and the call
//!   reports a timeout instead of hanging its caller.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
#[cfg(test)]
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One external call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
}

impl Cmd {
    pub fn new(program: &str) -> Self {
        Cmd {
            program: program.to_owned(),
            args: Vec::new(),
            env: BTreeMap::new(),
            stdin: None,
            timeout: Duration::from_secs(120),
        }
    }

    pub fn arg(mut self, a: impl Into<String>) -> Self {
        self.args.push(a.into());
        self
    }

    pub fn args<I, S>(mut self, it: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(it.into_iter().map(Into::into));
        self
    }

    pub fn env(mut self, k: &str, v: &str) -> Self {
        self.env.insert(k.to_owned(), v.to_owned());
        self
    }

    pub fn stdin(mut self, bytes: Vec<u8>) -> Self {
        self.stdin = Some(bytes);
        self
    }

    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }

    /// The command line, for test assertions. It never carries stdin bytes.
    #[cfg(test)]
    pub fn display(&self) -> String {
        let mut s = self.program.clone();
        for a in &self.args {
            s.push(' ');
            if a.is_empty() || a.contains(|c: char| c.is_whitespace() || c == '\'') {
                s.push('\'');
                s.push_str(&a.replace('\'', "'\\''"));
                s.push('\'');
            } else {
                s.push_str(a);
            }
        }
        s
    }
}

/// What a call returned.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Out {
    /// The exit code, or -1 when the program could not start or a signal
    /// ended it.
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

impl Out {
    pub fn ok(&self) -> bool {
        self.code == 0 && !self.timed_out
    }

    /// stderr, else stdout, trimmed: the text to show when a call fails.
    pub fn why(&self) -> String {
        if self.timed_out {
            return "timed out".to_owned();
        }
        let e = self.stderr.trim();
        if e.is_empty() {
            self.stdout.trim().to_owned()
        } else {
            e.to_owned()
        }
    }
}

pub trait Runner: Send + Sync {
    fn run(&self, cmd: &Cmd) -> Out;
}

/// The real runner.
pub struct System;

impl Runner for System {
    fn run(&self, cmd: &Cmd) -> Out {
        run_system(cmd)
    }
}

fn run_system(cmd: &Cmd) -> Out {
    let mut c = Command::new(&cmd.program);
    c.args(&cmd.args);
    for (k, v) in &cmd.env {
        c.env(k, v);
    }
    c.stdin(if cmd.stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    c.stdout(Stdio::piped());
    c.stderr(Stdio::piped());
    let mut child = match c.spawn() {
        Ok(ch) => ch,
        Err(e) => {
            return Out {
                code: -1,
                stderr: format!("could not start {}: {e}", cmd.program),
                ..Out::default()
            };
        }
    };
    if let (Some(bytes), Some(mut sin)) = (cmd.stdin.clone(), child.stdin.take()) {
        std::thread::spawn(move || {
            let _ = sin.write_all(&bytes);
        });
    }
    let mut so = child.stdout.take();
    let mut se = child.stderr.take();
    let t_out = std::thread::spawn(move || read_all(so.as_mut()));
    let t_err = std::thread::spawn(move || read_all(se.as_mut()));
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {}
            Err(_) => break None,
        }
        if start.elapsed() >= cmd.timeout {
            timed_out = true;
            let _ = child.kill();
            break child.wait().ok();
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let stdout = t_out.join().unwrap_or_default();
    let stderr = t_err.join().unwrap_or_default();
    Out {
        code: status.and_then(|s| s.code()).unwrap_or(-1),
        stdout,
        stderr,
        timed_out,
    }
}

fn read_all<R: Read>(r: Option<&mut R>) -> String {
    let mut buf = Vec::new();
    if let Some(r) = r {
        let _ = r.read_to_end(&mut buf);
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// A scripted runner for tests. Each rule matches a prefix of the command
/// line and answers with a fixed result; every call is recorded.
#[cfg(test)]
#[derive(Clone, Default)]
pub struct Fake {
    pub calls: Arc<Mutex<Vec<Cmd>>>,
    rules: Arc<Mutex<Vec<(String, Out)>>>,
}

#[cfg(test)]
impl Fake {
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer every call whose display line starts with `prefix`. The last
    /// matching rule wins, so a later rule can override an earlier one.
    pub fn on(&self, prefix: &str, code: i32, stdout: &str) -> &Self {
        self.rules.lock().unwrap().push((
            prefix.to_owned(),
            Out {
                code,
                stdout: stdout.to_owned(),
                ..Out::default()
            },
        ));
        self
    }

    pub fn lines(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(Cmd::display)
            .collect()
    }
}

#[cfg(test)]
impl Runner for Fake {
    fn run(&self, cmd: &Cmd) -> Out {
        self.calls.lock().unwrap().push(cmd.clone());
        let line = cmd.display();
        let rules = self.rules.lock().unwrap();
        rules
            .iter()
            .rev()
            .find(|(p, _)| line.starts_with(p.as_str()))
            .map(|(_, o)| o.clone())
            .unwrap_or(Out {
                code: 0,
                ..Out::default()
            })
    }
}

/// The value of an environment variable, if set and valid UTF-8.
pub fn env(name: &str) -> Option<String> {
    std::env::var_os(name).and_then(|v: OsString| v.into_string().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdin_is_closed_by_default() {
        // `cat` with an open stdin would wait for ever; closed, it ends at once.
        let out = System.run(&Cmd::new("cat").timeout(Duration::from_secs(5)));
        assert!(out.ok(), "{out:?}");
        assert_eq!(out.stdout, "");
    }

    #[test]
    fn stdin_bytes_reach_the_program() {
        let out = System.run(&Cmd::new("cat").stdin(b"abc".to_vec()));
        assert_eq!(out.stdout, "abc");
    }

    #[test]
    fn a_deadline_kills_a_hung_program() {
        let t = Instant::now();
        let out = System.run(
            &Cmd::new("sleep")
                .arg("30")
                .timeout(Duration::from_millis(200)),
        );
        assert!(out.timed_out);
        assert!(!out.ok());
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_missing_program_is_code_minus_one() {
        let out = System.run(&Cmd::new("/nonexistent/errand-box-test"));
        assert_eq!(out.code, -1);
        assert!(out.why().contains("could not start"));
    }

    #[test]
    fn display_quotes_spaces_and_quotes() {
        let c = Cmd::new("incus").arg("config").arg("a b").arg("it's");
        assert_eq!(c.display(), "incus config 'a b' 'it'\\''s'");
    }

    #[test]
    fn fake_records_and_answers() {
        let f = Fake::new();
        f.on("incus info", 0, "server_version: 6.0.6");
        f.on("incus list", 1, "");
        assert_eq!(
            f.run(&Cmd::new("incus").arg("info")).stdout,
            "server_version: 6.0.6"
        );
        assert_eq!(f.run(&Cmd::new("incus").arg("list")).code, 1);
        assert_eq!(f.lines(), vec!["incus info", "incus list"]);
    }
}
