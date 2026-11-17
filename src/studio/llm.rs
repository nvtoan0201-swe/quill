//! Talking to Claude, two ways:
//! - **Claude Code** (default): the `claude` CLI already signed in on this
//!   computer, run in print mode with no tools. No API key needed.
//! - **Anthropic API**: the Messages API over plain HTTP (there is no official
//!   Rust SDK), with a key.
//! Blocking: call it from a background task.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
const API_VERSION: &str = "2026-10-21";
/// Server-side fallback: if the model declines, Anthropic retries on a suitable model.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// Model and effort used through Claude Code.
pub const CLI_MODEL: &str = "haiku";
pub const CLI_EFFORT: &str = "high";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Backend {
    #[default]
    ClaudeCode,
    Api,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub backend: Backend,
    /// From the Coach tab, else `ANTHROPIC_API_KEY`.
    pub api_key: String,
    pub model: String,
    /// low | medium | high | xhigh | max
    pub effort: String,
    pub base_url: String,
    /// The `claude` executable, when found.
    pub cli: Option<PathBuf>,
}

impl Config {
    pub fn with_backend(backend: Backend, settings_key: &str, model: &str) -> Config {
        let api_key = if settings_key.trim().is_empty() { std::env::var("ANTHROPIC_API_KEY").unwrap_or_default() } else { settings_key.trim().to_string() };
        let (default_model, effort) = match backend {
            Backend::ClaudeCode => (CLI_MODEL, CLI_EFFORT),
            Backend::Api => (DEFAULT_MODEL, "medium"),
        };
        Config {
            backend,
            api_key,
            model: if model.trim().is_empty() { default_model.into() } else { model.trim().into() },
            effort: effort.into(),
            base_url: std::env::var("ANTHROPIC_BASE_URL").unwrap_or_else(|_| "https://api.anthropic.com".into()),
            cli: if backend == Backend::ClaudeCode { find_claude() } else { None },
        }
    }

    fn auth_token() -> Option<String> {
        std::env::var("ANTHROPIC_AUTH_TOKEN").ok().filter(|t| !t.is_empty())
    }

    pub fn available(&self) -> bool {
        match self.backend {
            Backend::ClaudeCode => self.cli.is_some(),
            Backend::Api => !self.api_key.is_empty() || Self::auth_token().is_some(),
        }
    }

    pub fn describe(&self) -> String {
        match self.backend {
            Backend::ClaudeCode => format!("Claude Code · {} · effort {}", self.model, self.effort),
            Backend::Api => format!("Anthropic API · {}", self.model),
        }
    }
}

/// Finds the `claude` CLI. Apps started from the desktop often lack the shell's
/// PATH (nvm, ~/.local/bin), so the usual install locations are searched too.
pub fn find_claude() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QUILL_CLAUDE") {
        return Some(PathBuf::from(p)).filter(|p| p.is_file());
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".claude/local"));
        dirs.push(home.join(".npm-global/bin"));
        dirs.push(home.join(".bun/bin"));
        // nvm: newest node version first.
        if let Ok(rd) = std::fs::read_dir(home.join(".nvm/versions/node")) {
            let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path().join("bin")).collect();
            v.sort();
            v.reverse();
            dirs.extend(v);
        }
    }
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.push(PathBuf::from("/usr/bin"));
    dirs.into_iter().map(|d| d.join("claude")).find(|p| p.is_file())
}

/// Arguments for a one-shot, tool-free answer in print mode.
pub fn cli_args(cfg: &Config, system: &str) -> Vec<String> {
    [
        "-p", "--output-format", "json", "--model", &cfg.model, "--effort", &cfg.effort,
        // A writing coach needs no tools, MCP servers or saved sessions.
        "--tools", "", "--strict-mcp-config", "--no-session-persistence", "--system-prompt", system,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The answer from `claude -p --output-format json`.
pub fn parse_cli_output(stdout: &str) -> Result<String, String> {
    // Some versions print warnings before the JSON line.
    let line = stdout.lines().rev().find(|l| l.trim_start().starts_with('{')).ok_or_else(|| {
        let first = stdout.lines().next().unwrap_or("").trim();
        if first.is_empty() { "Claude Code returned nothing.".to_string() } else { format!("Claude Code: {first}") }
    })?;
    let v: Value = serde_json::from_str(line).map_err(|e| format!("Unexpected Claude Code output: {e}"))?;
    let result = v.get("result").and_then(Value::as_str).unwrap_or("").to_string();
    if v.get("is_error").and_then(Value::as_bool).unwrap_or(false) || v.get("subtype").and_then(Value::as_str).is_some_and(|s| s != "success") {
        let msg = if result.is_empty() { v.get("subtype").and_then(Value::as_str).unwrap_or("error").to_string() } else { result };
        return Err(format!("Claude Code: {msg}"));
    }
    if v.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
        return Err("The assistant declined this request.".into());
    }
    if result.trim().is_empty() {
        return Err("The assistant returned an empty answer.".into());
    }
    Ok(result)
}

fn complete_cli(cfg: &Config, bin: &Path, system: &str, user: &str) -> Result<String, String> {
    // `claude` is a Node script: make sure its own directory (where nvm puts
    // `node`) is on PATH even when Quill was started from the desktop.
    let mut path = bin.parent().map(|d| d.as_os_str().to_owned()).unwrap_or_default();
    if let Some(p) = std::env::var_os("PATH") {
        path.push(":");
        path.push(p);
    }
    let mut child = Command::new(bin)
        .args(cli_args(cfg, system))
        .env("PATH", path)
        // A neutral working directory, so no project instructions are picked up.
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start Claude Code ({}): {e}", bin.display()))?;
    child.stdin.take().ok_or("no stdin")?.write_all(user.as_bytes()).map_err(|e| format!("Claude Code: {e}"))?;
    let out = child.wait_with_output().map_err(|e| format!("Claude Code: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    match parse_cli_output(&stdout) {
        Ok(t) => Ok(t),
        Err(e) if stdout.trim().is_empty() => {
            let err = String::from_utf8_lossy(&out.stderr);
            let first = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
            Err(if first.is_empty() { e } else { format!("Claude Code: {first} (run `claude` once in a terminal to sign in)") })
        }
        Err(e) => Err(e),
    }
}

fn key_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("quill").join("anthropic-key"))
}

/// The API key saved from the Coach tab (a 0600 file, like publishing accounts).
pub fn load_key() -> String {
    key_path().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| s.trim().to_string()).unwrap_or_default()
}

pub fn save_key(key: &str) -> std::io::Result<()> {
    let p = key_path().ok_or_else(|| std::io::Error::other("no config directory"))?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    if key.trim().is_empty() {
        let _ = std::fs::remove_file(&p);
        return Ok(());
    }
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&p)?;
    f.write_all(key.trim().as_bytes())
}

pub fn request_body(cfg: &Config, system: &str, user: &str, max_tokens: u32) -> Value {
    json!({
        "model": cfg.model,
        "max_tokens": max_tokens,
        "system": system,
        "messages": [{"role": "user", "content": user}],
        "output_config": {"effort": cfg.effort},
        "fallbacks": "default",
    })
}

/// The text of a Messages API response, or an explanation of why there is none.
pub fn parse_response(v: &Value) -> Result<String, String> {
    if let Some(err) = v.get("error") {
        return Err(err.get("message").and_then(Value::as_str).unwrap_or("API error").to_string());
    }
    let stop = v.get("stop_reason").and_then(Value::as_str).unwrap_or("");
    if stop == "refusal" {
        let why = v.pointer("/stop_details/explanation").and_then(Value::as_str).unwrap_or("");
        return Err(format!("The assistant declined this request. {why}").trim().to_string());
    }
    let text: String = v
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| blocks.iter().filter(|b| b.get("type").and_then(Value::as_str) == Some("text")).filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join(""))
        .unwrap_or_default();
    if text.trim().is_empty() {
        return Err("The assistant returned an empty answer.".into());
    }
    if stop == "max_tokens" {
        return Ok(format!("{text}\n\n_(answer cut off at the length limit)_"));
    }
    Ok(text)
}

pub fn complete(cfg: &Config, system: &str, user: &str, max_tokens: u32) -> Result<String, String> {
    if cfg.backend == Backend::ClaudeCode {
        let bin = cfg.cli.as_deref().ok_or("Claude Code was not found. Install it (npm install -g @anthropic-ai/claude-code) and sign in, or switch the coach to the Anthropic API.")?;
        return complete_cli(cfg, bin, system, user);
    }
    if !cfg.available() {
        return Err("No API key. Add your Anthropic API key in the Coach tab, or set ANTHROPIC_API_KEY.".into());
    }
    let url = format!("{}/v1/messages", cfg.base_url.trim_end_matches('/'));
    let body = request_body(cfg, system, user, max_tokens);
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout(Duration::from_secs(600)).build();
    let mut delay = Duration::from_secs(2);
    for attempt in 0..3 {
        let mut req = agent
            .post(&url)
            .set("content-type", "application/json")
            .set("anthropic-version", API_VERSION);
        if cfg.api_key.is_empty() {
            let token = Config::auth_token().unwrap_or_default();
            req = req.set("authorization", &format!("Bearer {token}")).set("anthropic-beta", &format!("oauth-2025-04-20,{FALLBACK_BETA}"));
        } else {
            req = req.set("x-api-key", &cfg.api_key).set("anthropic-beta", FALLBACK_BETA);
        }
        match req.send_json(body.clone()) {
            Ok(resp) => {
                let v: Value = resp.into_json().map_err(|e| format!("Bad response: {e}"))?;
                return parse_response(&v);
            }
            Err(ureq::Error::Status(code, resp)) => {
                let retryable = code == 429 || code == 529 || code >= 500;
                let wait = resp.header("retry-after").and_then(|s| s.parse::<u64>().ok()).map(Duration::from_secs);
                let msg = resp.into_json::<Value>().ok().and_then(|v| parse_response(&v).err()).unwrap_or_else(|| format!("HTTP {code}"));
                if retryable && attempt < 2 {
                    std::thread::sleep(wait.unwrap_or(delay));
                    delay *= 2;
                    continue;
                }
                return Err(match code {
                    401 => "The API key was rejected (401). Check it in the Coach tab.".into(),
                    403 => format!("Access denied (403): {msg}"),
                    429 => "Rate limited (429). Try again in a minute.".into(),
                    _ => format!("API error {code}: {msg}"),
                });
            }
            Err(e) => {
                if attempt < 2 {
                    std::thread::sleep(delay);
                    delay *= 2;
                    continue;
                }
                return Err(format!("Network error: {e}"));
            }
        }
    }
    Err("The request failed.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_shape() {
        let cfg = Config { backend: Backend::Api, api_key: "k".into(), model: DEFAULT_MODEL.into(), effort: "medium".into(), base_url: String::new(), cli: None };
        let b = request_body(&cfg, "sys", "hi", 4000);
        assert_eq!(b["model"], "claude-opus-5-5");
        assert_eq!(b["messages"][0]["role"], "user");
        assert_eq!(b["output_config"]["effort"], "medium");
        assert_eq!(b["fallbacks"], "default");
        assert!(b.get("thinking").is_none() && b.get("temperature").is_none());
    }

    #[test]
    fn responses() {
        let ok = json!({"content": [{"type": "thinking", "thinking": ""}, {"type": "text", "text": "Hello"}], "stop_reason": "end_turn"});
        assert_eq!(parse_response(&ok).unwrap(), "Hello");
        let refused = json!({"content": [], "stop_reason": "refusal", "stop_details": {"type": "refusal", "category": null, "explanation": "x"}});
        assert!(parse_response(&refused).unwrap_err().contains("declined"));
        let err = json!({"type": "error", "error": {"type": "invalid_request_error", "message": "bad"}});
        assert_eq!(parse_response(&err).unwrap_err(), "bad");
        let cut = json!({"content": [{"type": "text", "text": "Part"}], "stop_reason": "max_tokens"});
        assert!(parse_response(&cut).unwrap().contains("cut off"));
    }

    #[test]
    fn local_server_round_trip() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 65536];
            let mut req = String::new();
            loop {
                let n = s.read(&mut buf).unwrap();
                req.push_str(&String::from_utf8_lossy(&buf[..n]));
                if let Some((head, body)) = req.split_once("\r\n\r\n") {
                    let len: usize = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
                    if body.len() >= len {
                        break;
                    }
                }
            }
            let body = r#"{"content":[{"type":"text","text":"Coach says hi"}],"stop_reason":"end_turn"}"#;
            write!(s, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}", body.len(), body).unwrap();
            req
        });
        let cfg = Config { backend: Backend::Api, api_key: "test-key".into(), model: DEFAULT_MODEL.into(), effort: "low".into(), base_url: format!("http://{addr}"), cli: None };
        assert_eq!(complete(&cfg, "sys", "hello", 100).unwrap(), "Coach says hi");
        let req = server.join().unwrap().to_lowercase();
        assert!(req.starts_with("post /v1/messages"));
        assert!(req.contains("x-api-key: test-key") && req.contains("anthropic-version: 2026-10-21"));
    }

    #[test]
    fn cli_output() {
        let ok = r#"{"type":"result","subtype":"success","is_error":false,"result":"Hint: check the verb.","stop_reason":"end_turn"}"#;
        assert_eq!(parse_cli_output(&format!("warning: x\n{ok}\n")).unwrap(), "Hint: check the verb.");
        let err = r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login"}"#;
        assert!(parse_cli_output(err).unwrap_err().contains("Not logged in"));
        assert!(parse_cli_output("").is_err());
    }

    #[test]
    fn cli_round_trip_with_stub() {
        // A stand-in `claude` that checks its arguments and echoes stdin back.
        let dir = std::env::temp_dir().join(format!("quill-claude-stub-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("claude");
        std::fs::write(
            &bin,
            "#!/bin/sh\ncase \"$*\" in *'--model haiku --effort high'*'--tools  --strict-mcp-config'*) ;; *) echo bad args >&2; exit 1;; esac\n\
             input=$(cat)\nprintf '{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"got %s\"}\\n' \"$input\"\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut cfg = Config::with_backend(Backend::ClaudeCode, "", "");
        cfg.cli = Some(bin);
        assert_eq!(cfg.model, "haiku");
        assert_eq!(complete(&cfg, "sys", "hello", 100).unwrap(), "got hello");
        let _ = std::fs::remove_dir_all(dir);
    }
}
