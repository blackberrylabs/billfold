//! The price table: a snapshot of API Price Index (`prices/apipriceindex.json`, CC BY 4.0,
//! attribution "API Price Index (apipriceindex.com)") embedded at build time, never
//! fetched at run time, and the provider sheets' rules for what the index does not carry
//! (cache writes, long-context tiers, peak hours, announced changes). A model the table
//! does not know is `None`: tokens are counted, dollars are not invented.

use std::collections::HashMap;
use std::sync::OnceLock;

use chrono::{DateTime, Datelike, Timelike, Utc};
use regex::Regex;
use serde::Deserialize;

use crate::model::{Billing, PriceInfo, Tokens};
use crate::providers::{Provider, Providers};

pub const EMBEDDED: &str = include_str!("../../../prices/apipriceindex.json");

#[derive(Debug, Clone, Deserialize)]
struct Index {
    schema_version: u32,
    license: String,
    attribution: String,
    as_of: String,
    models: Vec<Entry>,
}

#[derive(Debug, Clone, Deserialize)]
struct Entry {
    id: String,
    provider: ProviderRef,
    base_model: Option<String>,
    api: Option<Api>,
    pricing: Option<Pricing>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProviderRef {
    id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Api {
    model: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Pricing {
    input_per_mtok: f64,
    output_per_mtok: f64,
    cached_input_per_mtok: Option<f64>,
    verified_at: Option<String>,
    source_url: Option<String>,
    #[serde(default)]
    tier: Option<String>,
    discount: Option<f64>,
}

/// One priced model, as the table knows it.
#[derive(Debug, Clone)]
pub struct ModelPrice {
    /// The index id (`anthropic:sonnet-5-5`).
    pub id: String,
    pub provider_id: String,
    /// The provider's API id when the index knows it, and the index's base model.
    pub api_model: Option<String>,
    pub base_model: Option<String>,
    pub input: f64,
    pub output: f64,
    pub cached_input: Option<f64>,
    /// `standard`, `off_peak`, `peak`, `le_200k`…
    pub tier: String,
    pub discount: Option<f64>,
    pub verified_at: Option<String>,
    pub source_url: Option<String>,
}

pub struct PriceTable {
    info: PriceInfo,
    entries: Vec<ModelPrice>,
    /// exact api id or alias → entry indexes (several providers may serve one model)
    by_api: HashMap<String, Vec<usize>>,
    /// base_model → entry indexes
    by_base: HashMap<String, Vec<usize>>,
}

impl PriceTable {
    pub fn parse(text: &str) -> Result<Self, String> {
        let idx: Index = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if idx.schema_version != 1 {
            return Err(format!(
                "price table schema {} (expected 1)",
                idx.schema_version
            ));
        }
        let mut entries = Vec::new();
        let mut by_api: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_base: HashMap<String, Vec<usize>> = HashMap::new();
        for e in &idx.models {
            let Some(p) = &e.pricing else { continue };
            let i = entries.len();
            entries.push(ModelPrice {
                id: e.id.clone(),
                provider_id: e.provider.id.clone(),
                api_model: e.api.as_ref().and_then(|a| a.model.clone()),
                base_model: e.base_model.clone(),
                input: p.input_per_mtok,
                output: p.output_per_mtok,
                cached_input: p.cached_input_per_mtok,
                tier: p.tier.clone().unwrap_or_else(|| "standard".into()),
                discount: p.discount,
                verified_at: p.verified_at.clone(),
                source_url: p.source_url.clone(),
            });
            if let Some(api) = &e.api {
                if let Some(m) = &api.model {
                    by_api.entry(m.to_lowercase()).or_default().push(i);
                }
                for a in &api.aliases {
                    by_api.entry(a.to_lowercase()).or_default().push(i);
                }
            }
            if let Some(b) = &e.base_model {
                by_base.entry(b.to_lowercase()).or_default().push(i);
            }
            // the slug of the index id is a last-resort key (`gpt-4o`, `kimi-k3`)
            if let Some((_, slug)) = e.id.split_once(':') {
                by_base.entry(slug.to_lowercase()).or_default().push(i);
            }
        }
        Ok(PriceTable {
            info: PriceInfo {
                name: "API Price Index".into(),
                as_of: idx.as_of.clone(),
                license: idx.license.clone(),
                attribution: idx.attribution.clone(),
                models: entries.len(),
            },
            entries,
            by_api,
            by_base,
        })
    }

    /// The embedded snapshot, parsed once.
    pub fn embedded() -> &'static PriceTable {
        static T: OnceLock<PriceTable> = OnceLock::new();
        T.get_or_init(|| PriceTable::parse(EMBEDDED).expect("the embedded price table is valid"))
    }

    pub fn info(&self) -> &PriceInfo {
        &self.info
    }

    /// The price of a model as a host logged it. `provider` is the provider the host is
    /// wired to, when known: it picks the right row when several providers serve the
    /// model (`claude-sonnet-5-5` at Anthropic, Vertex and Bedrock).
    pub fn lookup(
        &self,
        model: &str,
        provider: Option<&Provider>,
        providers: &Providers,
    ) -> Option<ModelPrice> {
        // a model the provider serves for nothing, by its own sheet
        if let Some(p) = provider {
            if let Some(free) = &p.pricing.free {
                let bare = candidates(model, Some(p));
                if bare.iter().any(|c| free.matches(c)) {
                    return Some(ModelPrice {
                        id: format!("{}:{}", p.id, bare.get(1).unwrap_or(&bare[0])),
                        provider_id: p.id.clone(),
                        api_model: Some(model.to_string()),
                        base_model: None,
                        input: 0.0,
                        output: 0.0,
                        cached_input: Some(0.0),
                        tier: "free".into(),
                        discount: None,
                        verified_at: None,
                        source_url: free.source.as_ref().and_then(|s| s.url.clone()),
                    });
                }
            }
        }
        let wanted_ids: Vec<&str> = provider
            .map(|p| p.index_ids.iter().map(String::as_str).collect())
            .unwrap_or_default();
        let pick = |cands: &Vec<usize>| -> Option<&ModelPrice> {
            if cands.is_empty() {
                return None;
            }
            // the provider the host is wired to, else a first-party row, else the first
            cands
                .iter()
                .find(|&&i| wanted_ids.contains(&self.entries[i].provider_id.as_str()))
                .or_else(|| {
                    cands.iter().find(|&&i| {
                        let pid = &self.entries[i].provider_id;
                        providers.list.iter().any(|p| {
                            p.index_ids.iter().any(|x| x == pid)
                                && self.entries[i].id.starts_with(&format!("{pid}:"))
                                && first_party(pid)
                        })
                    })
                })
                .or_else(|| cands.first())
                .map(|&i| &self.entries[i])
        };
        for key in candidates(model, provider) {
            if let Some(c) = self.by_api.get(&key) {
                if let Some(p) = pick(c) {
                    return Some(p.clone());
                }
            }
            if let Some(c) = self.by_base.get(&key) {
                if let Some(p) = pick(c) {
                    return Some(p.clone());
                }
            }
        }
        None
    }
}

fn first_party(pid: &str) -> bool {
    matches!(
        pid,
        "anthropic"
            | "openai"
            | "google"
            | "deepseek"
            | "mistral"
            | "xai"
            | "moonshot"
            | "z-ai"
            | "minimax"
            | "alibaba"
            | "groq"
            | "cohere"
            | "perplexity"
            | "together"
            | "fireworks"
            | "cerebras"
    )
}

fn date_suffix() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:[-@]20[0-9]{6})(?:-v\d+:\d+)?$").unwrap())
}

/// The keys to try for a logged model id, most specific first: as written, with the
/// provider's prefixes removed, with an `author/` prefix removed, with a date suffix
/// removed, through the provider's aliases.
pub fn candidates(model: &str, provider: Option<&Provider>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: String| {
        let s = s.to_lowercase();
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    };
    let m = model.trim();
    push(m.to_string());
    let mut stripped = m.to_string();
    if let Some(p) = provider {
        for pre in &p.id_prefixes {
            if let Some(rest) = stripped.strip_prefix(pre.as_str()) {
                stripped = rest.to_string();
                break;
            }
        }
        for (from, to) in &p.aliases {
            if from.eq_ignore_ascii_case(&stripped) || from.eq_ignore_ascii_case(m) {
                push(to.clone());
            }
        }
    }
    push(stripped.clone());
    if let Some((_, rest)) = stripped.split_once('/') {
        push(rest.to_string());
        stripped = rest.to_string();
    }
    for suffix in [":batch", ":free", ":thinking", ":extended"] {
        if let Some(rest) = stripped.strip_suffix(suffix) {
            push(rest.to_string());
            stripped = rest.to_string();
        }
    }
    let undated = date_suffix().replace(&stripped, "").to_string();
    push(undated.clone());
    if let Some(p) = provider {
        for (from, to) in &p.aliases {
            if from.eq_ignore_ascii_case(&undated) {
                push(to.clone());
            }
        }
    }
    out
}

/// The result of pricing one request.
#[derive(Debug, Clone, PartialEq)]
pub struct Priced {
    pub usd: f64,
    /// What was applied beyond the four base prices.
    pub notes: Vec<String>,
}

/// Price one request. `when` is the request's time (UTC), for peak hours and announced
/// changes. `prompt` for the tiers is input + cache read + cache write.
pub fn price_request(
    tokens: &Tokens,
    billing: &Billing,
    price: &ModelPrice,
    provider: Option<&Provider>,
    when: Option<DateTime<Utc>>,
) -> Priced {
    let rules = provider.map(|p| &p.pricing);
    let mut input = price.input;
    let mut output = price.output;
    let mut cached = price.cached_input;
    let mut notes = Vec::new();
    let model_key = price
        .id
        .split_once(':')
        .map(|(_, s)| s)
        .unwrap_or(&price.id);
    if let Some(r) = rules {
        // an announced change that is already in force
        for a in &r.announced {
            if a.models.iter().any(|m| same_model(m, model_key, price)) {
                if let (Some(w), Some(from)) = (when, parse_toml_date(&a.from)) {
                    if w >= from {
                        input = a.input;
                        output = a.output;
                        if a.cache_read.is_some() {
                            cached = a.cache_read;
                        }
                        notes.push(format!("price in force since {}", a.from));
                    }
                }
            }
        }
        // long-context tier
        let prompt = tokens.input + tokens.cache_read + tokens.cache_write;
        for t in &r.long_context {
            if t.models.iter().any(|m| same_model(m, model_key, price)) && prompt > t.above {
                input = t.input;
                output = t.output;
                if t.cache_read.is_some() {
                    cached = t.cache_read;
                }
                notes.push(format!("long-context tier (> {} prompt tokens)", t.above));
            }
        }
    }
    let cached = cached.unwrap_or(input);
    let w5 = rules.and_then(|r| r.cache_write_5m).unwrap_or(1.0);
    let w1h = rules.and_then(|r| r.cache_write_1h).unwrap_or(w5);
    let write_1h = billing.cache_write_1h.min(tokens.cache_write);
    let write_5m = tokens.cache_write - write_1h;
    let mut usd = (tokens.input as f64 * input
        + tokens.output as f64 * output
        + tokens.cache_read as f64 * cached
        + write_5m as f64 * input * w5
        + write_1h as f64 * input * w1h)
        / 1_000_000.0;
    if tokens.cache_write > 0 && rules.and_then(|r| r.cache_write_5m).is_none() {
        notes.push("cache writes priced as input (no documented cache-write price)".into());
    }
    // peak hours when the index stores the off-peak rate
    if price.tier == "off_peak" {
        if let (Some(r), Some(w)) = (rules.and_then(|r| r.off_peak.as_ref()), when) {
            if in_windows(&r.windows, w) {
                usd *= r.peak_multiplier;
                notes.push(format!("peak hours ×{}", r.peak_multiplier));
            }
        } else {
            notes.push("off-peak rate (time of request unknown)".into());
        }
    }
    if let Some(d) = price.discount {
        if d > 0.0 {
            notes.push(format!("promotional price (−{:.0} %)", d * 100.0));
        }
    }
    if price.tier == "free" {
        notes.push("free model".into());
    }
    Priced { usd, notes }
}

fn same_model(rule_model: &str, index_slug: &str, price: &ModelPrice) -> bool {
    let r = rule_model.to_lowercase();
    r == index_slug.to_lowercase()
        || price.id.to_lowercase().ends_with(&format!(":{r}"))
        || price.api_model.as_deref().is_some_and(|m| {
            m.to_lowercase() == r || m.to_lowercase().starts_with(&format!("{r}-"))
        })
        || price
            .base_model
            .as_deref()
            .is_some_and(|m| m.to_lowercase() == r)
}

fn parse_toml_date(d: &toml::value::Datetime) -> Option<DateTime<Utc>> {
    let s = d.to_string();
    let date = s.get(..10)?;
    let naive = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    Some(DateTime::<Utc>::from_naive_utc_and_offset(
        naive.and_hms_opt(0, 0, 0)?,
        Utc,
    ))
}

fn in_windows(windows: &[crate::providers::OffPeakWindow], when: DateTime<Utc>) -> bool {
    let weekday = when.weekday().num_days_from_monday(); // 0 = Monday
    let minute = when.hour() * 60 + when.minute();
    windows.iter().any(|w| {
        let days_ok = match w.days.as_str() {
            "mon-fri" => weekday < 5,
            _ => true,
        };
        let parse = |s: &str| -> Option<u32> {
            let (h, m) = s.split_once(':')?;
            Some(h.parse::<u32>().ok()? * 60 + m.parse::<u32>().ok()?)
        };
        match (parse(&w.start), parse(&w.end)) {
            (Some(a), Some(b)) => days_ok && minute >= a && minute < b,
            _ => false,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers;

    #[test]
    fn the_snapshot_loads_with_its_attribution() {
        let t = PriceTable::embedded();
        assert_eq!(t.info().license, "CC BY 4.0");
        assert!(t.info().attribution.contains("API Price Index"));
        assert!(t.info().models > 700);
    }

    #[test]
    fn logged_ids_find_their_price() {
        let t = PriceTable::embedded();
        let ps = providers::embedded();
        let anthropic = ps.get("anthropic");
        let p = t.lookup("claude-sonnet-5-5", anthropic, &ps).unwrap();
        assert_eq!(p.id, "anthropic:sonnet-5-5");
        assert_eq!(p.input, 2.0);
        // a dated id, a Bedrock id, a Vertex id all land on the same row
        assert_eq!(
            t.lookup("claude-sonnet-4-5-20250929", anthropic, &ps)
                .unwrap()
                .id,
            "anthropic:sonnet-4-5"
        );
        assert_eq!(
            t.lookup("us.anthropic.claude-haiku-5-5", anthropic, &ps)
                .unwrap()
                .id,
            "anthropic:haiku-5-5"
        );
        assert_eq!(
            t.lookup("claude-opus-4-1", anthropic, &ps).unwrap().id,
            "anthropic:opus-4-1"
        );
        // DeepSeek's API names are aliases in the sheet
        assert_eq!(
            t.lookup("deepseek-flash", ps.get("deepseek"), &ps)
                .unwrap()
                .id,
            "deepseek:deepseek-v4.1-flash"
        );
        assert_eq!(
            t.lookup("gpt-5.3-codex", ps.get("openai"), &ps)
                .unwrap()
                .provider_id,
            "openai"
        );
        assert!(t.lookup("made-up-model-9000", None, &ps).is_none());
        // OpenCode Zen: a `-free` model is priced 0 by the sheet, a paid one at the
        // first-party price
        let zen = ps.get("opencode");
        let free = t.lookup("opencode/space-bunny-free", zen, &ps).unwrap();
        assert_eq!(
            (free.tier.as_str(), free.input, free.output),
            ("free", 0.0, 0.0)
        );
        assert_eq!(free.provider_id, "opencode");
        let paid = t.lookup("claude-sonnet-5-5", zen, &ps).unwrap();
        assert_eq!(paid.provider_id, "anthropic");
        assert!(t.lookup("space-bunny-free", None, &ps).is_none());
    }

    #[test]
    fn a_request_is_priced_with_cache_rules_and_tiers() {
        let t = PriceTable::embedded();
        let ps = providers::embedded();
        let anthropic = ps.get("anthropic");
        let p = t.lookup("claude-sonnet-5-5", anthropic, &ps).unwrap();
        let tokens = Tokens {
            input: 1_000_000,
            output: 100_000,
            cache_write: 1_000_000,
            cache_read: 1_000_000,
            reasoning: 0,
            requests: 1,
        };
        let billing = Billing {
            cache_write_1h: 0,
            tier: None,
            geo: None,
        };
        let r = price_request(&tokens, &billing, &p, anthropic, None);
        // 2 + 1 + 0.1 + 2 × 1.25
        assert!(
            (r.usd - 5.6).abs() < 1e-9,
            "{} {:?} {:?}",
            r.usd,
            r.notes,
            p
        );
        let billing_1h = Billing {
            cache_write_1h: 1_000_000,
            tier: None,
            geo: None,
        };
        let r = price_request(&tokens, &billing_1h, &p, anthropic, None);
        assert!((r.usd - 7.1).abs() < 1e-9, "{}", r.usd); // 2 + 1 + 0.1 + 2 × 2
                                                          // Haiku 5.5 above 100K prompt tokens pays the higher tier
        let h = t.lookup("claude-haiku-5-5", anthropic, &ps).unwrap();
        let big = Tokens {
            input: 150_000,
            output: 1_000,
            cache_write: 0,
            cache_read: 0,
            reasoning: 0,
            requests: 1,
        };
        let r = price_request(&big, &billing, &h, anthropic, None);
        assert!(r.notes.iter().any(|n| n.contains("long-context")));
        assert!((r.usd - (150_000.0 * 0.5 + 1_000.0 * 2.5) / 1e6).abs() < 1e-9);
        // DeepSeek at peak hours is doubled
        let d = t
            .lookup("deepseek-v4-pro", ps.get("deepseek"), &ps)
            .unwrap();
        assert_eq!(d.tier, "off_peak");
        let small = Tokens {
            input: 1_000_000,
            output: 0,
            cache_write: 0,
            cache_read: 0,
            reasoning: 0,
            requests: 1,
        };
        let peak = "2026-10-14T07:30:00Z".parse::<DateTime<Utc>>().unwrap(); // Wednesday
        let off = "2026-10-14T20:30:00Z".parse::<DateTime<Utc>>().unwrap();
        let rp = price_request(&small, &billing, &d, ps.get("deepseek"), Some(peak));
        let ro = price_request(&small, &billing, &d, ps.get("deepseek"), Some(off));
        assert!((rp.usd - 2.0 * ro.usd).abs() < 1e-9);
    }
}
