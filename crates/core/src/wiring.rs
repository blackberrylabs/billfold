//! The wiring: which accounts each present host is logged into, read from its credential
//! and settings files as the sheets describe them. Only the account-descriptive fields are
//! kept (plan, e-mail domain, organization, expiry); every secret stays in the file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bbl_hostrules::env::Env;
use bbl_hostrules::read::{get, read_tree};
use bbl_hostrules::resolve::resolve;
use bbl_hostrules::rules::{Format, Location};
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::model::{Account, Credit};
use crate::sheets::{BillLocation, CreditHint, Host, LocKind};
use crate::util;

/// A loaded credential or settings file: its tree, with the file facts.
pub struct Loaded {
    pub path: PathBuf,
    pub tree: Value,
    pub mode: Option<String>,
    pub exposed: bool,
}

/// Load a location's file into a tree, whatever its format. SQLite goes through the
/// key/value description or, for OpenCode's `credential` table, a dedicated reader.
pub fn load(loc: &Location<BillLocation>, path: &Path) -> Result<Loaded, String> {
    let (mode, exposed) = util::mode_of(path);
    let tree = match loc.format {
        Format::Sqlite => {
            if let Some(kv) = &loc.ext.sqlite_kv {
                util::sqlite_kv(path, &kv.table, &kv.key_column, &kv.value_column)?
            } else if loc.ext.fields.providers_table.as_deref() == Some("credential") {
                opencode_credentials(path)?
            } else {
                return Err("sqlite location without a key/value description".into());
            }
        }
        f => read_tree(path, f)
            .map(|(_, v)| v)
            .map_err(|e| e.to_string())?,
    };
    Ok(Loaded {
        path: path.to_path_buf(),
        tree,
        mode,
        exposed,
    })
}

/// OpenCode's `credential` table as `{integration_id: {type, expires, active}}` — the
/// value's secrets are not copied out of the row.
fn opencode_credentials(path: &Path) -> Result<Value, String> {
    let conn = util::open_ro(path)?;
    let mut stmt = conn
        .prepare("SELECT integration_id, value, active FROM credential")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            let id: String = r.get(0)?;
            let value: String = r.get(1)?;
            let active: Option<i64> = r.get(2).ok();
            Ok((id, value, active))
        })
        .map_err(|e| e.to_string())?;
    let mut map = serde_json::Map::new();
    for (id, value, active) in rows.flatten() {
        let v: Value = serde_json::from_str(&value).unwrap_or(Value::Null);
        let mut o = serde_json::Map::new();
        if let Some(t) = v.get("type").and_then(Value::as_str) {
            o.insert("type".into(), Value::String(t.to_string()));
        }
        if let Some(e) = v.get("expires") {
            o.insert("expires".into(), e.clone());
        }
        o.insert("active".into(), Value::from(active.unwrap_or(1) != 0));
        map.insert(id, Value::Object(o));
    }
    Ok(Value::Object(map))
}

fn credit_from_type(t: &str) -> Credit {
    match t {
        "oauth" => Credit::Subscription { plan: None },
        "api" | "key" | "wellknown" => Credit::ApiKey,
        "external" => Credit::Unknown,
        _ => Credit::Unknown,
    }
}

fn str_at(tree: &Value, path: &str) -> Option<String> {
    let v = get(tree, path)?;
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn has(tree: &Value, path: &str) -> bool {
    matches!(get(tree, path), Some(v) if !v.is_null() && v.as_str().is_none_or(|s| !s.is_empty()))
}

/// The accounts of one host, from one loaded file.
pub fn accounts_from(
    host: &Host,
    loc: &Location<BillLocation>,
    file: &Loaded,
    now: DateTime<Utc>,
) -> Vec<Account> {
    let f = &loc.ext.fields;
    let base = |provider: String| Account {
        host: host.id.clone(),
        host_name: host.name.clone(),
        provider,
        credit: Credit::Unknown,
        email_domain: None,
        organization: None,
        expires_at: None,
        expired: false,
        path: file.path.clone(),
        mode: file.mode.clone(),
        modified: util::modified_of(&file.path),
        legacy: loc.legacy,
        note: loc.ext.note.clone(),
    };
    // a table of providers: one account per child
    if let Some(t) = &f.providers_table {
        // a SQLite table is already the object; a key path digs into a text tree
        let table = if t.is_empty() || loc.format == Format::Sqlite {
            Some(&file.tree)
        } else {
            get(&file.tree, t)
        };
        let Some(Value::Object(map)) = table else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (pid, v) in map {
            let mut a = base(normalise_provider(pid));
            if let Some(t) = v.get("type").and_then(Value::as_str) {
                a.credit = credit_from_type(t);
            } else if v.get("oauth_token").is_some()
                || v.get("users").is_some()
                || loc.ext.credit == Some(CreditHint::Subscription)
            {
                a.credit = Credit::Subscription { plan: None };
            }
            if let Some(e) = v
                .get("expires")
                .or_else(|| v.get("expiry"))
                .or_else(|| v.get("expires_at"))
            {
                if let Some(t) = util::parse_time(e, &f.expires_unit) {
                    a.expires_at = Some(util::rfc3339(t));
                    a.expired = t < now;
                }
            }
            if let Some(u) = v.get("user").and_then(Value::as_str) {
                a.organization = Some(u.to_string());
            }
            if v.get("active") == Some(&Value::Bool(false)) {
                a.note = Some("inactive".into());
            }
            out.push(a);
        }
        return out;
    }
    let provider = loc
        .ext
        .provider
        .clone()
        .or_else(|| {
            f.provider
                .as_ref()
                .and_then(|p| str_at(&file.tree, p))
                .map(|s| normalise_provider(&s))
        })
        .unwrap_or_else(|| "unknown".into());
    let mut a = base(provider);
    // the credit
    a.credit = match loc.ext.credit {
        Some(CreditHint::Subscription) => Credit::Subscription { plan: None },
        Some(CreditHint::ApiKey) => Credit::ApiKey,
        Some(CreditHint::KeyringFallback) | Some(CreditHint::Mixed) | None => Credit::Unknown,
    };
    if let Some(m) = &f.mode {
        if let Some(v) = str_at(&file.tree, m) {
            let v_l = v.to_lowercase();
            if f.mode_subscription.iter().any(|x| x.to_lowercase() == v_l) {
                a.credit = Credit::Subscription { plan: None };
            } else if f.mode_api_key.iter().any(|x| x.to_lowercase() == v_l) {
                a.credit = Credit::ApiKey;
            }
        }
    }
    if let Some(k) = &f.api_key {
        if has(&file.tree, k) && !matches!(a.credit, Credit::Subscription { .. }) {
            a.credit = Credit::ApiKey;
        }
    }
    if let Some(p) = &f.plan {
        // a plan name never overrides a mode that said "key" (an API organization has an
        // organizationType too)
        if let Some(v) = str_at(&file.tree, p) {
            if !matches!(a.credit, Credit::ApiKey) {
                a.credit = Credit::Subscription { plan: Some(v) };
            }
        }
    }
    if let Some(e) = &f.email {
        if let Some(v) = str_at(&file.tree, e) {
            a.email_domain = util::email_domain(&v);
        }
    }
    if let Some(o) = &f.organization {
        a.organization = str_at(&file.tree, o).map(|v| hide_emails(&v));
    }
    if let Some(e) = &f.expires {
        if let Some(v) = get(&file.tree, e) {
            if let Some(t) = util::parse_time(v, &f.expires_unit) {
                a.expires_at = Some(util::rfc3339(t));
                a.expired = t < now;
            }
        }
    }
    // claims of a JWT, decoded locally
    if let Some(j) = &f.jwt {
        if let Some(tok) = str_at(&file.tree, j) {
            if let Some(claims) = util::jwt_payload(&tok) {
                let c = &f.jwt_claims;
                if let Some(p) = c.plan.as_ref().and_then(|p| str_at(&claims, p)) {
                    a.credit = Credit::Subscription { plan: Some(p) };
                } else if matches!(a.credit, Credit::Unknown) {
                    a.credit = Credit::Subscription { plan: None };
                }
                if a.email_domain.is_none() {
                    if let Some(e) = c.email.as_ref().and_then(|p| str_at(&claims, p)) {
                        a.email_domain = util::email_domain(&e);
                    }
                }
                if a.expires_at.is_none() {
                    if let Some(v) = c.expires.as_ref().and_then(|p| get(&claims, p)) {
                        if let Some(t) = util::parse_time(v, "s") {
                            a.expires_at = Some(util::rfc3339(t));
                            a.expired = t < now;
                        }
                    }
                }
            }
        }
    }
    // a settings file that says nothing about an account is not one; one that only names
    // the provider is kept as a hint for the keyring row (see `accounts`)
    if loc.ext.what == LocKind::Settings
        && matches!(a.credit, Credit::Unknown)
        && a.email_domain.is_none()
        && a.organization.is_none()
        && a.expires_at.is_none()
        && a.provider == "unknown"
    {
        return Vec::new();
    }
    vec![a]
}

/// An organization name may be generated from an address ("x@y.com's Organization"):
/// keep the domain only.
pub fn hide_emails(s: &str) -> String {
    s.split(' ')
        .map(|w| match w.rsplit_once('@') {
            Some((local, rest)) if !local.is_empty() && rest.contains('.') => format!("…@{rest}"),
            _ => w.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Provider ids as the hosts write them → Billfold's provider ids.
pub fn normalise_provider(s: &str) -> String {
    let l = s.trim().to_lowercase();
    match l.as_str() {
        "openai-native" | "openai-codex" | "chatgpt" | "codex" => "openai".into(),
        "claude" | "claude-code" | "anthropic-vertex" => "anthropic".into(),
        "gemini" | "google-gemini" | "vertex" | "vertex-ai" | "google-vertex" | "googleai"
        | "gemini-cli" => "google".into(),
        "github.com" | "github-copilot" | "copilot" | "gh" => "github".into(),
        "bedrock" | "amazon-bedrock" | "aws-bedrock" => "aws".into(),
        "azure-openai" | "azure" => "azure".into(),
        "zhipu" | "zhipuai" | "glm" => "z-ai".into(),
        "dashscope" | "qwen" | "alibaba-cloud" => "alibaba".into(),
        "kimi" | "moonshotai" => "moonshot".into(),
        "hf" | "hugging-face" => "huggingface".into(),
        _ => l.split(':').next().unwrap_or(&l).to_string(),
    }
}

/// All accounts of the present hosts, merged per (host, provider, legacy): the most
/// informative credit wins, descriptive fields are unioned.
pub fn accounts(
    sheets: &crate::sheets::Sheets,
    env: &Env,
    present: &[String],
    now: DateTime<Utc>,
    errors: &mut Vec<String>,
) -> (Vec<Account>, Vec<Loaded>) {
    let mut raw: Vec<Account> = Vec::new();
    let mut loaded: Vec<Loaded> = Vec::new();
    for host in &sheets.hosts {
        if !present.contains(&host.id) {
            continue;
        }
        for loc in &host.locations {
            if !matches!(loc.ext.what, LocKind::Credential | LocKind::Settings) {
                continue;
            }
            for r in resolve(loc, env, &env.roots) {
                if !r.path.is_file() {
                    continue;
                }
                match load(loc, &r.path) {
                    Ok(file) => {
                        raw.extend(accounts_from(host, loc, &file, now));
                        loaded.push(file);
                    }
                    Err(e) => {
                        // an encrypted or opaque file still says the host is wired
                        if loc.ext.what == LocKind::Credential {
                            let (mode, _) = util::mode_of(&r.path);
                            raw.push(Account {
                                host: host.id.clone(),
                                host_name: host.name.clone(),
                                provider: loc
                                    .ext
                                    .provider
                                    .clone()
                                    .unwrap_or_else(|| "unknown".into()),
                                credit: Credit::Unknown,
                                email_domain: None,
                                organization: None,
                                expires_at: None,
                                expired: false,
                                path: r.path.clone(),
                                mode,
                                modified: util::modified_of(&r.path),
                                legacy: loc.legacy,
                                note: Some(format!("present, not parsed: {e}")),
                            });
                        } else {
                            errors.push(format!("{}: {e}", r.path.display()));
                        }
                    }
                }
            }
        }
        // a host whose secrets live in the keyring, with no readable credential file: the
        // settings' provider names the account
        if let Some(k) = &host.ext.keyring {
            let any_cred = raw
                .iter()
                .any(|a| a.host == host.id && !a.legacy && !matches!(a.credit, Credit::Unknown));
            if !any_cred {
                let provider = raw
                    .iter()
                    .find(|a| a.host == host.id && a.provider != "unknown")
                    .map(|a| a.provider.clone())
                    .or_else(|| host.locations.iter().find_map(|l| l.ext.provider.clone()))
                    .unwrap_or_else(|| "unknown".into());
                raw.retain(|a| {
                    !(a.host == host.id
                        && matches!(a.credit, Credit::Unknown)
                        && a.provider == provider)
                });
                raw.push(Account {
                    host: host.id.clone(),
                    host_name: host.name.clone(),
                    provider,
                    credit: Credit::Keyring,
                    email_domain: None,
                    organization: None,
                    expires_at: None,
                    expired: false,
                    path: PathBuf::from(format!("keyring:{}", k.service)),
                    mode: None,
                    modified: None,
                    legacy: false,
                    note: Some(k.source.quote.clone()),
                });
            }
        }
    }
    (merge(raw), loaded)
}

fn rank(c: &Credit) -> u8 {
    match c {
        Credit::Subscription { plan: Some(_) } => 4,
        Credit::Subscription { plan: None } => 3,
        Credit::ApiKey => 2,
        Credit::Keyring => 1,
        Credit::Unknown => 0,
    }
}

fn merge(raw: Vec<Account>) -> Vec<Account> {
    let mut by_key: BTreeMap<(String, String, bool, bool), Account> = BTreeMap::new();
    for a in raw {
        let is_keyring = matches!(a.credit, Credit::Keyring);
        let key = (a.host.clone(), a.provider.clone(), a.legacy, is_keyring);
        match by_key.get_mut(&key) {
            None => {
                by_key.insert(key, a);
            }
            Some(m) => {
                // the credential file (not the settings) is the path shown
                let a_is_cred = !a.path.to_string_lossy().ends_with(".claude.json")
                    && !a.path.to_string_lossy().ends_with("settings.json")
                    && !a.path.to_string_lossy().ends_with("config.yaml")
                    && !a.path.to_string_lossy().ends_with("globalState.json");
                if rank(&a.credit) > rank(&m.credit) {
                    m.credit = a.credit.clone();
                }
                if m.email_domain.is_none() {
                    m.email_domain = a.email_domain.clone();
                }
                if m.organization.is_none() {
                    m.organization = a.organization.clone();
                }
                if m.expires_at.is_none() {
                    m.expires_at = a.expires_at.clone();
                    m.expired = a.expired;
                }
                if a_is_cred
                    && (m.path.to_string_lossy().ends_with(".claude.json")
                        || m.path.to_string_lossy().ends_with("settings.json"))
                {
                    m.path = a.path.clone();
                    m.mode = a.mode.clone();
                    m.modified = a.modified.clone();
                }
                if m.note.is_none() {
                    m.note = a.note.clone();
                }
            }
        }
    }
    let mut out: Vec<Account> = by_key.into_values().collect();
    // the settings-only "unknown" rows disappear when a real credential exists for the host
    let hosts_with_real: Vec<String> = out
        .iter()
        .filter(|a| rank(&a.credit) >= 2)
        .map(|a| a.host.clone())
        .collect();
    out.retain(|a| {
        !(a.provider == "unknown" && rank(&a.credit) == 0 && hosts_with_real.contains(&a.host))
    });
    out
}
