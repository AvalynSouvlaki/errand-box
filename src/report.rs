//! The summary `setup`, `up` and `doctor` print: one line per step, with
//! what it found or changed, and what is degraded on this host.

use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum St {
    /// Already as wanted; nothing changed.
    Ok,
    /// Changed now.
    Changed,
    /// Works with less than the full guarantee; the detail says what is lost.
    Degraded,
    /// Not done; the detail says why.
    Failed,
    /// Not applicable on this host or not asked for.
    Skipped,
}

impl St {
    fn word(self) -> &'static str {
        match self {
            St::Ok => "ok",
            St::Changed => "changed",
            St::Degraded => "DEGRADED",
            St::Failed => "FAILED",
            St::Skipped => "skipped",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub rows: Vec<(String, St, String)>,
    /// Print each row to stderr as it is added: a setup that takes half an
    /// hour, or is interrupted, then shows how far it got.
    pub live: bool,
}

impl Report {
    pub fn live() -> Self {
        Report {
            live: true,
            ..Report::default()
        }
    }

    pub fn add(&mut self, step: &str, st: St, detail: impl Into<String>) {
        let detail = detail.into();
        if self.live {
            eprintln!("errand-box: {step}: {} {detail}", st.word());
        }
        self.rows.push((step.to_owned(), st, detail));
    }

    /// The rows and the counts as JSON, for a program to read.
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "rows": self.rows.iter().map(|(step, st, d)| serde_json::json!({
                "step": step,
                "status": st.word().to_ascii_lowercase(),
                "detail": d,
            })).collect::<Vec<_>>(),
            "changed": self.count(St::Changed),
            "ok": self.count(St::Ok),
            "degraded": self.count(St::Degraded),
            "failed": self.count(St::Failed),
            "skipped": self.count(St::Skipped),
        })
    }

    pub fn count(&self, st: St) -> usize {
        self.rows.iter().filter(|r| r.1 == st).count()
    }

    pub fn failed(&self) -> bool {
        self.count(St::Failed) > 0
    }

    pub fn render(&self) -> String {
        let w = self
            .rows
            .iter()
            .map(|r| r.0.len())
            .max()
            .unwrap_or(4)
            .max(4);
        let mut s = String::new();
        for (step, st, d) in &self.rows {
            let _ = writeln!(s, "{step:<w$}  {:<8}  {d}", st.word());
        }
        let _ = writeln!(
            s,
            "{} changed, {} unchanged, {} degraded, {} failed, {} skipped",
            self.count(St::Changed),
            self.count(St::Ok),
            self.count(St::Degraded),
            self.count(St::Failed),
            self.count(St::Skipped)
        );
        s
    }

    /// 0 when nothing failed, 1 otherwise. Degraded is not a failure; it is
    /// reported.
    pub fn code(&self) -> i32 {
        if self.failed() { 1 } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_code() {
        let mut r = Report::default();
        r.add("a", St::Ok, "x");
        r.add("bb", St::Degraded, "y");
        assert_eq!(r.code(), 0);
        r.add("c", St::Failed, "z");
        assert_eq!(r.code(), 1);
        let t = r.render();
        assert!(t.contains("bb    DEGRADED  y\n"), "{t}");
        assert!(t.ends_with("0 changed, 1 unchanged, 1 degraded, 1 failed, 0 skipped\n"));
    }
}
