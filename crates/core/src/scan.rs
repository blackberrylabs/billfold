//! The scan: which hosts are here, what they are wired to, which keys the machine holds,
//! what the logs say was consumed — priced per request, then aggregated — and the
//! findings. Read-only from the first line to the last.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use bbl_hostrules::env::Env;
use bbl_hostrules::read::{get, read_tree};
use bbl_hostrules::resolve::{expand_glob, project_roots, resolve};
use chrono::{DateTime, Duration, Local, Utc};
use serde_json::Value;

use crate::meter;
use crate::model::*;
use crate::prices::{price_request, PriceTable};
use crate::providers::Providers;
use crate::sheets::{LocKind, Sheets};
use crate::util::{self, display_home};
use crate::{keys, wiring};

#[derive(Debug, Clone)]
pub struct Options {
    pub now: DateTime<Utc>,
    pub since_days: i64,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            now: Utc::now(),
            since_days: 30,
        }
    }
}

pub fn scan(env: &Env) -> Inventory {
    scan_with(
        env,
        &crate::sheets::embedded(),
        &crate::providers::embedded(),
        PriceTable::embedded(),
        &Options::default(),
    )
}

pub fn scan_with(
    env: &Env,
    sheets: &Sheets,
    providers: &Providers,
    prices: &PriceTable,
    opt: &Options,
) -> Inventory {
    let since = opt.now - Duration::days(opt.since_days);
    let mut inv = Inventory {
        home: env.home.clone(),
        since: since.with_timezone(&Local).format("%Y-%m-%d").to_string(),
        prices: prices.info().clone(),
        not_read: sheets.not_read.clone(),
        ..Default::default()
    };
    let roots = project_roots(env);

    // ---- presence
    let mut present: Vec<String> = Vec::new();
    let mut presence: BTreeMap<String, String> = BTreeMap::new();
    for h in &sheets.hosts {
        let mut why = Vec::new();
        for p in &h.ext.detect.paths {
            if let Some(e) = env.expand(p) {
                if expand_glob(&e).iter().any(|x| x.exists()) {
                    why.push(display_home(&e, &env.home));
                }
            }
        }
        for b in &h.ext.detect.binaries {
            if env.which(b).is_some() {
                why.push(format!("`{b}` on PATH"));
            }
        }
        if !why.is_empty() {
            present.push(h.id.clone());
            presence.insert(h.id.clone(), why.join(", "));
        } else {
            let looked: Vec<String> = h
                .ext
                .detect
                .paths
                .iter()
                .cloned()
                .chain(h.ext.detect.binaries.iter().map(|b| format!("`{b}`")))
                .collect();
            presence.insert(h.id.clone(), format!("not found: {}", looked.join(", ")));
        }
    }

    // ---- wiring and keys
    let (accounts, _loaded) = wiring::accounts(sheets, env, &present, opt.now, &mut inv.errors);
    inv.accounts = accounts;
    // a provider whose login may pay with credits or with a plan: the store does not say
    for a in &mut inv.accounts {
        if matches!(a.credit, Credit::Subscription { plan: None })
            && providers
                .get(&a.provider)
                .is_some_and(|p| p.login_credit.as_deref() == Some("either"))
        {
            a.credit = Credit::Unknown;
            let n = "the login pays with credits or with a plan; the store does not say which";
            a.note = Some(match a.note.take() {
                Some(x) => format!("{x} {n}."),
                None => format!("{n}."),
            });
        }
    }
    inv.keys = keys::keys(sheets, providers, env, &present, &roots);

    // ---- meter
    let mut requests_by_host: Vec<(String, meter::Request)> = Vec::new();
    for h in &sheets.hosts {
        if !present.contains(&h.id) {
            continue;
        }
        for loc in &h.locations {
            if loc.ext.what != LocKind::Usage {
                continue;
            }
            let Some(spec) = &loc.ext.usage else { continue };
            let files: Vec<PathBuf> = resolve(loc, env, &roots)
                .into_iter()
                .map(|r| r.path)
                .filter(|p| p.is_file())
                .filter(|p| {
                    util::modified_dt(p)
                        .is_none_or(|m| m >= since || spec.reader.ends_with("sqlite"))
                })
                .collect();
            if files.is_empty() {
                continue;
            }
            let read = meter::read(&spec.reader, &h.id, &h.name, &files, since);
            inv.errors.extend(read.errors);
            inv.quotas.extend(read.quotas);
            let retention = retention_days(spec, env);
            for (path, n) in read.files {
                let modified = util::modified_dt(&path);
                let days_left = match (retention, modified) {
                    (Some(r), Some(m)) => Some(r - (opt.now - m).num_days()),
                    _ => None,
                };
                inv.logs.push(LogFile {
                    host: h.id.clone(),
                    path: path.clone(),
                    bytes: util::size_of(&path),
                    modified: modified.map(util::rfc3339),
                    days_left,
                    requests: n,
                });
            }
            for r in read.requests {
                requests_by_host.push((h.id.clone(), r));
            }
        }
    }

    // ---- price and aggregate
    let mut lines: BTreeMap<(String, String, Option<PathBuf>, String), UsageLine> = BTreeMap::new();
    let mut unpriced: BTreeMap<(String, String), Tokens> = BTreeMap::new();
    let mut disagree: BTreeMap<(String, String), (f64, f64)> = BTreeMap::new();
    for (host, r) in requests_by_host {
        let host_name = sheets.name_of(&host).to_string();
        let provider_id = r
            .provider_hint
            .clone()
            .or_else(|| provider_of_host(&inv.accounts, &host));
        let provider = provider_id.as_deref().and_then(|p| providers.get(p));
        let credit = r
            .credit_hint
            .clone()
            .unwrap_or_else(|| credit_of(&inv.accounts, &host, provider_id.as_deref()));
        let price = prices.lookup(&r.model, provider, providers);
        let (usd, mut note) = match &price {
            Some(p) => {
                let priced = price_request(&r.tokens, &r.billing, p, provider, r.when);
                (
                    Some(priced.usd),
                    if priced.notes.is_empty() {
                        None
                    } else {
                        Some(priced.notes.join("; "))
                    },
                )
            }
            None => (None, Some("model not in the price table".into())),
        };
        if usd.is_none() {
            unpriced
                .entry((host.clone(), r.model.clone()))
                .or_default()
                .add(&r.tokens);
        }
        if let (Some(u), Some(h)) = (usd, r.host_cost_usd) {
            if !matches!(credit, Credit::Subscription { .. }) {
                let e = disagree
                    .entry((host.clone(), r.model.clone()))
                    .or_insert((0.0, 0.0));
                e.0 += u;
                e.1 += h;
            }
        }
        let free = price.as_ref().is_some_and(|p| p.tier == "free");
        if matches!(credit, Credit::Subscription { .. }) && !free {
            note = Some(match note {
                Some(n) => format!("included in plan; {n}"),
                None => "included in plan".into(),
            });
        }
        let day = r
            .when
            .map(|w| w.with_timezone(&Local).format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "undated".into());
        let key = (
            host.clone(),
            r.model.clone(),
            r.project.clone(),
            day.clone(),
        );
        let line = lines.entry(key).or_insert_with(|| UsageLine {
            host: host.clone(),
            host_name: host_name.clone(),
            model: r.model.clone(),
            provider: price
                .as_ref()
                .map(|p| p.provider_id.clone())
                .or(provider_id.clone()),
            project: r.project.clone(),
            day,
            tokens: Tokens::default(),
            billing: Billing::default(),
            host_cost_usd: None,
            usd: None,
            pricing_note: note.clone(),
            credit: credit.clone(),
            sources: Vec::new(),
        });
        line.tokens.add(&r.tokens);
        line.billing.cache_write_1h += r.billing.cache_write_1h;
        if line.billing.tier.is_none() {
            line.billing.tier = r.billing.tier.clone();
        }
        if line.billing.geo.is_none() {
            line.billing.geo = r.billing.geo.clone();
        }
        if let Some(h) = r.host_cost_usd {
            line.host_cost_usd = Some(line.host_cost_usd.unwrap_or(0.0) + h);
        }
        if let Some(u) = usd {
            line.usd = Some(line.usd.unwrap_or(0.0) + u);
        }
        if !line.sources.contains(&r.source) {
            line.sources.push(r.source.clone());
        }
    }
    inv.usage = lines.into_values().collect();

    // ---- host status
    let by_host = inv.tokens_by_host();
    for h in &sheets.hosts {
        let t = by_host.get(&h.id).copied().unwrap_or_default();
        inv.hosts.push(HostStatus {
            id: h.id.clone(),
            name: h.name.clone(),
            present: present.contains(&h.id),
            presence: presence.get(&h.id).cloned().unwrap_or_default(),
            has_usage_log: h.locations.iter().any(|l| l.ext.what == LocKind::Usage),
            accounts: inv.accounts.iter().filter(|a| a.host == h.id).count(),
            requests: t.requests,
            tokens: t,
        });
    }

    // ---- findings
    findings(
        &mut inv, sheets, providers, env, &present, opt, unpriced, disagree,
    );
    inv.findings.sort_by(|a, b| {
        a.kind
            .severity()
            .cmp(&b.kind.severity())
            .then(a.kind.cmp(&b.kind))
            .then(a.id.cmp(&b.id))
    });
    inv.notes.push(format!(
        "prices: {} snapshot of {} ({}, {}); dollars are API-equivalent estimates",
        inv.prices.name, inv.prices.as_of, inv.prices.license, inv.prices.attribution
    ));
    inv
}

/// The retention the host applies, days, honouring its setting when the file says so.
fn retention_days(spec: &crate::sheets::UsageSpec, env: &Env) -> Option<i64> {
    let mut days = spec.retention_days;
    if let (Some(key), Some(file)) = (&spec.retention_setting, &spec.retention_file) {
        if let Some(p) = env.expand(file) {
            if let Ok((_, tree)) = read_tree(&p, bbl_hostrules::rules::Format::Jsonc) {
                if let Some(v) = get(&tree, key) {
                    days = match v {
                        Value::Number(n) => n.as_i64(),
                        Value::String(s) => parse_duration_days(s),
                        _ => days,
                    };
                }
            }
        }
    }
    days
}

/// `30d`, `2w`, `12h` → days.
fn parse_duration_days(s: &str) -> Option<i64> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let n: i64 = num.parse().ok()?;
    Some(match unit {
        "d" | "" => n,
        "w" => n * 7,
        "h" => n / 24,
        "m" => n * 30,
        "y" => n * 365,
        _ => return None,
    })
}

fn provider_of_host(accounts: &[Account], host: &str) -> Option<String> {
    accounts
        .iter()
        .find(|a| a.host == host && !a.legacy && a.provider != "unknown")
        .map(|a| a.provider.clone())
}

fn credit_of(accounts: &[Account], host: &str, provider: Option<&str>) -> Credit {
    let mut best = Credit::Unknown;
    for a in accounts
        .iter()
        .filter(|a| a.host == host && !a.legacy && provider.is_none_or(|p| a.provider == p))
    {
        best = match (&best, &a.credit) {
            (Credit::Subscription { .. }, _) => best,
            (_, Credit::Subscription { plan }) => Credit::Subscription { plan: plan.clone() },
            (Credit::ApiKey, _) => best,
            (_, Credit::ApiKey) => Credit::ApiKey,
            (_, Credit::Keyring) => Credit::Keyring,
            _ => best,
        };
    }
    best
}

fn rel(p: &Path, home: &Path) -> String {
    display_home(p, home).trim_start_matches("~/").to_string()
}

#[allow(clippy::too_many_arguments)]
fn findings(
    inv: &mut Inventory,
    sheets: &Sheets,
    providers: &Providers,
    env: &Env,
    present: &[String],
    opt: &Options,
    unpriced: BTreeMap<(String, String), Tokens>,
    disagree: BTreeMap<(String, String), (f64, f64)>,
) {
    let home = env.home.clone();
    let mut out: Vec<Finding> = Vec::new();
    let host_name = |id: &str| sheets.name_of(id).to_string();
    let src_of_host = |id: &str, what: LocKind| -> Option<String> {
        sheets.host(id).and_then(|h| {
            h.locations
                .iter()
                .find(|l| l.ext.what == what)
                .and_then(|l| l.source.url.clone())
        })
    };

    // keys in clear
    for k in &inv.keys {
        if k.reference || k.place == KeyPlace::ProcessEnv {
            continue;
        }
        let (kind, title, hint) = match (k.git, k.exposed) {
            (GitState::Tracked, _) => (
                Kind::TrackedSecret,
                format!("{} key in clear in a git-tracked file", k.provider_name),
                format!("remove the key from `{}`, add the file to `.gitignore`, rotate the key at {}", rel(&k.path, &home), k.provider_name),
            ),
            (_, true) => (
                Kind::ExposedSecret,
                format!("{} key readable by other users", k.provider_name),
                format!("chmod 600 {} and rotate the key if the machine is shared", display_home(&k.path, &home)),
            ),
            _ => (
                Kind::ClearSecret,
                format!("{} key in clear", k.provider_name),
                match k.place {
                    KeyPlace::ShellRc => "prefer a secrets manager or the host's own login; a key in a start-up file is inherited by every process".into(),
                    KeyPlace::McpConfig => "use a `${VAR}` reference in the server definition and keep the value in your environment".into(),
                    _ => "the file is owner-only; keep it out of backups and shared folders, rotate the key when in doubt".into(),
                },
            ),
        };
        let provider = providers.get(&k.provider);
        out.push(Finding {
            kind,
            id: format!(
                "{}/{}/{}{}",
                kind.slug(),
                k.provider,
                rel(&k.path, &home),
                k.line.map(|l| format!(":{l}")).unwrap_or_default()
            ),
            host: k.used_by.first().cloned(),
            host_name: k.used_by.first().map(|h| host_name(h)),
            title,
            subject: format!(
                "{} · {} · {}",
                k.provider_name,
                k.name,
                display_home(&k.path, &home)
            ),
            detail: format!(
                "`{}` = {} at {}{}{}; git: {}{}",
                k.name,
                k.shown,
                display_home(&k.path, &home),
                k.line.map(|l| format!(" line {l}")).unwrap_or_default(),
                k.mode
                    .as_ref()
                    .map(|m| format!(" (mode {m})"))
                    .unwrap_or_default(),
                match k.git {
                    GitState::Tracked => "tracked",
                    GitState::Ignored => "ignored",
                    GitState::Untracked => "untracked",
                    GitState::NotInRepo => "not in a repository",
                },
                if k.used_by.is_empty() {
                    String::new()
                } else {
                    format!("; read by {}", k.used_by.join(", "))
                }
            ),
            path: Some(k.path.clone()),
            line: k.line,
            hint: Some(hint),
            command: None,
            url: provider.and_then(|p| p.usage_url.clone()),
            source_url: provider.and_then(|p| p.source.url.clone()),
        });
    }
    // shared keys
    let mut by_fp: HashMap<&str, Vec<&Key>> = HashMap::new();
    for k in inv
        .keys
        .iter()
        .filter(|k| !k.reference && !k.fingerprint.is_empty())
    {
        by_fp.entry(&k.fingerprint).or_default().push(k);
    }
    for (fp, ks) in by_fp {
        let places: Vec<String> = {
            let mut v: Vec<String> = ks.iter().map(|k| display_home(&k.path, &home)).collect();
            v.sort();
            v.dedup();
            v
        };
        if places.len() < 2 {
            continue;
        }
        let k = ks[0];
        out.push(Finding {
            kind: Kind::SharedKey,
            id: format!("shared-key/{}/{}", k.provider, fp),
            host: None,
            host_name: None,
            title: format!("the same {} key in {} places", k.provider_name, places.len()),
            subject: format!("{} · {}", k.provider_name, k.shown),
            detail: format!("key {} (fingerprint {fp}, this run only) found in: {}", k.shown, places.join(", ")),
            path: Some(k.path.clone()),
            line: k.line,
            hint: Some("one key per tool makes rotation and revocation local; create separate keys in the provider's console".into()),
            command: None,
            url: providers.get(&k.provider).and_then(|p| p.usage_url.clone()),
            source_url: None,
        });
    }
    // accounts: expired, legacy, keyring
    for a in &inv.accounts {
        if a.expired {
            out.push(Finding {
                kind: Kind::Expired,
                id: format!("expired/{}/{}", a.host, rel(&a.path, &home)),
                host: Some(a.host.clone()),
                host_name: Some(a.host_name.clone()),
                title: format!(
                    "{}: {} session expired",
                    a.host_name,
                    providers.name_of(&a.provider)
                ),
                subject: format!("{} · {}", a.host_name, display_home(&a.path, &home)),
                detail: format!(
                    "expiry {} is past; the host refreshes or asks for a new login",
                    a.expires_at.clone().unwrap_or_default()
                ),
                path: Some(a.path.clone()),
                line: None,
                hint: Some("log in again from the host if it no longer refreshes by itself".into()),
                command: sheets.host(&a.host).and_then(|h| h.ext.check.clone()),
                url: None,
                source_url: src_of_host(&a.host, LocKind::Credential),
            });
        }
        if a.legacy {
            out.push(Finding {
                kind: Kind::LegacyStore,
                id: format!("legacy-store/{}/{}", a.host, rel(&a.path, &home)),
                host: Some(a.host.clone()),
                host_name: Some(a.host_name.clone()),
                title: format!("{}: an older credential file is still there", a.host_name),
                subject: format!("{} · {}", a.host_name, display_home(&a.path, &home)),
                detail: a.note.clone().unwrap_or_else(|| {
                    "the current version of the host stores its login elsewhere".into()
                }),
                path: Some(a.path.clone()),
                line: None,
                hint: Some(format!(
                    "if the host no longer reads it, delete {} yourself — Billfold never deletes",
                    display_home(&a.path, &home)
                )),
                command: None,
                url: None,
                source_url: src_of_host(&a.host, LocKind::Credential),
            });
        }
        if matches!(a.credit, Credit::Keyring) {
            out.push(Finding {
                kind: Kind::InKeyring,
                id: format!("in-keyring/{}", a.host),
                host: Some(a.host.clone()),
                host_name: Some(a.host_name.clone()),
                title: format!("{}: secrets in the system keyring", a.host_name),
                subject: a.host_name.clone(),
                detail: format!(
                    "{} — Billfold does not read the keyring",
                    a.note.clone().unwrap_or_default()
                ),
                path: None,
                line: None,
                hint: None,
                command: sheets.host(&a.host).and_then(|h| h.ext.check.clone()),
                url: None,
                source_url: sheets
                    .host(&a.host)
                    .and_then(|h| h.ext.keyring.as_ref())
                    .and_then(|k| k.source.url.clone()),
            });
        }
    }
    // double credit: a subscription on the host and a key of the same provider in the shell environment
    for a in inv
        .accounts
        .iter()
        .filter(|a| matches!(a.credit, Credit::Subscription { .. }) && !a.legacy)
    {
        let env_keys: Vec<&Key> = inv
            .keys
            .iter()
            .filter(|k| {
                k.provider == a.provider
                    && !k.reference
                    && matches!(
                        k.place,
                        KeyPlace::ProcessEnv | KeyPlace::ShellRc | KeyPlace::EnvironmentD
                    )
            })
            .collect();
        if let Some(k) = env_keys.first() {
            out.push(Finding {
                kind: Kind::DoubleCredit,
                id: format!("double-credit/{}/{}", a.host, a.provider),
                host: Some(a.host.clone()),
                host_name: Some(a.host_name.clone()),
                title: format!("{}: subscription and {} API key both present", a.host_name, providers.name_of(&a.provider)),
                subject: format!("{} · {}", a.host_name, providers.name_of(&a.provider)),
                detail: format!(
                    "logged in on {} ({}) and `{}` is set in {} — the host decides which credential it uses",
                    a.host_name,
                    match &a.credit { Credit::Subscription { plan: Some(p) } => format!("plan {p}"), _ => "a subscription".into() },
                    k.name,
                    display_home(&k.path, &home)
                ),
                path: Some(k.path.clone()),
                line: k.line,
                hint: Some("check which credential the host reports as in use; unset the key for this host if the plan should pay".into()),
                command: sheets.host(&a.host).and_then(|h| h.ext.check.clone()),
                url: providers.get(&a.provider).and_then(|p| p.usage_url.clone()),
                source_url: src_of_host(&a.host, LocKind::Credential),
            });
        }
    }
    // unpriced models
    for ((host, model), t) in unpriced {
        if t.total() == 0 {
            continue;
        }
        out.push(Finding {
            kind: Kind::Unpriced,
            id: format!("unpriced/{host}/{model}"),
            host: Some(host.clone()),
            host_name: Some(host_name(&host)),
            title: format!("{model}: not in the price table"),
            subject: format!("{} · {model}", host_name(&host)),
            detail: format!("{} tokens in {} request(s) counted, no dollar equivalent ({} snapshot of {})", crate::report::human(t.total()), t.requests, inv.prices.name, inv.prices.as_of),
            path: None,
            line: None,
            hint: Some("the model id is as the host wrote it; a newer Billfold or a newer snapshot of the index may know it".into()),
            command: None,
            url: Some("https://apipriceindex.com/".into()),
            source_url: None,
        });
    }
    // host disagrees
    for ((host, model), (ours, theirs)) in disagree {
        if ours < 0.05 || theirs < 0.05 {
            continue;
        }
        let ratio = (ours - theirs).abs() / theirs;
        if ratio > 0.10 {
            out.push(Finding {
                kind: Kind::HostDisagrees,
                id: format!("host-disagrees/{host}/{model}"),
                host: Some(host.clone()),
                host_name: Some(host_name(&host)),
                title: format!("{}: its own cost for {model} differs by {:.0} %", host_name(&host), ratio * 100.0),
                subject: format!("{} · {model}", host_name(&host)),
                detail: format!("the host wrote ${theirs:.2}, Billfold estimates ${ours:.2} at the {} prices of {}", inv.prices.name, inv.prices.as_of),
                path: None,
                line: None,
                hint: Some("a different price table on the host's side, a promotion, a tier or a provider-reported figure; the host's number is what it was charged if it came from the provider".into()),
                command: None,
                url: None,
                source_url: src_of_host(&host, LocKind::Usage),
            });
        }
    }
    // near limit
    for q in &inv.quotas {
        if q.used_percent >= 80.0 {
            out.push(Finding {
                kind: Kind::NearLimit,
                id: format!("near-limit/{}/{}", q.host, q.window),
                host: Some(q.host.clone()),
                host_name: Some(q.host_name.clone()),
                title: format!("{}: {} window at {:.0} % the last time it was written", q.host_name, q.window, q.used_percent),
                subject: format!("{} · {} window", q.host_name, q.window),
                detail: format!(
                    "{:.0} % used{}{}{} — as the host wrote it{}",
                    q.used_percent,
                    q.window_minutes.map(|m| format!(" of a {m}-minute window")).unwrap_or_default(),
                    q.resets_at.as_ref().map(|r| format!(", resets at {r}")).unwrap_or_default(),
                    q.plan.as_ref().map(|p| format!(" (plan {p})")).unwrap_or_default(),
                    q.as_of.as_ref().map(|t| format!(" on {t}")).unwrap_or_default()
                ),
                path: Some(q.path.clone()),
                line: None,
                hint: Some("a snapshot, not a live figure; the host's own status command has the current one".into()),
                command: sheets.host(&q.host).and_then(|h| h.ext.check.clone()),
                url: None,
                source_url: src_of_host(&q.host, LocKind::Usage),
            });
        }
    }
    // pruning soon: per host, the files within 7 days of deletion
    let mut soon: BTreeMap<String, (usize, u64, i64)> = BTreeMap::new();
    for l in &inv.logs {
        if let Some(d) = l.days_left {
            if (0..=7).contains(&d) && l.requests > 0 {
                let e = soon.entry(l.host.clone()).or_insert((0, 0, i64::MAX));
                e.0 += 1;
                e.1 += l.requests;
                e.2 = e.2.min(d);
            }
        }
    }
    for (host, (files, reqs, min_days)) in soon {
        let spec = sheets
            .host(&host)
            .and_then(|h| h.locations.iter().find_map(|l| l.ext.usage.clone()));
        out.push(Finding {
            kind: Kind::PruningSoon,
            id: format!("pruning-soon/{host}"),
            host: Some(host.clone()),
            host_name: Some(host_name(&host)),
            title: format!("{}: {files} log file(s) deleted within {min_days} day(s)", host_name(&host)),
            subject: host_name(&host),
            detail: format!(
                "{reqs} request(s) in {files} file(s) are older than the host's retention minus 7 days{}",
                spec.as_ref().and_then(|s| s.retention_setting.as_ref()).map(|k| format!(" (`{k}`)")).unwrap_or_default()
            ),
            path: None,
            line: None,
            hint: Some(spec.as_ref().and_then(|s| s.retention_setting.as_ref()).map(|k| format!("raise `{k}` in the host's settings if you want to keep the history — Billfold does not copy logs")).unwrap_or_else(|| "copy what you want to keep — Billfold does not".into())),
            command: None,
            url: None,
            source_url: src_of_host(&host, LocKind::Usage),
        });
    }
    // no usage log
    for h in &sheets.hosts {
        if !present.contains(&h.id) {
            continue;
        }
        if let Some(reason) = &h.ext.no_usage_log {
            if !h.locations.iter().any(|l| l.ext.what == LocKind::Usage) {
                out.push(Finding {
                    kind: Kind::NoUsageLog,
                    id: format!("no-usage-log/{}", h.id),
                    host: Some(h.id.clone()),
                    host_name: Some(h.name.clone()),
                    title: format!("{}: no usage log on this machine", h.name),
                    subject: h.name.clone(),
                    detail: reason.clone(),
                    path: None,
                    line: None,
                    hint: h
                        .ext
                        .usage_url
                        .as_ref()
                        .map(|u| format!("usage is shown at {u}")),
                    command: h.ext.check.clone(),
                    url: h.ext.usage_url.clone(),
                    source_url: h
                        .locations
                        .first()
                        .and_then(|l| l.source.url.clone())
                        .or_else(|| h.ext.keyring.as_ref().and_then(|k| k.source.url.clone())),
                });
            }
        }
    }
    let _ = opt;
    inv.findings = out;
}

/// Findings for present hosts only (all of them: Billfold has no "what if" view).
pub fn present_findings(inv: &Inventory) -> impl Iterator<Item = &Finding> {
    inv.findings.iter()
}
