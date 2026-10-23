//! errand's `config.json`, made from two files the operator writes:
//! - `errand-box.env`: settings that are not secret (channel, users,
//!   provider name and endpoint, model, git identity);
//! - `creds.env`: the secrets (chat token, provider key, GitHub token).
//!
//! The result is written to `<errand home>/.config/errand/config.json`,
//! mode 0600, owned by the errand account. No value is printed: a summary
//! names only which keys were set.

use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Parse `KEY=VALUE` lines. `#` starts a comment line; values may be wrapped
/// in single or double quotes; `export ` before a key is allowed.
pub fn parse_env(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut m = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let l = l.strip_prefix("export ").unwrap_or(l);
        let (k, v) = l
            .split_once('=')
            .ok_or_else(|| format!("line {}: not KEY=VALUE", n + 1))?;
        let k = k.trim();
        if !crate::policy::valid_env_name(k) {
            return Err(format!("line {}: {k:?} is not a variable name", n + 1));
        }
        let v = v.trim();
        let v = if v.len() >= 2
            && ((v.starts_with('"') && v.ends_with('"'))
                || (v.starts_with('\'') && v.ends_with('\'')))
        {
            &v[1..v.len() - 1]
        } else {
            v
        };
        m.insert(k.to_owned(), v.to_owned());
    }
    Ok(m)
}

fn list(v: Option<&String>) -> Vec<String> {
    v.map(|s| {
        s.split(',')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(str::to_owned)
            .collect()
    })
    .unwrap_or_default()
}

/// errand's default egress allowlist (its config.example.json).
pub const DEFAULT_ALLOW: &[&str] = &[
    "github.com",
    "api.github.com",
    "*.githubusercontent.com",
    "crates.io",
    "static.crates.io",
    "index.crates.io",
    "jsr.io",
    "deno.land",
    "registry.npmjs.org",
    "pypi.org",
    "files.pythonhosted.org",
    "debian.org",
    "*.debian.org",
    "registry-1.docker.io",
    "auth.docker.io",
    "production.cloudflare.docker.com",
    "mirror.gcr.io",
];

/// Build errand's configuration. `home` is the errand account's home.
pub fn build(
    settings: &BTreeMap<String, String>,
    creds: &BTreeMap<String, String>,
    home: &str,
) -> Result<Value, String> {
    let need = |m: &BTreeMap<String, String>, k: &str, file: &str| {
        m.get(k)
            .filter(|v| !v.is_empty())
            .cloned()
            .ok_or_else(|| format!("{file} has no {k}"))
    };
    let channel = need(settings, "ERRAND_CHANNEL_ID", "errand-box.env")?;
    let allowed = list(settings.get("ERRAND_ALLOWED_USER_IDS"));
    if allowed.is_empty() {
        return Err("errand-box.env has no ERRAND_ALLOWED_USER_IDS".into());
    }
    let provider = need(settings, "ERRAND_PROVIDER", "errand-box.env")?;
    let token = need(creds, "ERRAND_CHAT_TOKEN", "creds.env")?;
    let key = need(creds, "ERRAND_PROVIDER_CREDENTIAL", "creds.env")?;

    let mut p = serde_json::Map::new();
    p.insert("credential".into(), json!(key));
    if let Some(n) = settings
        .get("ERRAND_PROVIDER_CREDENTIAL_NAME")
        .filter(|v| !v.is_empty())
    {
        p.insert("credentialName".into(), json!(n));
    }
    if let Some(u) = settings
        .get("ERRAND_PROVIDER_BASE_URL")
        .filter(|v| !v.is_empty())
    {
        p.insert("baseUrl".into(), json!(u));
    }
    if let Some(k) = settings
        .get("ERRAND_PROVIDER_KIND")
        .filter(|v| !v.is_empty())
    {
        if !matches!(k.as_str(), "openai" | "anthropic") {
            return Err(format!(
                "ERRAND_PROVIDER_KIND must be openai or anthropic, not {k:?}"
            ));
        }
        p.insert("kind".into(), json!(k));
    }
    let mut agent =
        json!({ "provider": provider, "providers": { provider.clone(): Value::Object(p) } });
    if let Some(m) = settings.get("ERRAND_MODEL").filter(|v| !v.is_empty()) {
        agent["model"] = json!(m);
    }
    let mut allow = list(settings.get("ERRAND_EGRESS_ALLOW"));
    if allow.is_empty() {
        allow = DEFAULT_ALLOW.iter().map(|s| (*s).to_owned()).collect();
    }
    let mut cfg = json!({
        "chat": {
            "token": token,
            "channelId": channel,
            "allowedUserIds": allowed,
            "blockedUserIds": [],
            "operatorUserIds": list(settings.get("ERRAND_OPERATOR_USER_IDS")),
            "startOnMention": settings.get("ERRAND_START_ON_MENTION").is_some_and(|v| v == "true"),
        },
        "agent": agent,
        "projectRoot": format!("{home}/projects"),
        "stateDir": format!("{home}/state"),
        "sandbox": {
            "backend": "bailey",
            "requireFullEnforcement": true,
            "network": "restricted",
            "egress": { "mode": "proxy", "allow": allow },
            "gracePeriodMs": 10000,
        },
        "output": { "forwardToolOutput": false, "postDiffs": true },
        "shutdown": { "allowedUserIds": list(settings.get("ERRAND_OPERATOR_USER_IDS")) },
        "web": { "host": "127.0.0.1", "port": 8787, "observer": false },
        "limits": { "maxConcurrentTurns": 2, "maxLiveSessions": 4 },
        "timeouts": { "idleMs": 1800000, "startupMs": 60000 },
    });
    if let Some(gt) = creds.get("ERRAND_GITHUB_TOKEN").filter(|v| !v.is_empty()) {
        cfg["github"] = json!({
            "token": gt,
            "userName": need(settings, "ERRAND_GITHUB_USER_NAME", "errand-box.env")?,
            "userEmail": need(settings, "ERRAND_GITHUB_USER_EMAIL", "errand-box.env")?,
        });
    }
    Ok(cfg)
}

/// The keys that were set, for the summary. Never the values.
pub fn summary(settings: &BTreeMap<String, String>, creds: &BTreeMap<String, String>) -> String {
    let s: Vec<&str> = settings.keys().map(String::as_str).collect();
    let c: Vec<&str> = creds.keys().map(String::as_str).collect();
    format!("settings: {}; secrets: {}", s.join(", "), c.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> BTreeMap<String, String> {
        parse_env("# test\nERRAND_CHANNEL_ID=123\nERRAND_ALLOWED_USER_IDS=1, 2\nERRAND_PROVIDER=gw\nexport ERRAND_PROVIDER_BASE_URL='https://gw.example/v1'\nERRAND_MODEL=\"m-1\"\n").unwrap()
    }

    fn creds() -> BTreeMap<String, String> {
        parse_env(
            "ERRAND_CHAT_TOKEN=test-chat-token\nERRAND_PROVIDER_CREDENTIAL=test-provider-key\n",
        )
        .unwrap()
    }

    #[test]
    fn builds_a_bailey_config_with_the_broker() {
        let c = build(&settings(), &creds(), "/var/lib/errand").unwrap();
        assert_eq!(c["sandbox"]["backend"], "bailey");
        assert_eq!(c["sandbox"]["egress"]["mode"], "proxy");
        assert_eq!(c["stateDir"], "/var/lib/errand/state");
        assert_eq!(
            c["agent"]["providers"]["gw"]["baseUrl"],
            "https://gw.example/v1"
        );
        assert_eq!(c["agent"]["model"], "m-1");
        assert_eq!(c["chat"]["allowedUserIds"], json!(["1", "2"]));
        assert!(c.get("github").is_none());
    }

    #[test]
    fn missing_values_name_the_file_and_key() {
        let mut s = settings();
        s.remove("ERRAND_CHANNEL_ID");
        assert_eq!(
            build(&s, &creds(), "/h").unwrap_err(),
            "errand-box.env has no ERRAND_CHANNEL_ID"
        );
        let mut c = creds();
        c.remove("ERRAND_CHAT_TOKEN");
        assert_eq!(
            build(&settings(), &c, "/h").unwrap_err(),
            "creds.env has no ERRAND_CHAT_TOKEN"
        );
    }

    #[test]
    fn github_needs_the_identity_too() {
        let mut c = creds();
        c.insert("ERRAND_GITHUB_TOKEN".into(), "test-gh".into());
        assert!(
            build(&settings(), &c, "/h")
                .unwrap_err()
                .contains("ERRAND_GITHUB_USER_NAME")
        );
    }

    #[test]
    fn summary_has_no_values() {
        let s = summary(&settings(), &creds());
        assert!(!s.contains("test-chat-token") && !s.contains("test-provider-key"));
        assert!(s.contains("ERRAND_CHAT_TOKEN"));
    }

    #[test]
    fn env_parsing_refuses_junk() {
        assert!(parse_env("NOEQUALS\n").is_err());
        assert!(parse_env("1BAD=x\n").is_err());
    }
}
