//! The provider sheets (`providers/*.toml`): for each AI provider, the variable names its
//! documentation gives, the key shapes scanners have observed, where its usage is shown,
//! and the price rules the embedded index does not carry (cache writes, long-context tiers,
//! off-peak hours, announced changes). Every claim names its source. Schema in
//! `docs/providers.md`.

use bbl_hostrules::rules::Source;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Shape {
    /// Anchored regex of a whole key.
    pub regex: String,
    pub source: Source,
}

/// Prices above a prompt size, per million tokens.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LongContext {
    pub models: Vec<String>,
    /// Prompt tokens (input + cache) above which the tier applies.
    pub above: u64,
    pub input: f64,
    pub output: f64,
    pub cache_read: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OffPeakWindow {
    /// `mon-fri` or `all`.
    pub days: String,
    /// `HH:MM` UTC.
    pub start: String,
    pub end: String,
}

/// The index stores the off-peak rate for this provider; outside the windows, multiply.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OffPeak {
    pub windows: Vec<OffPeakWindow>,
    pub peak_multiplier: f64,
}

/// A price change already announced, with its effective date.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Announced {
    pub models: Vec<String>,
    pub from: toml::value::Datetime,
    pub input: f64,
    pub output: f64,
    pub cache_read: Option<f64>,
    pub quote: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Pricing {
    /// Cache write price as a multiple of the input price, 5-minute lifetime.
    pub cache_write_5m: Option<f64>,
    /// Same, 1-hour lifetime.
    pub cache_write_1h: Option<f64>,
    /// Reasoning tokens are billed as output (already inside `output` in the logs).
    #[serde(default)]
    pub reasoning_as_output: bool,
    #[serde(default)]
    pub long_context: Vec<LongContext>,
    pub off_peak: Option<OffPeak>,
    #[serde(default)]
    pub announced: Vec<Announced>,
    pub source: Option<Source>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    /// Documented environment variable names (`ANTHROPIC_API_KEY`), primary attribution.
    #[serde(default)]
    pub env_vars: Vec<String>,
    /// Names seen in tool configuration files (`anthropic-api-key`, `anthropicApiKey`),
    /// compared without case, dashes or underscores.
    #[serde(default)]
    pub config_names: Vec<String>,
    /// Observed key shapes, secondary attribution.
    #[serde(default)]
    pub shapes: Vec<Shape>,
    /// Where the provider shows usage and billing (an `open-url` action, never fetched).
    pub usage_url: Option<String>,
    /// Model-id prefixes to strip before matching the price table (`anthropic.`, `us.`).
    #[serde(default)]
    pub id_prefixes: Vec<String>,
    /// Aliases the logs may carry → the id the price table knows.
    #[serde(default)]
    pub aliases: Vec<(String, String)>,
    /// Which provider ids of the price table this provider's models are priced under
    /// (the index's provider ids: `anthropic`, `openai`, `google`, `z-ai`…).
    #[serde(default)]
    pub index_ids: Vec<String>,
    #[serde(default)]
    pub pricing: Pricing,
    pub source: Source,
}

#[derive(Debug, Clone)]
pub struct Providers {
    pub list: Vec<Provider>,
    compiled: Vec<(usize, Regex)>,
}

impl Providers {
    pub fn load<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Result<Self, String> {
        let mut list = Vec::new();
        for (file, text) in files {
            let p: Provider = toml::from_str(text).map_err(|e| format!("{file}: {e}"))?;
            if p.source.quote.is_empty() {
                return Err(format!("{file}: a provider sheet needs a sourced quote"));
            }
            list.push(p);
        }
        list.sort_by_key(|a| a.name.to_lowercase());
        let mut compiled = Vec::new();
        for (i, p) in list.iter().enumerate() {
            for s in &p.shapes {
                let re = Regex::new(&s.regex).map_err(|e| format!("{}: {e}", p.id))?;
                compiled.push((i, re));
            }
        }
        Ok(Providers { list, compiled })
    }

    pub fn get(&self, id: &str) -> Option<&Provider> {
        self.list.iter().find(|p| p.id == id)
    }

    pub fn name_of<'a>(&'a self, id: &'a str) -> &'a str {
        self.get(id).map(|p| p.name.as_str()).unwrap_or(id)
    }

    /// The provider a variable or config key name belongs to, by its documented names.
    pub fn by_name(&self, name: &str) -> Option<&Provider> {
        let n = fold(name);
        // exact env var first, then config names, then a provider id inside the name
        self.list
            .iter()
            .find(|p| p.env_vars.iter().any(|v| fold(v) == n))
            .or_else(|| {
                self.list
                    .iter()
                    .find(|p| p.config_names.iter().any(|v| fold(v) == n))
            })
            .or_else(|| {
                let looks_like_key = n.contains("apikey")
                    || n.contains("token")
                    || n.contains("secret")
                    || n.ends_with("key");
                if !looks_like_key {
                    return None;
                }
                self.list.iter().find(|p| {
                    n.contains(&fold(&p.id)) || p.config_names.iter().any(|c| n.contains(&fold(c)))
                })
            })
    }

    /// The provider a key value belongs to, by its observed shape.
    pub fn by_shape(&self, value: &str) -> Option<&Provider> {
        self.compiled
            .iter()
            .find(|(_, re)| re.is_match(value))
            .map(|(i, _)| &self.list[*i])
    }

    /// Does the value look like a key of this provider (any shape), or, when the provider
    /// documents no shape, like a credential at all?
    pub fn plausible(&self, p: &Provider, value: &str) -> bool {
        if p.shapes.is_empty() {
            bbl_hostrules::secrets::value_looks_secret(value) || value.len() >= 20
        } else {
            self.compiled
                .iter()
                .any(|(i, re)| self.list[*i].id == p.id && re.is_match(value))
        }
    }
}

/// Lower case, without dashes, underscores or dots.
pub fn fold(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '-' | '_' | '.' | ' '))
        .flat_map(char::to_lowercase)
        .collect()
}

pub const EMBEDDED: &[(&str, &str)] = &[
    (
        "anthropic.toml",
        include_str!("../../../providers/anthropic.toml"),
    ),
    (
        "openai.toml",
        include_str!("../../../providers/openai.toml"),
    ),
    (
        "google.toml",
        include_str!("../../../providers/google.toml"),
    ),
    (
        "deepseek.toml",
        include_str!("../../../providers/deepseek.toml"),
    ),
    (
        "mistral.toml",
        include_str!("../../../providers/mistral.toml"),
    ),
    ("xai.toml", include_str!("../../../providers/xai.toml")),
    ("groq.toml", include_str!("../../../providers/groq.toml")),
    (
        "openrouter.toml",
        include_str!("../../../providers/openrouter.toml"),
    ),
    (
        "together.toml",
        include_str!("../../../providers/together.toml"),
    ),
    (
        "fireworks.toml",
        include_str!("../../../providers/fireworks.toml"),
    ),
    (
        "perplexity.toml",
        include_str!("../../../providers/perplexity.toml"),
    ),
    (
        "cohere.toml",
        include_str!("../../../providers/cohere.toml"),
    ),
    (
        "huggingface.toml",
        include_str!("../../../providers/huggingface.toml"),
    ),
    (
        "cerebras.toml",
        include_str!("../../../providers/cerebras.toml"),
    ),
    (
        "moonshot.toml",
        include_str!("../../../providers/moonshot.toml"),
    ),
    ("z-ai.toml", include_str!("../../../providers/z-ai.toml")),
    (
        "alibaba.toml",
        include_str!("../../../providers/alibaba.toml"),
    ),
    (
        "minimax.toml",
        include_str!("../../../providers/minimax.toml"),
    ),
    (
        "github.toml",
        include_str!("../../../providers/github.toml"),
    ),
    ("aws.toml", include_str!("../../../providers/aws.toml")),
    ("azure.toml", include_str!("../../../providers/azure.toml")),
];

pub fn embedded() -> Providers {
    Providers::load(EMBEDDED.iter().copied()).expect("embedded provider sheets are valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_shapes_attribute_keys() {
        let p = embedded();
        assert_eq!(p.by_name("ANTHROPIC_API_KEY").unwrap().id, "anthropic");
        assert_eq!(p.by_name("openai-api-key").unwrap().id, "openai");
        assert_eq!(p.by_name("GOOGLE_API_KEY").unwrap().id, "google");
        assert_eq!(p.by_name("deepseekApiKey").unwrap().id, "deepseek");
        assert!(p.by_name("EDITOR").is_none());
        let k = format!("sk-ant-api03-{}AA", "R".repeat(93));
        assert_eq!(p.by_shape(&k).unwrap().id, "anthropic");
        let o = format!("sk-proj-{}T3BlbkFJ{}", "a".repeat(74), "b".repeat(74));
        assert_eq!(p.by_shape(&o).unwrap().id, "openai");
        assert_eq!(
            p.by_shape("gsk_0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOP")
                .unwrap()
                .id,
            "groq"
        );
        assert!(p.by_shape("hello").is_none());
    }
}
