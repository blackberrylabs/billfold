//! The `bbl-report/1` view of an inventory, for the Blackberrylabs control tower. Nothing
//! here is new: only what the CLI and the app already show, secrets masked upstream.

use bbl_report::{
    Action, ActionKind, Evidence, Finding, Module, Open, Report, Subject, SubjectKind, Summary,
    Tone,
};

use crate::model::Inventory;

/// Binary name of the app, as packaged.
pub const APP: &str = "billfold-app";
/// Binary name of the command line, as packaged.
pub const CLI: &str = "billfold";

pub fn module() -> Module {
    Module {
        id: "billfold".into(),
        name: "Billfold".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        verb: "tally".into(),
        tagline: "What this machine spends on AI — the accounts it is wired to, the keys it holds, the tokens it burned.".into(),
        app: Some(APP.into()),
        cli: Some(CLI.into()),
        home: "https://github.com/blackberrylabs/billfold".into(),
    }
}

/// `1.2 M`, `340 k`, `12`.
pub fn human(n: u64) -> String {
    if n >= 10_000_000 {
        format!("{:.0} M", n as f64 / 1e6)
    } else if n >= 1_000_000 {
        format!("{:.1} M", n as f64 / 1e6)
    } else if n >= 10_000 {
        format!("{:.0} k", n as f64 / 1e3)
    } else if n >= 1_000 {
        format!("{:.1} k", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

/// Build the report. `duration_ms` is the time the scan took.
pub fn build(inv: &Inventory, duration_ms: u64) -> Report {
    let mut report = Report::new(module());
    report.duration_ms = duration_ms;
    let n = inv.findings.len();
    let act = inv
        .findings
        .iter()
        .filter(|f| f.kind.severity() == bbl_report::Severity::ActNow)
        .count();
    let tokens = inv.total_tokens();
    let usd = inv.total_usd();
    let keys = inv.keys.iter().filter(|k| !k.reference).count();
    report.summary = vec![
        Summary {
            key: "keys".into(),
            label: "Keys found".into(),
            value: keys.to_string(),
            tone: if inv
                .keys
                .iter()
                .any(|k| !k.reference && k.git == crate::model::GitState::Tracked)
            {
                Tone::Alert
            } else {
                Tone::Neutral
            },
        },
        Summary {
            key: "tokens_30d".into(),
            label: format!("Tokens since {}", inv.since),
            value: human(tokens.total()),
            tone: Tone::Neutral,
        },
        Summary {
            key: "usd_30d".into(),
            label: format!(
                "API-equivalent, pay-as-you-go ({})",
                inv.prices.as_of.get(..10).unwrap_or(&inv.prices.as_of)
            ),
            value: format!("${usd:.2}"),
            tone: Tone::Neutral,
        },
        Summary {
            key: "findings".into(),
            label: "Findings".into(),
            value: n.to_string(),
            tone: if act > 0 {
                Tone::Alert
            } else if n == 0 {
                Tone::Good
            } else {
                Tone::Neutral
            },
        },
    ];
    for f in &inv.findings {
        let mut actions = Vec::new();
        if let Some(h) = &f.hint {
            actions.push(Action {
                kind: ActionKind::Hint,
                label: h.clone(),
                target: None,
            });
        }
        if let Some(p) = &f.path {
            if !p.to_string_lossy().starts_with("keyring:")
                && !p.to_string_lossy().starts_with("env:")
            {
                let target = match f.line {
                    Some(l) => format!("{}:{l}", p.display()),
                    None => p.display().to_string(),
                };
                actions.push(Action {
                    kind: ActionKind::OpenFile,
                    label: "Open file".into(),
                    target: Some(target),
                });
            }
        }
        if let Some(c) = &f.command {
            actions.push(Action {
                kind: ActionKind::Command,
                label: format!("Check in the host: {c}"),
                target: Some(c.clone()),
            });
        }
        if let Some(u) = &f.url {
            actions.push(Action {
                kind: ActionKind::OpenUrl,
                label: "Provider's usage page".into(),
                target: Some(u.clone()),
            });
        }
        if let Some(u) = &f.source_url {
            actions.push(Action {
                kind: ActionKind::OpenUrl,
                label: "Source of this rule".into(),
                target: Some(u.clone()),
            });
        }
        report.findings.push(Finding {
            id: f.id.clone(),
            severity: f.kind.severity(),
            kind: f.kind.slug().into(),
            title: f.title.clone(),
            subject: Subject {
                kind: if f.host.is_some() {
                    SubjectKind::Host
                } else {
                    SubjectKind::Other
                },
                label: f.subject.clone(),
            },
            detail: f.detail.clone(),
            evidence: f
                .path
                .iter()
                .map(|p| Evidence {
                    label: p.display().to_string(),
                    path: Some(p.display().to_string()),
                    line: f.line.map(|l| l as u32),
                })
                .collect(),
            actions,
            open: Some(Open {
                app: APP.into(),
                args: vec!["--select".into(), f.id.clone()],
            }),
        });
    }
    let present: Vec<&str> = inv
        .hosts
        .iter()
        .filter(|h| h.present)
        .map(|h| h.name.as_str())
        .collect();
    report.notes.push(format!(
        "hosts present: {}",
        if present.is_empty() {
            "none".into()
        } else {
            present.join(", ")
        }
    ));
    report.notes.push(format!(
        "{} provider(s) wired, {} account(s), {} key(s), {} usage line(s), {} log file(s)",
        inv.providers().len(),
        inv.accounts.len(),
        keys,
        inv.usage.len(),
        inv.logs.len()
    ));
    report.notes.extend(inv.notes.iter().cloned());
    report
}
