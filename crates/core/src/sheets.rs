//! Billfold's host sheets (`hosts/*.toml`): the common schema comes from `bbl-hostrules`;
//! this module adds what an inventory of accounts, keys and usage needs — per location,
//! what the file is (a credential store, a settings file, a usage log), which of its fields
//! are safe to show and which are secrets, how to read the log; per host, how it is
//! detected, where it keeps secrets, where its usage is shown. Schema in `docs/hosts.md`.
//! Every claim names its source; the loader refuses a location without one.

use bbl_hostrules::rules::{Location, Source};
use bbl_hostrules::{Extension, HostExtension, HostRule, Rules};
use serde::{Deserialize, Serialize};

/// What the file is to Billfold (`kind` in the sheet — `role` is a common field).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocKind {
    /// Where the host keeps its login: tokens, API key, account.
    Credential,
    /// A settings file that names the provider, the model, sometimes keys.
    Settings,
    /// A per-request token log.
    Usage,
}

/// How the host pays through this credential, when the file alone says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CreditHint {
    /// An OAuth login on a plan.
    Subscription,
    /// An API key.
    ApiKey,
    /// Depends on a field (`auth_mode`, `type`).
    Mixed,
    /// The file is a fallback; the default store is the system keyring.
    KeyringFallback,
}

/// Key paths (bbl-hostrules `read::get` syntax) of the fields safe to show, by meaning.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Fields {
    /// Plan or subscription type (`claudeAiOauth.subscriptionType`).
    pub plan: Option<String>,
    /// An e-mail address: only its domain is kept.
    pub email: Option<String>,
    pub organization: Option<String>,
    /// Expiry: RFC 3339, or epoch seconds / milliseconds (`expires_unit`).
    pub expires: Option<String>,
    #[serde(default)]
    pub expires_unit: String,
    /// `auth_mode` / `type`: a string telling subscription from key (`mode_subscription`,
    /// `mode_api_key` are the values that mean each).
    pub mode: Option<String>,
    #[serde(default)]
    pub mode_subscription: Vec<String>,
    #[serde(default)]
    pub mode_api_key: Vec<String>,
    /// A JWT whose payload (decoded locally, never shown) carries claims; `jwt_claims`
    /// maps meanings to claim paths.
    pub jwt: Option<String>,
    #[serde(default)]
    pub jwt_claims: JwtClaims,
    /// A key path whose presence means an API key is stored (`OPENAI_API_KEY`).
    pub api_key: Option<String>,
    /// For a settings file: the default provider and model.
    pub provider: Option<String>,
    pub model: Option<String>,
    /// For a table of providers (OpenCode `auth.json`, Kilo): each child key is a provider id.
    pub providers_table: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct JwtClaims {
    pub plan: Option<String>,
    pub email: Option<String>,
    pub account: Option<String>,
    pub expires: Option<String>,
}

/// How to read a usage log.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UsageSpec {
    /// The reader: `claude-jsonl`, `codex-jsonl`, `gemini-jsonl`, `qwen-jsonl`,
    /// `opencode-sqlite`, `goose-sqlite`, `aider-markdown`, `cline-tasks`, `cline-sessions`,
    /// `kilo-sqlite`, `copilot-jsonl`, `zed-sqlite`, `amp-json`.
    pub reader: String,
    /// Default retention in days, when the host prunes.
    pub retention_days: Option<i64>,
    /// The setting that changes it (`cleanupPeriodDays`), and the file it lives in.
    pub retention_setting: Option<String>,
    pub retention_file: Option<String>,
    /// The host writes its own cost per request.
    #[serde(default)]
    pub cost_by_host: bool,
    /// The host writes its remaining quota.
    #[serde(default)]
    pub quota: bool,
}

/// The fields Billfold adds to every location.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BillLocation {
    #[serde(rename = "kind")]
    pub what: LocKind,
    pub credit: Option<CreditHint>,
    /// The provider this credential is for (`anthropic`, `openai`, `github`…); for a table
    /// of providers, read from the table instead.
    pub provider: Option<String>,
    #[serde(default)]
    pub fields: Fields,
    /// Key paths that are secrets: never shown, their presence may be stated.
    #[serde(default)]
    pub secrets: Vec<String>,
    /// Key paths whose children are provider keys in clear (Qwen `env`, Cline
    /// `secrets.json` root as `""`), scanned by `keys`.
    #[serde(default)]
    pub keys_in: Vec<String>,
    pub usage: Option<UsageSpec>,
    /// For a `sqlite` credential store that is a key/value table (VS Code's `ItemTable`):
    /// the rows become an object the `fields` paths are read from.
    pub sqlite_kv: Option<SqliteKv>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SqliteKv {
    pub table: String,
    pub key_column: String,
    pub value_column: String,
}

impl Extension for BillLocation {
    fn validate(loc: &Location<Self>) -> Result<(), String> {
        match loc.ext.what {
            LocKind::Usage if loc.ext.usage.is_none() => {
                Err("a usage location needs a `usage` table with its reader".into())
            }
            LocKind::Usage
                if !matches!(
                    loc.ext.usage.as_ref().map(|u| u.reader.as_str()),
                    Some(
                        "claude-jsonl"
                            | "codex-jsonl"
                            | "gemini-jsonl"
                            | "qwen-jsonl"
                            | "opencode-sqlite"
                            | "goose-sqlite"
                            | "aider-markdown"
                            | "cline-tasks"
                            | "cline-sessions"
                            | "kilo-sqlite"
                            | "copilot-jsonl"
                            | "zed-sqlite"
                            | "amp-json"
                    )
                ) =>
            {
                Err(format!(
                    "unknown usage reader `{}`",
                    loc.ext
                        .usage
                        .as_ref()
                        .map(|u| u.reader.as_str())
                        .unwrap_or("")
                ))
            }
            LocKind::Credential if loc.ext.credit.is_none() => {
                Err("a credential location needs `credit`".into())
            }
            _ => Ok(()),
        }
    }
}

/// How a host is detected.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Detect {
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub binaries: Vec<String>,
}

/// Where the host keeps secrets when not in a file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Keyring {
    /// The Secret Service name or label the host uses (`goose`, `gemini-cli-oauth`).
    pub service: String,
    pub source: Source,
}

/// The fields Billfold adds to every host.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BillHost {
    #[serde(default)]
    pub detect: Detect,
    pub keyring: Option<Keyring>,
    /// The host's own page where usage is shown, when it has one (Cursor's dashboard).
    pub usage_url: Option<String>,
    /// Why no usage log is read, for hosts that keep none (with its source in `note`).
    pub no_usage_log: Option<String>,
    /// The host's own commands to check an account or usage (`claude` then `/status`).
    pub check: Option<String>,
}

impl HostExtension for BillHost {}

pub type Host = HostRule<BillLocation, BillHost>;
pub type Sheets = Rules<BillLocation, BillHost>;

/// The sheets shipped with Billfold, embedded at build time.
pub const EMBEDDED: &[(&str, &str)] = &[
    (
        "_not-read.toml",
        include_str!("../../../hosts/_not-read.toml"),
    ),
    ("aider.toml", include_str!("../../../hosts/aider.toml")),
    ("amp.toml", include_str!("../../../hosts/amp.toml")),
    (
        "claude-code.toml",
        include_str!("../../../hosts/claude-code.toml"),
    ),
    (
        "claude-desktop.toml",
        include_str!("../../../hosts/claude-desktop.toml"),
    ),
    ("cline.toml", include_str!("../../../hosts/cline.toml")),
    ("codex.toml", include_str!("../../../hosts/codex.toml")),
    ("copilot.toml", include_str!("../../../hosts/copilot.toml")),
    ("cursor.toml", include_str!("../../../hosts/cursor.toml")),
    ("devin.toml", include_str!("../../../hosts/devin.toml")),
    (
        "gemini-cli.toml",
        include_str!("../../../hosts/gemini-cli.toml"),
    ),
    ("goose.toml", include_str!("../../../hosts/goose.toml")),
    (
        "jetbrains-ai.toml",
        include_str!("../../../hosts/jetbrains-ai.toml"),
    ),
    (
        "kilo-code.toml",
        include_str!("../../../hosts/kilo-code.toml"),
    ),
    ("kiro.toml", include_str!("../../../hosts/kiro.toml")),
    (
        "opencode.toml",
        include_str!("../../../hosts/opencode.toml"),
    ),
    (
        "qwen-code.toml",
        include_str!("../../../hosts/qwen-code.toml"),
    ),
    (
        "roo-code.toml",
        include_str!("../../../hosts/roo-code.toml"),
    ),
    ("trae.toml", include_str!("../../../hosts/trae.toml")),
    ("warp.toml", include_str!("../../../hosts/warp.toml")),
    ("zed.toml", include_str!("../../../hosts/zed.toml")),
];

/// Load the embedded sheets. A sheet that does not load is a bug, not a runtime condition.
pub fn embedded() -> Sheets {
    Sheets::load(EMBEDDED.iter().copied()).expect("embedded host sheets are valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_sheet_loads_and_is_sourced() {
        let s = embedded();
        assert!(s.hosts.len() >= 20, "{} hosts", s.hosts.len());
        for h in &s.hosts {
            assert!(
                !h.locations.is_empty() || h.ext.no_usage_log.is_some(),
                "{}: nothing",
                h.id
            );
            for l in &h.locations {
                assert!(
                    !l.source.quote.is_empty(),
                    "{}: a location without a quote",
                    h.id
                );
            }
        }
        assert!(!s.not_read.is_empty());
    }
}
