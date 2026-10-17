//! Aider: `.aider.chat.history.md` at the git root — blockquote lines
//! `> Tokens: 12k sent, 1.2k received. Cost: $0.02 message, $0.05 session.` (counts are
//! rounded by Aider itself), the model from `Model: <name> with …`, the session's start
//! from `# aider chat started at …`.

use std::path::PathBuf;
use std::sync::OnceLock;

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use regex::Regex;

use super::{in_window, read_text, Read, Request};
use crate::model::{Billing, Tokens};

fn re_tokens() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"Tokens:\s*([\d.,]+k?)\s*sent(?:,\s*([\d.,]+k?)\s*cache write)?(?:,\s*([\d.,]+k?)\s*cache hit)?,\s*([\d.,]+k?)\s*received\.(?:\s*Cost:\s*\$([\d.]+)\s*(?:message|request))?").unwrap()
    })
}

fn re_model() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"Model:\s*(\S+)\s+with").unwrap())
}

fn re_started() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"# aider chat started at (\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})").unwrap()
    })
}

/// `5.2k` → 5200, `12k` → 12000, `604` → 604, `5,244` → 5244.
fn count(s: &str) -> u64 {
    let s = s.replace(',', "");
    if let Some(k) = s.strip_suffix('k') {
        (k.parse::<f64>().unwrap_or(0.0) * 1000.0).round() as u64
    } else {
        s.parse::<f64>().unwrap_or(0.0).round() as u64
    }
}

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
        let mut model = String::from("unknown");
        let mut when: Option<DateTime<Utc>> = None;
        let mut n = 0u64;
        for line in text.lines() {
            if let Some(c) = re_started().captures(line) {
                if let Ok(nd) = NaiveDateTime::parse_from_str(&c[1], "%Y-%m-%d %H:%M:%S") {
                    // Aider writes local time
                    when = Local
                        .from_local_datetime(&nd)
                        .single()
                        .map(|t| t.with_timezone(&Utc));
                }
                continue;
            }
            if let Some(c) = re_model().captures(line) {
                model = c[1].to_string();
                continue;
            }
            let Some(c) = re_tokens().captures(line) else {
                continue;
            };
            if !in_window(when, since) {
                continue;
            }
            let sent = count(&c[1]);
            let cache_write = c.get(2).map(|m| count(m.as_str())).unwrap_or(0);
            let cache_read = c.get(3).map(|m| count(m.as_str())).unwrap_or(0);
            n += 1;
            out.requests.push(Request {
                model: model.clone(),
                when,
                tokens: Tokens {
                    input: sent.saturating_sub(cache_write + cache_read),
                    output: count(&c[4]),
                    cache_write,
                    cache_read,
                    reasoning: 0,
                    requests: 1,
                },
                billing: Billing::default(),
                project: f.parent().map(|p| p.to_path_buf()),
                host_cost_usd: c.get(5).and_then(|m| m.as_str().parse::<f64>().ok()),
                provider_hint: provider_of(&model),
                credit_hint: Some(crate::model::Credit::ApiKey),
                source: f.clone(),
            });
        }
        out.files.push((f.clone(), n));
    }
    out
}

/// Aider names models `provider/model` or by a bare name LiteLLM resolves.
fn provider_of(model: &str) -> Option<String> {
    let m = model.to_lowercase();
    if let Some((p, _)) = m.split_once('/') {
        return Some(crate::wiring::normalise_provider(p));
    }
    if m.starts_with("claude") {
        Some("anthropic".into())
    } else if m.starts_with("gpt")
        || m.starts_with("o1")
        || m.starts_with("o3")
        || m.starts_with("o4")
    {
        Some("openai".into())
    } else if m.starts_with("gemini") {
        Some("google".into())
    } else if m.starts_with("deepseek") {
        Some("deepseek".into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_counts_and_costs() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join(".aider.chat.history.md");
        std::fs::write(&f, "\n# aider chat started at 2026-10-17 10:00:00\n\n> Model: anthropic/claude-sonnet-5-5 with diff edit format\n\n#### hi\n\n> Tokens: 5.2k sent, 1.0k cache write, 2.1k cache hit, 604 received. Cost: $0.02 message, $0.02 session.\n> Tokens: 5,244 sent, 604 received. Cost: $0.03 request, $0.05 session.\n").unwrap();
        let r = read(&[f], "2026-10-01T00:00:00Z".parse().unwrap());
        assert_eq!(r.requests.len(), 2);
        assert_eq!(r.requests[0].tokens.input, 2100);
        assert_eq!(r.requests[0].tokens.cache_read, 2100);
        assert_eq!(r.requests[0].tokens.output, 604);
        assert_eq!(r.requests[0].host_cost_usd, Some(0.02));
        assert_eq!(r.requests[1].tokens.input, 5244);
        assert_eq!(r.requests[0].provider_hint.as_deref(), Some("anthropic"));
        assert_eq!(r.requests[0].model, "anthropic/claude-sonnet-5-5");
    }
}
