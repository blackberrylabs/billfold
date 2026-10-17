//! Codex CLI: `~/.codex/sessions/Y/M/D/rollout-*.jsonl[.zst]`. Usage arrives as
//! `token_count` events with a cumulative total and a per-turn delta; the model comes from
//! the `turn_context` before it; the last `rate_limits` of the newest file is the quota.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{in_window, read_text, str_at, time_at, u64_at, Read, Request};
use crate::model::{Billing, Credit, Quota, Tokens};

pub fn read(host: &str, host_name: &str, files: &[PathBuf], since: DateTime<Utc>) -> Read {
    let mut out = Read::default();
    let mut newest_quota: Option<(DateTime<Utc>, Vec<Quota>)> = None;
    for f in files {
        let text = match read_text(f) {
            Ok(t) => t,
            Err(e) => {
                out.errors.push(e);
                continue;
            }
        };
        let mut model = String::new();
        let mut cwd: Option<PathBuf> = None;
        let mut plan: Option<String> = None;
        let mut chatgpt_account = false;
        let mut prev_total: Option<Tokens> = None;
        let mut count = 0u64;
        let mut file_quota: Option<(DateTime<Utc>, Vec<Quota>)> = None;
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let t = v.get("type").and_then(Value::as_str).unwrap_or("");
            let payload = v.get("payload").cloned().unwrap_or(Value::Null);
            let when = time_at(&v, "timestamp", "");
            match t {
                "session_meta" => {
                    cwd = str_at(&payload, "cwd").map(PathBuf::from);
                    chatgpt_account = payload
                        .get("creator_account_id")
                        .is_some_and(|x| !x.is_null());
                    if model.is_empty() {
                        if let Some(m) = str_at(&payload, "model") {
                            model = m;
                        }
                    }
                }
                "turn_context" => {
                    if let Some(m) = str_at(&payload, "model") {
                        model = m;
                    }
                    if cwd.is_none() {
                        cwd = str_at(&payload, "cwd").map(PathBuf::from);
                    }
                }
                "event_msg"
                    if payload.get("type").and_then(Value::as_str) == Some("token_count") =>
                {
                    if let Some(rl) = payload.get("rate_limits").filter(|x| !x.is_null()) {
                        plan = str_at(rl, "plan_type").or(plan);
                        let mut qs = Vec::new();
                        for w in ["primary", "secondary"] {
                            if let Some(win) = rl.get(w).filter(|x| !x.is_null()) {
                                qs.push(Quota {
                                    host: host.into(),
                                    host_name: host_name.into(),
                                    window: w.into(),
                                    used_percent: win
                                        .get("used_percent")
                                        .and_then(Value::as_f64)
                                        .unwrap_or(0.0),
                                    window_minutes: win
                                        .get("window_minutes")
                                        .and_then(Value::as_i64),
                                    resets_at: win
                                        .get("resets_at")
                                        .and_then(|x| crate::util::parse_time(x, "s"))
                                        .map(crate::util::rfc3339),
                                    plan: plan.clone(),
                                    as_of: when.map(crate::util::rfc3339),
                                    path: f.clone(),
                                });
                            }
                        }
                        if !qs.is_empty() {
                            file_quota = Some((when.unwrap_or(since), qs));
                        }
                    }
                    let Some(info) = payload.get("info").filter(|x| !x.is_null()) else {
                        continue;
                    };
                    let total = tokens_of(info.get("total_token_usage"));
                    let delta = match info.get("last_token_usage").filter(|x| !x.is_null()) {
                        Some(l) => tokens_of(Some(l)),
                        None => match prev_total {
                            Some(p) => diff(&total, &p),
                            None => total,
                        },
                    };
                    prev_total = Some(total);
                    if delta.total() == 0 || !in_window(when, since) {
                        continue;
                    }
                    count += 1;
                    out.requests.push(Request {
                        model: if model.is_empty() {
                            "unknown".into()
                        } else {
                            model.clone()
                        },
                        when,
                        tokens: Tokens {
                            requests: 1,
                            ..delta
                        },
                        billing: Billing::default(),
                        project: cwd.clone(),
                        host_cost_usd: None,
                        provider_hint: Some("openai".into()),
                        credit_hint: if plan.is_some() || chatgpt_account {
                            Some(Credit::Subscription { plan: plan.clone() })
                        } else {
                            None
                        },
                        source: f.clone(),
                    });
                }
                _ => {}
            }
        }
        // the plan learned later in the file applies to the whole session
        if let Some(p) = &plan {
            for r in out.requests.iter_mut().filter(|r| &r.source == f) {
                r.credit_hint = Some(Credit::Subscription {
                    plan: Some(p.clone()),
                });
            }
        }
        if let Some((t, qs)) = file_quota {
            if newest_quota.as_ref().is_none_or(|(nt, _)| t > *nt) {
                newest_quota = Some((t, qs));
            }
        }
        out.files.push((f.clone(), count));
    }
    if let Some((_, qs)) = newest_quota {
        out.quotas = qs;
    }
    out
}

fn tokens_of(v: Option<&Value>) -> Tokens {
    let Some(v) = v else { return Tokens::default() };
    let cached = u64_at(v, "cached_input_tokens");
    let input = u64_at(v, "input_tokens");
    Tokens {
        // Codex's input_tokens includes the cached part
        input: input.saturating_sub(cached),
        output: u64_at(v, "output_tokens"),
        cache_write: u64_at(v, "cache_write_input_tokens"),
        cache_read: cached,
        reasoning: u64_at(v, "reasoning_output_tokens"),
        requests: 0,
    }
}

fn diff(a: &Tokens, b: &Tokens) -> Tokens {
    Tokens {
        input: a.input.saturating_sub(b.input),
        output: a.output.saturating_sub(b.output),
        cache_write: a.cache_write.saturating_sub(b.cache_write),
        cache_read: a.cache_read.saturating_sub(b.cache_read),
        reasoning: a.reasoning.saturating_sub(b.reasoning),
        requests: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deltas_model_and_quota() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("rollout-2026-10-17T10-00-00-abc.jsonl");
        let text = [
            r#"{"timestamp":"2026-10-17T10:00:00Z","type":"session_meta","payload":{"id":"abc","cwd":"/home/me/p","creator_account_id":"acc"}}"#,
            r#"{"timestamp":"2026-10-17T10:00:01Z","type":"turn_context","payload":{"model":"gpt-5.3-codex","cwd":"/home/me/p"}}"#,
            r#"{"timestamp":"2026-10-17T10:00:05Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":400,"cache_write_input_tokens":0,"output_tokens":50,"reasoning_output_tokens":10,"total_tokens":1050},"last_token_usage":{"input_tokens":1000,"cached_input_tokens":400,"cache_write_input_tokens":0,"output_tokens":50,"reasoning_output_tokens":10,"total_tokens":1050}},"rate_limits":{"primary":{"used_percent":85.5,"window_minutes":300,"resets_at":1760700000},"secondary":{"used_percent":12.0,"window_minutes":10080,"resets_at":1761000000},"plan_type":"plus"}}}"#,
            r#"{"timestamp":"2026-10-17T10:01:05Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":3000,"cached_input_tokens":2000,"cache_write_input_tokens":0,"output_tokens":80,"reasoning_output_tokens":10,"total_tokens":3080},"last_token_usage":null},"rate_limits":null}}"#,
        ].join("\n");
        std::fs::write(&f, text).unwrap();
        let r = read(
            "codex",
            "Codex CLI",
            &[f],
            "2026-10-01T00:00:00Z".parse().unwrap(),
        );
        assert_eq!(r.requests.len(), 2);
        assert_eq!(r.requests[0].tokens.input, 600);
        assert_eq!(r.requests[0].tokens.cache_read, 400);
        assert_eq!(r.requests[1].tokens.input, 400);
        assert_eq!(r.requests[1].tokens.cache_read, 1600);
        assert_eq!(r.requests[1].tokens.output, 30);
        assert_eq!(r.requests[0].model, "gpt-5.3-codex");
        assert_eq!(
            r.requests[1].credit_hint,
            Some(Credit::Subscription {
                plan: Some("plus".into())
            })
        );
        assert_eq!(r.quotas.len(), 2);
        assert_eq!(r.quotas[0].used_percent, 85.5);
        assert_eq!(r.quotas[0].plan.as_deref(), Some("plus"));
    }
}
