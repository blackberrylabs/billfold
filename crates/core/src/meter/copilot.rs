//! Copilot CLI: `~/.copilot/session-state/<id>/events.jsonl` — `session.shutdown` events
//! carry cumulative per-model metrics for the session; a resumed session writes another
//! shutdown, so the last one per session is the total. `inputTokens` includes both cache
//! buckets. Credits (`totalNanoAiu`, 1 AIU = $0.01) are the host's own cost figure.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{in_window, read_text, time_at, u64_at, Read, Request};
use crate::model::{Billing, Credit, Tokens};

/// model → (tokens, AIU credits)
type PerModel = HashMap<String, (Tokens, f64)>;

pub fn read(files: &[PathBuf], since: DateTime<Utc>) -> Read {
    let mut out = Read::default();
    for f in files {
        let text = match read_text(f) {
            Ok(t) => t,
            Err(e) => {
                out.errors.push(e);
                continue;
            }
        };
        // the last shutdown of the file wins
        let mut last: Option<(Option<DateTime<Utc>>, PerModel)> = None;
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if v.get("type").and_then(Value::as_str) != Some("session.shutdown") {
                continue;
            }
            let when = time_at(&v, "timestamp", "");
            let Some(Value::Object(metrics)) = v.get("data").and_then(|d| d.get("modelMetrics"))
            else {
                continue;
            };
            let mut per: PerModel = HashMap::new();
            for (model, m) in metrics {
                let u = m.get("usage").cloned().unwrap_or(Value::Null);
                let cr = u64_at(&u, "cacheReadTokens");
                let cw = u64_at(&u, "cacheWriteTokens");
                let t = Tokens {
                    input: u64_at(&u, "inputTokens").saturating_sub(cr + cw),
                    output: u64_at(&u, "outputTokens"),
                    cache_write: cw,
                    cache_read: cr,
                    reasoning: u64_at(&u, "reasoningTokens"),
                    requests: u64_at(m, "requests.count").max(1),
                };
                let aiu = m.get("totalNanoAiu").and_then(Value::as_f64).unwrap_or(0.0) / 1e9;
                per.insert(model.clone(), (t, aiu));
            }
            last = Some((when, per));
        }
        let Some((when, per)) = last else { continue };
        if !in_window(when, since) {
            continue;
        }
        let mut n = 0u64;
        for (model, (t, aiu)) in per {
            n += t.requests;
            out.requests.push(Request {
                model,
                when,
                tokens: t,
                billing: Billing::default(),
                project: None,
                host_cost_usd: if aiu > 0.0 { Some(aiu * 0.01) } else { None },
                provider_hint: Some("github".into()),
                credit_hint: Some(Credit::Subscription { plan: None }),
                source: f.clone(),
            });
        }
        out.files.push((f.clone(), n));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_shutdown_per_session() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("events.jsonl");
        std::fs::write(&f, [
            r#"{"type":"session.start","timestamp":"2026-10-17T09:00:00Z"}"#,
            r#"{"type":"session.shutdown","timestamp":"2026-10-17T09:30:00Z","data":{"modelMetrics":{"gpt-6.1-sol":{"usage":{"inputTokens":1000,"outputTokens":10,"cacheReadTokens":300,"cacheWriteTokens":100,"reasoningTokens":2},"requests":{"count":3},"totalNanoAiu":500000000}},"totalNanoAiu":500000000}}"#,
            r#"{"type":"session.shutdown","timestamp":"2026-10-17T10:30:00Z","data":{"modelMetrics":{"gpt-6.1-sol":{"usage":{"inputTokens":2000,"outputTokens":20,"cacheReadTokens":600,"cacheWriteTokens":200,"reasoningTokens":4},"requests":{"count":6},"totalNanoAiu":1000000000}},"totalNanoAiu":1000000000}}"#,
        ].join("\n")).unwrap();
        let r = read(&[f], "2026-10-01T00:00:00Z".parse().unwrap());
        assert_eq!(r.requests.len(), 1);
        assert_eq!(r.requests[0].tokens.input, 1200);
        assert_eq!(r.requests[0].tokens.requests, 6);
        assert_eq!(r.requests[0].host_cost_usd, Some(0.01));
    }
}
