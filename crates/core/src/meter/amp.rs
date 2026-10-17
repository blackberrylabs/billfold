//! Amp: `~/.local/share/amp/threads/T-*.json` — `messages[].usage` with the model, the time
//! and Anthropic-style token fields (documented by ccusage only).

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{in_window, read_text, str_at, time_at, u64_at, Read, Request};
use crate::model::{Billing, Credit, Tokens};

pub fn read(files: &[PathBuf], since: DateTime<Utc>) -> Read {
    let mut out = Read::default();
    for f in files {
        let Ok(text) = read_text(f) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let mut n = 0u64;
        if let Some(Value::Array(msgs)) = v.get("messages") {
            for m in msgs {
                let Some(u) = m.get("usage").filter(|x| x.is_object()) else {
                    continue;
                };
                let when = time_at(u, "timestamp", "ms").or_else(|| time_at(m, "timestamp", "ms"));
                if !in_window(when, since) {
                    continue;
                }
                n += 1;
                out.requests.push(Request {
                    model: str_at(u, "model")
                        .or_else(|| str_at(m, "model"))
                        .unwrap_or_else(|| "unknown".into()),
                    when,
                    tokens: Tokens {
                        input: u64_at(u, "inputTokens"),
                        output: u64_at(u, "outputTokens"),
                        cache_write: u64_at(u, "cacheCreationInputTokens"),
                        cache_read: u64_at(u, "cacheReadInputTokens"),
                        reasoning: 0,
                        requests: 1,
                    },
                    billing: Billing::default(),
                    project: None,
                    host_cost_usd: None,
                    provider_hint: None,
                    credit_hint: Some(Credit::Subscription { plan: None }),
                    source: f.clone(),
                });
            }
        }
        out.files.push((f.clone(), n));
    }
    out
}
