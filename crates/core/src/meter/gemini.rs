//! Gemini CLI (`~/.gemini/tmp/*/chats/session-*.jsonl`): the message is written again when
//! its tokens arrive, so the last record per id wins. Qwen Code (`~/.qwen/projects/*/chats/
//! *.jsonl`): Gemini-style `usageMetadata` plus the auth type of the session.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{in_window, read_text, str_at, time_at, u64_at, Read, Request};
use crate::model::{Billing, Credit, Tokens};

pub fn read_gemini(files: &[PathBuf], since: DateTime<Utc>) -> Read {
    let mut out = Read::default();
    for f in files {
        let text = match read_text(f) {
            Ok(t) => t,
            Err(e) => {
                out.errors.push(e);
                continue;
            }
        };
        let mut by_id: HashMap<String, Request> = HashMap::new();
        let mut anon = 0u64;
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if v.get("type").and_then(Value::as_str) != Some("gemini") {
                continue;
            }
            let Some(t) = v.get("tokens").filter(|x| x.is_object()) else {
                continue;
            };
            let when = time_at(&v, "timestamp", "");
            if !in_window(when, since) {
                continue;
            }
            let cached = u64_at(t, "cached");
            let input = u64_at(t, "input");
            let id = str_at(&v, "id").unwrap_or_else(|| {
                anon += 1;
                format!("anon-{anon}")
            });
            by_id.insert(
                id,
                Request {
                    model: str_at(&v, "model").unwrap_or_else(|| "unknown".into()),
                    when,
                    tokens: Tokens {
                        input: input.saturating_sub(cached),
                        output: u64_at(t, "output"),
                        cache_write: 0,
                        cache_read: cached,
                        reasoning: u64_at(t, "thoughts"),
                        requests: 1,
                    },
                    billing: Billing::default(),
                    project: None,
                    host_cost_usd: None,
                    provider_hint: Some("google".into()),
                    credit_hint: None,
                    source: f.clone(),
                },
            );
        }
        out.files.push((f.clone(), by_id.len() as u64));
        out.requests.extend(by_id.into_values());
    }
    out
}

pub fn read_qwen(files: &[PathBuf], since: DateTime<Utc>) -> Read {
    let mut out = Read::default();
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
            let Some(u) = v.get("usageMetadata").filter(|x| x.is_object()) else {
                continue;
            };
            let when = time_at(&v, "timestamp", "");
            if !in_window(when, since) {
                continue;
            }
            let cached = u64_at(u, "cachedContentTokenCount");
            let auth = str_at(&v, "executionContext.authType");
            let credit = auth.as_deref().map(|a| match a {
                "qwen-oauth" => Credit::Subscription { plan: None },
                _ => Credit::ApiKey,
            });
            let provider = auth.as_deref().map(|a| match a {
                "anthropic" => "anthropic",
                "gemini" | "vertex-ai" => "google",
                "qwen-oauth" => "alibaba",
                _ => "openai",
            });
            count += 1;
            out.requests.push(Request {
                model: str_at(&v, "model")
                    .or_else(|| str_at(&v, "executionContext.modelId"))
                    .unwrap_or_else(|| "unknown".into()),
                when,
                tokens: Tokens {
                    input: u64_at(u, "promptTokenCount").saturating_sub(cached),
                    output: u64_at(u, "candidatesTokenCount"),
                    cache_write: 0,
                    cache_read: cached,
                    reasoning: u64_at(u, "thoughtsTokenCount"),
                    requests: 1,
                },
                billing: Billing::default(),
                project: project_slug(f),
                host_cost_usd: None,
                provider_hint: provider.map(String::from),
                credit_hint: credit,
                source: f.clone(),
            });
        }
        out.files.push((f.clone(), count));
    }
    out
}

/// `~/.qwen/projects/<slug>/chats/x.jsonl` → the slug (lossy, kept as a label).
fn project_slug(f: &Path) -> Option<PathBuf> {
    let chats = f.parent()?;
    if chats.file_name()? != "chats" {
        return None;
    }
    Some(PathBuf::from(
        chats.parent()?.file_name()?.to_string_lossy().to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_keeps_the_last_record_per_id() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("session-1.jsonl");
        std::fs::write(&f, [
            r#"{"sessionId":"s","projectHash":"h"}"#,
            r#"{"id":"m1","type":"gemini","timestamp":"2026-10-17T10:00:00Z","model":"gemini-2.5-pro","tokens":{"input":100,"output":10,"cached":0,"thoughts":2,"tool":0,"total":110}}"#,
            r#"{"id":"m1","type":"gemini","timestamp":"2026-10-17T10:00:01Z","model":"gemini-2.5-pro","tokens":{"input":100,"output":12,"cached":60,"thoughts":2,"tool":0,"total":112}}"#,
        ].join("\n")).unwrap();
        let r = read_gemini(&[f], "2026-10-01T00:00:00Z".parse().unwrap());
        assert_eq!(r.requests.len(), 1);
        assert_eq!(r.requests[0].tokens.output, 12);
        assert_eq!(r.requests[0].tokens.input, 40);
        assert_eq!(r.requests[0].tokens.cache_read, 60);
    }

    #[test]
    fn qwen_reads_the_auth_type() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path().join("projects/-home-me-p/chats");
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("s.jsonl");
        std::fs::write(&f, r#"{"type":"assistant","timestamp":"2026-10-17T10:00:00Z","sessionId":"s","model":"qwen3-coder","usageMetadata":{"promptTokenCount":300,"candidatesTokenCount":20,"cachedContentTokenCount":100,"totalTokenCount":320},"executionContext":{"modelId":"qwen3-coder","authType":"openai"}}"#).unwrap();
        let r = read_qwen(&[f], "2026-10-01T00:00:00Z".parse().unwrap());
        assert_eq!(r.requests.len(), 1);
        assert_eq!(r.requests[0].credit_hint, Some(Credit::ApiKey));
        assert_eq!(r.requests[0].tokens.input, 200);
        assert_eq!(
            r.requests[0]
                .project
                .as_deref()
                .map(|p| p.to_string_lossy().to_string())
                .as_deref(),
            Some("-home-me-p")
        );
    }
}
