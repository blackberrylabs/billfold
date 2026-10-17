//! Cline, Roo Code: `tasks/<id>/ui_messages.json` with `api_req_started` entries whose
//! `text` is JSON `{tokensIn, tokensOut, cacheWrites, cacheReads, cost}`; the model from
//! `task_metadata.json` (`model_usage`) when it is there. Cline's newer session manifests
//! (`~/.cline/data/sessions/<id>/<id>.json`) carry one aggregate per session.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{in_window, read_text, str_at, time_at, u64_at, Read, Request};
use crate::model::{Billing, Credit, Tokens};

pub fn read_tasks(files: &[PathBuf], since: DateTime<Utc>) -> Read {
    let mut out = Read::default();
    for f in files {
        let text = match read_text(f) {
            Ok(t) => t,
            Err(e) => {
                out.errors.push(e);
                continue;
            }
        };
        let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let models = models_of(f.parent());
        let mut n = 0u64;
        for e in &entries {
            if e.get("say").and_then(Value::as_str) != Some("api_req_started") {
                continue;
            }
            let Some(info) = e
                .get("text")
                .and_then(Value::as_str)
                .and_then(|t| serde_json::from_str::<Value>(t).ok())
            else {
                continue;
            };
            let when = time_at(e, "ts", "ms");
            if !in_window(when, since) {
                continue;
            }
            let model = models
                .iter()
                .rfind(|(ts, _, _)| when.is_none_or(|w| *ts <= w.timestamp_millis()))
                .or(models.first())
                .map(|(_, m, _)| m.clone())
                .unwrap_or_else(|| "unknown".into());
            let provider = models
                .iter()
                .rfind(|(ts, _, _)| when.is_none_or(|w| *ts <= w.timestamp_millis()))
                .or(models.first())
                .and_then(|(_, _, p)| p.clone());
            n += 1;
            out.requests.push(Request {
                model,
                when,
                tokens: Tokens {
                    input: u64_at(&info, "tokensIn"),
                    output: u64_at(&info, "tokensOut"),
                    cache_write: u64_at(&info, "cacheWrites"),
                    cache_read: u64_at(&info, "cacheReads"),
                    reasoning: 0,
                    requests: 1,
                },
                billing: Billing::default(),
                project: None,
                host_cost_usd: info.get("cost").and_then(Value::as_f64),
                provider_hint: provider.map(|p| crate::wiring::normalise_provider(&p)),
                credit_hint: None,
                source: f.clone(),
            });
        }
        out.files.push((f.clone(), n));
    }
    out
}

/// `task_metadata.json` → `(ts, model_id, provider)` in time order, when present.
fn models_of(dir: Option<&Path>) -> Vec<(i64, String, Option<String>)> {
    let Some(dir) = dir else { return Vec::new() };
    let Ok(text) = std::fs::read_to_string(dir.join("task_metadata.json")) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let mut out: Vec<(i64, String, Option<String>)> = v
        .get("model_usage")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|m| {
                    Some((
                        m.get("ts").and_then(Value::as_i64).unwrap_or(0),
                        str_at(m, "model_id")?,
                        str_at(m, "model_provider_id"),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|(t, _, _)| *t);
    out
}

pub fn read_sessions(files: &[PathBuf], since: DateTime<Utc>) -> Read {
    let mut out = Read::default();
    for f in files {
        let Ok(text) = read_text(f) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let meta = v.get("metadata").unwrap_or(&v);
        let Some(u) = meta
            .get("aggregateUsage")
            .or_else(|| meta.get("usage"))
            .filter(|x| x.is_object())
        else {
            continue;
        };
        let when = time_at(meta, "updatedAt", "ms")
            .or_else(|| time_at(meta, "createdAt", "ms"))
            .or_else(|| crate::util::modified_dt(f));
        if !in_window(when, since) {
            continue;
        }
        out.files.push((f.clone(), 1));
        out.requests.push(Request {
            model: str_at(meta, "model").unwrap_or_else(|| "unknown".into()),
            when,
            tokens: Tokens {
                input: u64_at(u, "inputTokens"),
                output: u64_at(u, "outputTokens"),
                cache_write: u64_at(u, "cacheWriteTokens"),
                cache_read: u64_at(u, "cacheReadTokens"),
                reasoning: 0,
                requests: u64_at(u, "requests").max(1),
            },
            billing: Billing::default(),
            project: str_at(meta, "cwd").map(PathBuf::from),
            host_cost_usd: u.get("totalCost").and_then(Value::as_f64),
            provider_hint: str_at(meta, "provider").map(|p| crate::wiring::normalise_provider(&p)),
            credit_hint: Some(Credit::ApiKey),
            source: f.clone(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_req_started_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path().join("tasks/1");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("task_metadata.json"), r#"{"model_usage":[{"ts":1792281600000,"model_id":"claude-sonnet-5-5","model_provider_id":"anthropic","mode":"act"}]}"#).unwrap();
        let f = d.join("ui_messages.json");
        std::fs::write(&f, r#"[{"ts":1792281601000,"type":"say","say":"api_req_started","text":"{\"request\":\"x\",\"tokensIn\":10,\"tokensOut\":20,\"cacheWrites\":5,\"cacheReads\":6,\"cost\":0.005}"},{"ts":1792281602000,"type":"say","say":"text","text":"hi"}]"#).unwrap();
        let r = read_tasks(&[f], "2026-10-01T00:00:00Z".parse().unwrap());
        assert_eq!(r.requests.len(), 1);
        assert_eq!(r.requests[0].model, "claude-sonnet-5-5");
        assert_eq!(r.requests[0].tokens.cache_read, 6);
        assert_eq!(r.requests[0].host_cost_usd, Some(0.005));
    }
}
