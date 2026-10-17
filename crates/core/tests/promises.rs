//! The promises every Blackberrylabs tool keeps, checked on this crate.

use std::path::Path;

use bbl_hostrules::env::{Env, Os};

#[test]
fn the_core_never_writes_and_never_reaches_the_network() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in walk(&src) {
        if entry.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&entry).unwrap();
        let code = text.split("#[cfg(test)]").next().unwrap_or("");
        for forbidden in [
            "fs::write",
            "File::create",
            "fs::remove",
            "fs::rename",
            "OpenOptions",
            "std::net",
            "TcpStream",
            "reqwest",
            "ureq",
            "Command::new",
            "SQLITE_OPEN_READ_WRITE",
            "SQLITE_OPEN_CREATE",
        ] {
            assert!(
                !code.contains(forbidden),
                "{}: mentions {forbidden}",
                entry.display()
            );
        }
    }
}

#[test]
fn the_report_conforms() {
    let tmp = tempfile::tempdir().unwrap();
    let env = Env::fixture(tmp.path(), Os::Linux);
    let inv = billfold_core::scan(&env);
    let report = billfold_core::report::build(&inv, 1);
    let json = report.to_json();
    let back = bbl_report::conformance::check(&json).expect("the report conforms to bbl-report/1");
    assert_eq!(back.module.id, "billfold");
    assert_eq!(back.module.verb, "tally");
    assert_eq!(back.findings.len(), inv.findings.len());
    for f in &back.findings {
        assert_eq!(f.open.as_ref().unwrap().app, "billfold-app");
    }
}

/// A planted key must never appear in clear anywhere Billfold writes or returns, and a
/// scan must leave the file tree exactly as it was.
#[test]
fn a_planted_key_never_leaves_the_machine_and_nothing_is_written() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    // a home directory is private (0700/0750): a 644 file inside it is not exposed
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(home, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let secret = format!("sk-ant-api03-{}AA", "Q".repeat(93));
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(
        home.join(".bashrc"),
        format!("export ANTHROPIC_API_KEY={secret}\n"),
    )
    .unwrap();
    std::fs::write(home.join(".claude/.credentials.json"), r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-SECRETSECRETSECRETSECRET","refreshToken":"r","expiresAt":1890000000000,"subscriptionType":"max","rateLimitTier":"default_claude_max_5x"}}"#).unwrap();
    let before = walk(home);
    let env = Env::fixture(home, Os::Linux);
    let inv = billfold_core::scan(&env);
    let json = serde_json::to_string(&inv).unwrap();
    assert!(
        !json.contains(&secret),
        "the planted key appears in the inventory"
    );
    assert!(!json.contains("QQQQQQQQ"));
    assert!(!json.contains("SECRETSECRET"));
    let report = billfold_core::report::build(&inv, 1).to_json();
    assert!(!report.contains("QQQQQQQQ") && !report.contains("SECRETSECRET"));
    assert_eq!(walk(home), before);
    // and the facts are there
    assert!(inv.keys.iter().any(|k| k.provider == "anthropic"
        && k.shown.starts_with("sk-ant-api03-…")
        && k.shown.ends_with("QQAA")));
    assert!(inv
        .findings
        .iter()
        .any(|f| f.kind == billfold_core::Kind::ClearSecret));
    assert!(
        inv.findings
            .iter()
            .any(|f| f.kind == billfold_core::Kind::DoubleCredit),
        "{:?}",
        inv.findings.iter().map(|f| &f.id).collect::<Vec<_>>()
    );
    let claude = inv
        .accounts
        .iter()
        .find(|a| a.host == "claude-code")
        .expect("a Claude Code account");
    assert_eq!(
        claude.credit,
        billfold_core::model::Credit::Subscription {
            plan: Some("max".into())
        }
    );
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}
