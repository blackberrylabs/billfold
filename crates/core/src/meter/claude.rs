//! Claude Code: `~/.claude/projects/<slug>/<session>.jsonl` and `<session>/subagents/*.jsonl`.
//! One line per content block of the same response → one request per (message.id,
//! requestId); a sidechain replay of a parent message is dropped in favour of the parent.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{in_window, read_text, str_at, time_at, u64_at, Read, Request};
use crate::model::{Billing, Tokens};

pub fn read(files: &[PathBuf], since: DateTime<Utc>) -> Read {
    let mut out = Read::default();
    // message id → (request id, request, is_sidechain)
    let mut seen: HashMap<String, Vec<(String, Request, bool)>> = HashMap::new();
    let mut anon = 0u64;
    for f in files {
        let text = match read_text(f) {
            Ok(t) => t,
            Err(e) => {
                out.errors.push(e);
                continue;
            }
        };
        let mut count = 0u64;
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if v.get("type").and_then(Value::as_str) != Some("assistant") {
                continue;
            }
            let Some(msg) = v.get("message") else {
                continue;
            };
            let model = str_at(msg, "model").unwrap_or_default();
            if model.is_empty() || model == "<synthetic>" {
                continue;
            }
            let Some(usage) = msg.get("usage") else {
                continue;
            };
            if v.get("isApiErrorMessage") == Some(&Value::Bool(true)) {
                continue;
            }
            let when = time_at(&v, "timestamp", "");
            if !in_window(when, since) {
                continue;
            }
            let sidechain = v.get("isSidechain") == Some(&Value::Bool(true));
            let tokens = Tokens {
                input: u64_at(usage, "input_tokens"),
                output: u64_at(usage, "output_tokens"),
                cache_write: u64_at(usage, "cache_creation_input_tokens"),
                cache_read: u64_at(usage, "cache_read_input_tokens"),
                reasoning: u64_at(usage, "output_tokens_details.thinking_tokens"),
                requests: 1,
            };
            let billing = Billing {
                cache_write_1h: u64_at(usage, "cache_creation.ephemeral_1h_input_tokens"),
                tier: str_at(usage, "speed")
                    .filter(|s| s != "standard")
                    .or_else(|| str_at(usage, "service_tier").filter(|s| s != "standard")),
                geo: str_at(usage, "inference_geo").filter(|g| g != "global"),
            };
            let req = Request {
                model,
                when,
                tokens,
                billing,
                project: str_at(&v, "cwd")
                    .map(PathBuf::from)
                    .or_else(|| project_from_path(f)),
                host_cost_usd: v.get("costUSD").and_then(Value::as_f64),
                provider_hint: Some("anthropic".into()),
                credit_hint: None,
                source: f.clone(),
            };
            let mid = str_at(msg, "id").unwrap_or_else(|| {
                anon += 1;
                format!("anon-{anon}")
            });
            let rid = str_at(&v, "requestId").unwrap_or_default();
            count += 1;
            let group = seen.entry(mid).or_default();
            if !group.iter().any(|(r, _, s)| *r == rid && *s == sidechain) {
                group.push((rid, req, sidechain));
            }
        }
        out.files.push((f.clone(), count));
    }
    // per message: the parent's copies when there are any, else the sidechain's
    for group in seen.into_values() {
        let has_parent = group.iter().any(|(_, _, s)| !s);
        for (_, r, s) in group {
            if s == !has_parent {
                out.requests.push(r);
            }
        }
    }
    out
}

/// `~/.claude/projects/<slug>/…` → nothing reliable (the slug is lossy); kept as the slug.
fn project_from_path(f: &Path) -> Option<PathBuf> {
    let mut p = f;
    while let Some(parent) = p.parent() {
        if parent.file_name().is_some_and(|n| n == "projects") {
            return Some(PathBuf::from(p.file_name()?.to_string_lossy().to_string()));
        }
        p = parent;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_request_per_message_and_request_id() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("s.jsonl");
        let line = |block: u32, id: &str, req: &str, side: bool| {
            format!(
                r#"{{"type":"assistant","isSidechain":{side},"timestamp":"2026-10-17T10:00:00Z","cwd":"/home/me/proj","requestId":"{req}","apiBlockIndex":{block},"message":{{"id":"{id}","model":"claude-sonnet-5-5","usage":{{"input_tokens":10,"output_tokens":20,"cache_creation_input_tokens":30,"cache_read_input_tokens":40,"cache_creation":{{"ephemeral_1h_input_tokens":30,"ephemeral_5m_input_tokens":0}},"output_tokens_details":{{"thinking_tokens":5}}}}}}}}"#
            )
        };
        let text = [
            line(0, "msg_1", "req_1", false),
            line(1, "msg_1", "req_1", false),
            line(0, "msg_2", "req_2", false),
            line(0, "msg_2", "req_3", true),
            r#"{"type":"user"}"#.into(),
            r#"{"type":"assistant","message":{"model":"<synthetic>","usage":{}}}"#.into(),
        ]
        .join("\n");
        std::fs::write(&f, text).unwrap();
        let r = read(&[f], "2026-10-01T00:00:00Z".parse().unwrap());
        // msg_1 once (two blocks), msg_2 once (the sidechain replay is dropped)
        assert_eq!(r.requests.len(), 2);
        let t: Tokens = r.requests.iter().fold(Tokens::default(), |mut a, r| {
            a.add(&r.tokens);
            a
        });
        assert_eq!(t.input, 20);
        assert_eq!(t.cache_write, 60);
        assert_eq!(r.requests[0].billing.cache_write_1h, 30);
        assert_eq!(
            r.requests[0].project.as_deref(),
            Some(Path::new("/home/me/proj"))
        );
    }
}
