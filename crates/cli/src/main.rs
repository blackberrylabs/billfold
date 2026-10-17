//! Billfold command line: `billfold` (the findings, most serious first), `billfold wiring`
//! (the accounts each host is logged into), `billfold keys` (the provider keys on the
//! machine, masked), `billfold meter` (tokens and API-equivalent dollars), `billfold hosts`,
//! `billfold providers`, `billfold report` (for the Blackberrylabs control tower, always
//! JSON); `--json` for scripts. Read-only.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use bbl_hostrules::env::Env;
use billfold_core::model::{Credit, GitState, Inventory, KeyPlace, Tokens};
use billfold_core::report::human;
use billfold_core::util::display_home;
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "billfold",
    version,
    about = "What this machine spends on AI: the accounts it is wired to, the keys it holds, the tokens it burned. Read-only."
)]
struct Cli {
    /// A project root to look at for `.env` files and per-project configs; repeatable
    /// (default: the projects Claude Code knows).
    #[arg(short, long = "project", global = true, value_name = "DIR", num_args = 1..)]
    paths: Vec<PathBuf>,
    /// Machine-readable output.
    #[arg(long, global = true)]
    json: bool,
    /// Look at another home directory (a backup, a fixture) instead of yours.
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    /// Only this host (its id: claude-code, codex, cursor…).
    #[arg(long, global = true)]
    host: Option<String>,
    /// The window of the meter, in days (default 30 — the retention of Claude Code and
    /// Gemini CLI; older logs may already be gone).
    #[arg(long, global = true, value_name = "DAYS", default_value_t = 30)]
    since: i64,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// The findings, most serious first (default).
    List,
    /// The accounts each present host is logged into, and how they pay.
    Wiring,
    /// The provider keys on this machine, masked: where, who reads them, how git sees them.
    Keys,
    /// Tokens consumed in the window, by host, model, project or day; dollars as
    /// API-equivalent at the embedded snapshot's prices.
    Meter {
        /// Group by.
        #[arg(long, value_enum, default_value_t = By::Model)]
        by: By,
    },
    /// The hosts Billfold knows, their presence, where their files and rules come from.
    Hosts,
    /// The providers Billfold knows: variable names, usage pages, price rules.
    Providers,
    /// The inventory as a `bbl-report/1` document for the Blackberrylabs control tower
    /// (always JSON).
    Report,
}

#[derive(Clone, Copy, ValueEnum, PartialEq, Eq)]
enum By {
    Host,
    Model,
    Project,
    Day,
}

fn main() -> Result<()> {
    match run() {
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::BrokenPipe) =>
        {
            Ok(())
        }
        r => r,
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut env = match &cli.home {
        Some(h) => Env::fixture(h, bbl_hostrules::env::Os::Linux),
        None => Env::current(),
    };
    let roots: Vec<PathBuf> = cli
        .paths
        .iter()
        .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()))
        .collect();
    env = env.with_roots(roots);
    let opt = billfold_core::Options {
        since_days: cli.since.max(1),
        ..Default::default()
    };
    let started = std::time::Instant::now();
    let inv = billfold_core::scan_with(
        &env,
        &billfold_core::sheets::embedded(),
        &billfold_core::providers::embedded(),
        billfold_core::prices::PriceTable::embedded(),
        &opt,
    );
    let duration_ms = started.elapsed().as_millis() as u64;
    let mut out = std::io::stdout().lock();
    match cli.command.unwrap_or(Command::List) {
        Command::Report => out.write_all(
            billfold_core::report::build(&inv, duration_ms)
                .to_json()
                .as_bytes(),
        )?,
        _ if cli.json => writeln!(out, "{}", serde_json::to_string_pretty(&inv)?)?,
        Command::List => list(&mut out, &inv, cli.host.as_deref())?,
        Command::Wiring => wiring(&mut out, &inv, cli.host.as_deref())?,
        Command::Keys => keys(&mut out, &inv)?,
        Command::Meter { by } => meter(&mut out, &inv, cli.host.as_deref(), by)?,
        Command::Hosts => hosts(&mut out, &inv)?,
        Command::Providers => providers(&mut out)?,
    }
    Ok(())
}

fn credit(c: &Credit) -> String {
    match c {
        Credit::Subscription { plan: Some(p) } => format!("plan {p}"),
        Credit::Subscription { plan: None } => "subscription".into(),
        Credit::ApiKey => "API key".into(),
        Credit::Keyring => "keyring".into(),
        Credit::Unknown => "unknown".into(),
    }
}

fn header(out: &mut impl Write, inv: &Inventory) -> Result<()> {
    let present: Vec<&str> = inv
        .hosts
        .iter()
        .filter(|h| h.present)
        .map(|h| h.name.as_str())
        .collect();
    let t = inv.total_tokens();
    writeln!(
        out,
        "{} provider(s), {} key(s), {} tokens since {} ≈ ${:.2} API-equivalent pay-as-you-go · hosts present: {}",
        inv.providers().len(),
        inv.keys.iter().filter(|k| !k.reference).count(),
        human(t.total()),
        inv.since,
        inv.total_usd(),
        if present.is_empty() { "none".to_string() } else { present.join(", ") }
    )?;
    writeln!(out)?;
    Ok(())
}

fn list(out: &mut impl Write, inv: &Inventory, host: Option<&str>) -> Result<()> {
    header(out, inv)?;
    let shown: Vec<_> = inv
        .findings
        .iter()
        .filter(|f| host.is_none_or(|h| f.host.as_deref() == Some(h)))
        .collect();
    if shown.is_empty() {
        writeln!(out, "No finding.")?;
    }
    let mut last = None;
    for f in &shown {
        let sev = f.kind.severity();
        if last != Some(sev) {
            writeln!(out, "── {} ──", sev.name().to_uppercase())?;
            last = Some(sev);
        }
        writeln!(out, "{:<11} {}", f.kind.label(), f.title)?;
        writeln!(out, "{:<11} {}", "", f.detail)?;
        if let Some(h) = &f.hint {
            writeln!(out, "{:<11} → {h}", "")?;
        }
    }
    writeln!(out)?;
    for n in &inv.notes {
        writeln!(out, "note: {n}")?;
    }
    for e in &inv.errors {
        writeln!(out, "could not read: {e}")?;
    }
    writeln!(
        out,
        "{} finding(s). Billfold changes nothing: every action above is yours to take.",
        shown.len()
    )?;
    Ok(())
}

fn wiring(out: &mut impl Write, inv: &Inventory, host: Option<&str>) -> Result<()> {
    header(out, inv)?;
    for h in inv
        .hosts
        .iter()
        .filter(|h| h.present && host.is_none_or(|x| h.id == x))
    {
        writeln!(out, "{} — {}", h.name, h.presence)?;
        let accounts: Vec<_> = inv.accounts.iter().filter(|a| a.host == h.id).collect();
        if accounts.is_empty() {
            writeln!(out, "    no account file read")?;
        }
        for a in accounts {
            writeln!(
                out,
                "    {:<12} {:<22} {:<18} {}{}{}{}",
                a.provider,
                credit(&a.credit),
                a.email_domain
                    .as_deref()
                    .map(|d| format!("@{d}"))
                    .unwrap_or_default(),
                display_home(&a.path, &inv.home),
                a.expires_at
                    .as_ref()
                    .map(|e| format!(
                        "  expires {}{}",
                        &e[..10.min(e.len())],
                        if a.expired { " (past)" } else { "" }
                    ))
                    .unwrap_or_default(),
                if a.legacy { "  legacy" } else { "" },
                a.organization
                    .as_ref()
                    .map(|o| format!("  {o}"))
                    .unwrap_or_default()
            )?;
        }
        if let Some(q) = inv.quotas.iter().find(|q| q.host == h.id) {
            for q in inv.quotas.iter().filter(|q| q.host == h.id) {
                writeln!(
                    out,
                    "    quota {:<9} {:>5.1} % used{}{}  (as the host wrote it{})",
                    q.window,
                    q.used_percent,
                    q.window_minutes
                        .map(|m| format!(" of {m} min"))
                        .unwrap_or_default(),
                    q.resets_at
                        .as_ref()
                        .map(|r| format!(", resets {r}"))
                        .unwrap_or_default(),
                    q.as_of
                        .as_ref()
                        .map(|t| format!(" on {t}"))
                        .unwrap_or_default()
                )?;
            }
            let _ = q;
        }
    }
    Ok(())
}

fn keys(out: &mut impl Write, inv: &Inventory) -> Result<()> {
    header(out, inv)?;
    if inv.keys.is_empty() {
        writeln!(out, "No provider key found in the places Billfold looks.")?;
        return Ok(());
    }
    writeln!(
        out,
        "{:<12} {:<28} {:<24} {:<12} {:<10} where",
        "provider", "name", "value", "git", "mode"
    )?;
    for k in &inv.keys {
        let git = match k.git {
            GitState::Tracked => "TRACKED",
            GitState::Ignored => "ignored",
            GitState::Untracked => "untracked",
            GitState::NotInRepo => "-",
        };
        let place = match k.place {
            KeyPlace::ProcessEnv => "environment".to_string(),
            _ => format!(
                "{}{}",
                display_home(&k.path, &inv.home),
                k.line.map(|l| format!(":{l}")).unwrap_or_default()
            ),
        };
        writeln!(
            out,
            "{:<12} {:<28} {:<24} {:<12} {:<10} {}{}{}",
            k.provider,
            k.name,
            k.shown,
            if k.reference { "reference" } else { git },
            k.mode.as_deref().unwrap_or("-"),
            place,
            if k.exposed { "  EXPOSED" } else { "" },
            if k.used_by.is_empty() {
                String::new()
            } else {
                format!("  read by {}", k.used_by.join(", "))
            }
        )?;
    }
    // shared fingerprints
    let mut by_fp: BTreeMap<&str, usize> = BTreeMap::new();
    for k in inv.keys.iter().filter(|k| !k.fingerprint.is_empty()) {
        *by_fp.entry(&k.fingerprint).or_default() += 1;
    }
    let shared = by_fp.values().filter(|n| **n > 1).count();
    if shared > 0 {
        writeln!(
            out,
            "\n{shared} key(s) appear in more than one place (fingerprints are for this run only)."
        )?;
    }
    Ok(())
}

fn meter(out: &mut impl Write, inv: &Inventory, host: Option<&str>, by: By) -> Result<()> {
    header(out, inv)?;
    // tokens, our dollars, the host's dollars, on a plan, pricing notes
    type Group = (Tokens, Option<f64>, f64, bool, Vec<String>);
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for u in inv
        .usage
        .iter()
        .filter(|u| host.is_none_or(|h| u.host == h))
    {
        let key = match by {
            By::Host => u.host_name.clone(),
            By::Model => format!("{} · {}", u.host_name, u.model),
            By::Project => format!(
                "{} · {}",
                u.host_name,
                u.project
                    .as_ref()
                    .map(|p| display_home(p, &inv.home))
                    .unwrap_or_else(|| "(no project)".into())
            ),
            By::Day => format!("{} · {}", u.day, u.host_name),
        };
        let g = groups
            .entry(key)
            .or_insert((Tokens::default(), None, 0.0, false, Vec::new()));
        g.0.add(&u.tokens);
        if let Some(d) = u.usd {
            g.1 = Some(g.1.unwrap_or(0.0) + d);
        }
        g.2 += u.host_cost_usd.unwrap_or(0.0);
        g.3 |= matches!(u.credit, Credit::Subscription { .. });
        if let Some(n) = &u.pricing_note {
            if !g.4.contains(n) {
                g.4.push(n.clone());
            }
        }
    }
    if groups.is_empty() {
        writeln!(out, "No usage log in the window (since {}).", inv.since)?;
        return Ok(());
    }
    writeln!(
        out,
        "{:<52} {:>7} {:>9} {:>9} {:>9} {:>9} {:>6} {:>12}",
        "", "reqs", "input", "output", "cache w", "cache r", "cache", "API-equiv."
    )?;
    for (k, (t, usd, host_cost, plan, notes)) in &groups {
        writeln!(
            out,
            "{:<52} {:>7} {:>9} {:>9} {:>9} {:>9} {:>5} {:>12}",
            if k.chars().count() > 52 {
                k.chars().take(51).collect::<String>() + "…"
            } else {
                k.clone()
            },
            t.requests,
            human(t.input),
            human(t.output),
            human(t.cache_write),
            human(t.cache_read),
            t.cache_share()
                .map(|s| format!("{s:.0} %"))
                .unwrap_or_else(|| "-".into()),
            match usd {
                Some(d) if *plan => format!("(${d:.2}) plan"),
                Some(d) => format!("${d:.2}"),
                None => "unpriced".into(),
            }
        )?;
        if *host_cost > 0.0 {
            writeln!(
                out,
                "{:<52} {:>7} host's own figure: ${host_cost:.2}",
                "", ""
            )?;
        }
        for n in notes
            .iter()
            .filter(|n| !n.starts_with("included in plan") || n.contains(';'))
        {
            writeln!(out, "{:<52} {:>7} {n}", "", "")?;
        }
    }
    let t = inv.total_tokens();
    writeln!(out)?;
    writeln!(
        out,
        "total {} tokens in {} request(s); ${:.2} API-equivalent for pay-as-you-go lines; subscription lines shown in parentheses are included in a plan",
        human(t.total()),
        t.requests,
        inv.total_usd()
    )?;
    writeln!(
        out,
        "prices: {} snapshot of {} — {} ({})",
        inv.prices.name, inv.prices.as_of, inv.prices.attribution, inv.prices.license
    )?;
    let soon: Vec<_> = inv
        .logs
        .iter()
        .filter(|l| l.days_left.is_some_and(|d| d <= 7))
        .collect();
    if !soon.is_empty() {
        writeln!(
            out,
            "{} log file(s) will be deleted by their host within 7 days.",
            soon.len()
        )?;
    }
    Ok(())
}

fn hosts(out: &mut impl Write, inv: &Inventory) -> Result<()> {
    let sheets = billfold_core::sheets::embedded();
    for h in &inv.hosts {
        writeln!(
            out,
            "{:<28} {:<8} {}",
            h.name,
            if h.present { "present" } else { "absent" },
            h.presence
        )?;
        if let Some(s) = sheets.host(&h.id) {
            let mut urls: Vec<String> = s
                .locations
                .iter()
                .filter_map(|l| l.source.url.clone())
                .collect();
            urls.extend(s.references.iter().cloned());
            urls.sort();
            urls.dedup();
            for u in urls {
                writeln!(out, "{:<28} {:<8} {u}", "", "")?;
            }
            if let Some(k) = &s.ext.keyring {
                writeln!(out, "{:<28} {:<8} secrets: keyring `{}`", "", "", k.service)?;
            }
            if let Some(n) = &s.ext.no_usage_log {
                writeln!(out, "{:<28} {:<8} no usage log: {n}", "", "")?;
            }
        }
    }
    writeln!(out)?;
    for n in &inv.not_read {
        writeln!(out, "not read: {} — {}", n.name, n.reason)?;
    }
    Ok(())
}

fn providers(out: &mut impl Write) -> Result<()> {
    let ps = billfold_core::providers::embedded();
    for p in &ps.list {
        writeln!(
            out,
            "{:<34} {:<40} {}",
            p.name,
            p.env_vars.join(", "),
            p.usage_url.as_deref().unwrap_or("")
        )?;
        if let Some(u) = &p.source.url {
            writeln!(out, "{:<34} {:<40} source: {u}", "", "")?;
        }
        let pr = &p.pricing;
        let mut rules = Vec::new();
        if let Some(w) = pr.cache_write_5m {
            rules.push(format!("cache write ×{w} (5 min)"));
        }
        if let Some(w) = pr.cache_write_1h {
            rules.push(format!("×{w} (1 h)"));
        }
        if !pr.long_context.is_empty() {
            rules.push(format!("{} long-context tier(s)", pr.long_context.len()));
        }
        if pr.off_peak.is_some() {
            rules.push("peak hours ×2".into());
        }
        if !pr.announced.is_empty() {
            rules.push(format!("{} announced change(s)", pr.announced.len()));
        }
        if !rules.is_empty() {
            writeln!(out, "{:<34} {:<40} prices: {}", "", "", rules.join(", "))?;
        }
    }
    Ok(())
}
