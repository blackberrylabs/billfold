//! The meter: one reader per log format the hosts write, each turning its files into
//! requests — model, time, tokens, what the host said about billing — deduplicated as the
//! host's own format requires. Nothing here prices anything; `scan` does, per request.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::model::{Billing, Credit, Quota, Tokens};

pub mod aider;
pub mod amp;
pub mod claude;
pub mod cline;
pub mod codex;
pub mod copilot;
pub mod gemini;
pub mod sqlite;

/// One request as a host logged it.
#[derive(Debug, Clone)]
pub struct Request {
    pub model: String,
    pub when: Option<DateTime<Utc>>,
    pub tokens: Tokens,
    pub billing: Billing,
    pub project: Option<PathBuf>,
    /// The cost the host wrote, USD.
    pub host_cost_usd: Option<f64>,
    /// The provider the host named for this request (`providerID`, `provider_name`).
    pub provider_hint: Option<String>,
    /// What the log itself says about the credit.
    pub credit_hint: Option<Credit>,
    pub source: PathBuf,
}

#[derive(Debug, Default)]
pub struct Read {
    pub requests: Vec<Request>,
    pub quotas: Vec<Quota>,
    /// (file, requests found) for the log-file list.
    pub files: Vec<(PathBuf, u64)>,
    pub errors: Vec<String>,
}

impl Read {
    pub fn merge(&mut self, o: Read) {
        self.requests.extend(o.requests);
        self.quotas.extend(o.quotas);
        self.files.extend(o.files);
        self.errors.extend(o.errors);
    }
}

/// Dispatch on the sheet's reader name.
pub fn read(
    reader: &str,
    host: &str,
    host_name: &str,
    files: &[PathBuf],
    since: DateTime<Utc>,
) -> Read {
    match reader {
        "claude-jsonl" => claude::read(files, since),
        "codex-jsonl" => codex::read(host, host_name, files, since),
        "gemini-jsonl" => gemini::read_gemini(files, since),
        "qwen-jsonl" => gemini::read_qwen(files, since),
        "opencode-sqlite" | "kilo-sqlite" => sqlite::read_opencode(files, since),
        "goose-sqlite" => sqlite::read_goose(files, since),
        "zed-sqlite" => sqlite::read_zed(files, since),
        "aider-markdown" => aider::read(files, since),
        "cline-tasks" => cline::read_tasks(files, since),
        "cline-sessions" => cline::read_sessions(files, since),
        "copilot-jsonl" => copilot::read(files, since),
        "amp-json" => amp::read(files, since),
        other => Read {
            errors: vec![format!("unknown reader {other}")],
            ..Default::default()
        },
    }
}

pub(crate) fn u64_at(v: &Value, path: &str) -> u64 {
    bbl_hostrules::read::get(v, path)
        .and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f.max(0.0) as u64)))
        .unwrap_or(0)
}

pub(crate) fn str_at(v: &Value, path: &str) -> Option<String> {
    bbl_hostrules::read::get(v, path)
        .and_then(Value::as_str)
        .map(String::from)
}

pub(crate) fn time_at(v: &Value, path: &str, unit: &str) -> Option<DateTime<Utc>> {
    bbl_hostrules::read::get(v, path).and_then(|x| crate::util::parse_time(x, unit))
}

/// Read a text file, decompressing zstd by extension.
pub(crate) fn read_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if path.extension().is_some_and(|e| e == "zst") {
        let out =
            zstd::decode_all(&bytes[..]).map_err(|e| format!("{}: zstd: {e}", path.display()))?;
        Ok(String::from_utf8_lossy(&out).to_string())
    } else {
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }
}

/// Keep a request when it is inside the window, or undated (the file's time decides).
pub(crate) fn in_window(when: Option<DateTime<Utc>>, since: DateTime<Utc>) -> bool {
    when.is_none_or(|t| t >= since)
}
