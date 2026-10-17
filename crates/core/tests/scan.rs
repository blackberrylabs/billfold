//! A fake home with a little of every host: the scan must find each kind of finding once,
//! count tokens right, and price them from the embedded snapshot.

use std::path::Path;
use std::time::{Duration, SystemTime};

use bbl_hostrules::env::{Env, Os};
use billfold_core::model::{Credit, GitState, Kind};
use billfold_core::{scan_with, Options};

fn write(p: &Path, text: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn chmod(p: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
    }
}

fn age(p: &Path, days: u64) {
    let t = SystemTime::now() - Duration::from_secs(days * 86400);
    let f = std::fs::File::options().write(true).open(p).unwrap();
    f.set_modified(t).unwrap();
}

fn fixture(home: &Path) {
    chmod(home, 0o700);
    let now = chrono::Utc::now();
    let ts = |h: i64| {
        (now - chrono::Duration::hours(h)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    };
    // --- Claude Code: Max plan, a session with two blocks of one message + a subagent replay
    write(
        &home.join(".claude/.credentials.json"),
        r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-SECRETSECRETSECRETSECRETSECRET","refreshToken":"sk-ant-ort01-SECRET","expiresAt":1890000000000,"scopes":["user:inference"],"subscriptionType":"max","rateLimitTier":"default_claude_max_5x"}}"#,
    );
    chmod(&home.join(".claude/.credentials.json"), 0o600);
    write(
        &home.join(".claude.json"),
        &format!(
            r#"{{"oauthAccount":{{"emailAddress":"someone@example.org","organizationName":"Example Org","billingType":"stripe_subscription"}},"projects":{{"{}":{{}}}},"mcpServers":{{"brave":{{"command":"npx","env":{{"BRAVE_API_KEY":"BSA_0123456789abcdefghijklmnopqrstuv","OPENAI_API_KEY":"${{OPENAI_API_KEY}}"}}}}}}}}"#,
            home.join("proj").display()
        ),
    );
    let claude_line = |block: u32, mid: &str, rid: &str, side: bool, model: &str| {
        format!(
            r#"{{"type":"assistant","isSidechain":{side},"timestamp":"{}","cwd":"{}","requestId":"{rid}","apiBlockIndex":{block},"message":{{"id":"{mid}","model":"{model}","usage":{{"input_tokens":1000,"output_tokens":200,"cache_creation_input_tokens":3000,"cache_read_input_tokens":20000,"cache_creation":{{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":3000}},"output_tokens_details":{{"thinking_tokens":50}},"service_tier":"standard","inference_geo":"global","speed":"standard"}}}}}}"#,
            ts(5),
            home.join("proj").display()
        )
    };
    write(
        &home.join(".claude/projects/-home-me-proj/s1.jsonl"),
        &[
            claude_line(0, "msg_a", "req_a", false, "claude-sonnet-5-5"),
            claude_line(1, "msg_a", "req_a", false, "claude-sonnet-5-5"),
            claude_line(0, "msg_b", "req_b", false, "claude-sonnet-5-5"),
            claude_line(0, "msg_c", "req_c", false, "claude-mystery-9"),
        ]
        .join("\n"),
    );
    write(
        &home.join(".claude/projects/-home-me-proj/s1/subagents/agent-1.jsonl"),
        &claude_line(0, "msg_b", "req_x", true, "claude-sonnet-5-5"),
    );
    // an old session about to be pruned (retention 30 days, 25 days old)
    write(
        &home.join(".claude/projects/-home-me-old/s0.jsonl"),
        &claude_line(0, "msg_old", "req_old", false, "claude-sonnet-5-5")
            .replace(&ts(5), &ts(25 * 24)),
    );
    age(&home.join(".claude/projects/-home-me-old/s0.jsonl"), 25);
    // --- Codex: plus plan, API key also in the shell → double credit; quota at 91 %
    let id_token = {
        use base64::Engine;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"email":"someone@example.org","https://api.openai.com/auth":{"chatgpt_plan_type":"plus","chatgpt_account_id":"acc_1"},"exp":1890000000}"#);
        format!("eyJhbGciOiJSUzI1NiJ9.{payload}.SIGNATURE")
    };
    write(
        &home.join(".codex/auth.json"),
        &format!(
            r#"{{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{{"id_token":"{id_token}","access_token":"eyJ.ACCESS.X","refresh_token":"rt_SECRET","account_id":"acc_1"}},"last_refresh":"2026-10-17T08:00:00Z"}}"#
        ),
    );
    chmod(&home.join(".codex/auth.json"), 0o600);
    write(
        &home.join(".codex/sessions/2026/10/17/rollout-2026-10-17T09-00-00-abc.jsonl"),
        &[
            format!(r#"{{"timestamp":"{}","type":"session_meta","payload":{{"id":"abc","cwd":"{}","creator_account_id":"acc_1"}}}}"#, ts(3), home.join("proj").display()),
            format!(r#"{{"timestamp":"{}","type":"turn_context","payload":{{"model":"gpt-5.3-codex"}}}}"#, ts(3)),
            format!(r#"{{"timestamp":"{}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":5000,"cached_input_tokens":4000,"cache_write_input_tokens":0,"output_tokens":300,"reasoning_output_tokens":100,"total_tokens":5300}},"last_token_usage":{{"input_tokens":5000,"cached_input_tokens":4000,"cache_write_input_tokens":0,"output_tokens":300,"reasoning_output_tokens":100,"total_tokens":5300}}}},"rate_limits":{{"primary":{{"used_percent":91.0,"window_minutes":300,"resets_at":1890000000}},"secondary":{{"used_percent":40.0,"window_minutes":10080,"resets_at":1890000000}},"plan_type":"plus"}}}}}}"#, ts(3)),
        ]
        .join("\n"),
    );
    write(&home.join(".bashrc"), &format!("export OPENAI_API_KEY=sk-proj-{}T3BlbkFJ{}\nexport EDITOR=vim\nexport GROQ_API_KEY=gsk_0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOP\n", "a".repeat(74), "b".repeat(74)));
    // --- a project with a tracked .env holding the same Groq key, and an ignored .env.local
    let proj = home.join("proj");
    std::fs::create_dir_all(proj.join(".git")).unwrap();
    write(&proj.join(".gitignore"), ".env.local\n");
    write(&proj.join(".env"), "GROQ_API_KEY=gsk_0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOP\nDATABASE_URL=postgres://x\n");
    write(
        &proj.join(".env.local"),
        &format!("DEEPSEEK_API_KEY=sk-{0}{0}\n", "0123456789abcdef"),
    );
    let mut idx = Vec::new();
    idx.extend_from_slice(b"DIRC");
    idx.extend_from_slice(&2u32.to_be_bytes());
    idx.extend_from_slice(&1u32.to_be_bytes());
    idx.extend_from_slice(&[0u8; 60]);
    idx.extend_from_slice(&(4u16).to_be_bytes());
    idx.extend_from_slice(b".env");
    idx.push(0);
    std::fs::write(proj.join(".git/index"), idx).unwrap();
    // --- OpenCode: legacy auth.json still there, current DB with an expired oauth row
    write(
        &home.join(".local/share/opencode/auth.json"),
        r#"{"anthropic":{"type":"api","key":"sk-ant-api03-LEGACYSECRET"}}"#,
    );
    {
        let db = home.join(".local/share/opencode/opencode.db");
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute_batch(
            "CREATE TABLE credential(id TEXT, integration_id TEXT, label TEXT, value TEXT, connector_id TEXT, method_id TEXT, active INTEGER);
             INSERT INTO credential VALUES ('1','openai','ChatGPT','{\"type\":\"oauth\",\"access\":\"SECRET\",\"refresh\":\"SECRET\",\"expires\":1600000000000}',NULL,NULL,1);
             INSERT INTO credential VALUES ('2','deepseek','key','{\"type\":\"key\",\"key\":\"sk-SECRET\"}',NULL,NULL,1);
             CREATE TABLE session_v2(id TEXT, directory TEXT, fork_boundary INTEGER);
             CREATE TABLE session_message(id TEXT, session_id TEXT, type TEXT, seq INTEGER, time_created INTEGER, data TEXT);",
        )
        .unwrap();
        let t = chrono::Utc::now().timestamp_millis() - 3_600_000;
        c.execute(
            "INSERT INTO session_v2 VALUES ('s1', ?1, NULL)",
            [proj.display().to_string()],
        )
        .unwrap();
        c.execute(
            "INSERT INTO session_message VALUES ('m1','s1','assistant',1,?1,?2)",
            rusqlite::params![t, format!(r#"{{"role":"assistant","modelID":"deepseek-v4-pro","providerID":"deepseek","cost":9.99,"tokens":{{"input":1000000,"output":100000,"reasoning":0,"cache":{{"read":0,"write":0}}}},"time":{{"created":{t}}}}}"#)],
        )
        .unwrap();
    }
    write(
        &home.join(".config/opencode/opencode.json"),
        r#"{"model":"anthropic/claude-sonnet-5-5"}"#,
    );
    // --- Goose: config present, secrets in the keyring (no secrets.yaml)
    write(&home.join(".config/goose/config.yaml"), "active_provider: anthropic\nproviders:\n  anthropic:\n    enabled: true\n    model: claude-sonnet-5-5\n");
    // --- Cursor present, no usage log
    write(&home.join(".config/Cursor/User/settings.json"), "{}");
}

#[test]
fn the_fixture_home_yields_every_kind_once_and_counts_right() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    fixture(home);
    let env = Env::fixture(home, Os::Linux);
    let inv = scan_with(
        &env,
        &billfold_core::sheets::embedded(),
        &billfold_core::providers::embedded(),
        billfold_core::prices::PriceTable::embedded(),
        &Options::default(),
    );
    let kinds: Vec<Kind> = inv.findings.iter().map(|f| f.kind).collect();
    let ids: Vec<&String> = inv.findings.iter().map(|f| &f.id).collect();
    // presence
    let present: Vec<&str> = inv
        .hosts
        .iter()
        .filter(|h| h.present)
        .map(|h| h.id.as_str())
        .collect();
    for h in ["claude-code", "codex", "opencode", "goose", "cursor"] {
        assert!(present.contains(&h), "{h} not present: {present:?}");
    }
    // accounts
    let claude = inv
        .accounts
        .iter()
        .find(|a| a.host == "claude-code" && !a.legacy)
        .unwrap();
    assert_eq!(
        claude.credit,
        Credit::Subscription {
            plan: Some("max".into())
        }
    );
    assert_eq!(claude.email_domain.as_deref(), Some("example.org"));
    assert_eq!(claude.organization.as_deref(), Some("Example Org"));
    let codex = inv.accounts.iter().find(|a| a.host == "codex").unwrap();
    assert_eq!(
        codex.credit,
        Credit::Subscription {
            plan: Some("plus".into())
        }
    );
    assert_eq!(codex.email_domain.as_deref(), Some("example.org"));
    let oc_openai = inv
        .accounts
        .iter()
        .find(|a| a.host == "opencode" && a.provider == "openai" && !a.legacy)
        .unwrap_or_else(|| {
            panic!(
                "{:?} {:?}",
                inv.errors,
                inv.accounts
                    .iter()
                    .map(|a| (&a.host, &a.provider, &a.credit, a.legacy, &a.note))
                    .collect::<Vec<_>>()
            )
        });
    assert!(oc_openai.expired);
    assert!(inv
        .accounts
        .iter()
        .any(|a| a.host == "goose" && a.credit == Credit::Keyring));
    // keys: masked, fingerprinted, git-aware; the reference is listed but not a key
    let json = serde_json::to_string(&inv).unwrap();
    assert!(!json.contains("SECRET"), "a secret leaked");
    assert!(!json.contains("T3BlbkFJ"));
    let groq: Vec<_> = inv.keys.iter().filter(|k| k.provider == "groq").collect();
    assert_eq!(
        groq.len(),
        2,
        "{:?}",
        inv.keys
            .iter()
            .map(|k| (&k.name, &k.path))
            .collect::<Vec<_>>()
    );
    assert_eq!(groq[0].fingerprint, groq[1].fingerprint);
    assert!(groq.iter().any(|k| k.git == GitState::Tracked));
    assert!(inv
        .keys
        .iter()
        .any(|k| k.provider == "deepseek" && k.git == GitState::Ignored));
    assert!(inv
        .keys
        .iter()
        .any(|k| k.reference && k.name == "OPENAI_API_KEY"));
    assert!(!inv
        .keys
        .iter()
        .any(|k| k.name == "EDITOR" || k.name == "DATABASE_URL"));
    // findings, one of each expected kind
    for k in [
        Kind::TrackedSecret,
        Kind::ClearSecret,
        Kind::SharedKey,
        Kind::Expired,
        Kind::DoubleCredit,
        Kind::Unpriced,
        Kind::PruningSoon,
        Kind::HostDisagrees,
        Kind::NearLimit,
        Kind::LegacyStore,
        Kind::NoUsageLog,
        Kind::InKeyring,
    ] {
        assert!(kinds.contains(&k), "missing {k:?} in {ids:?}");
    }
    assert!(!kinds.contains(&Kind::ExposedSecret), "{ids:?}");
    // ids are stable and home-relative
    assert!(
        ids.iter()
            .any(|i| i.as_str() == "tracked-secret/groq/proj/.env:1"),
        "{ids:?}"
    );
    assert!(ids
        .iter()
        .any(|i| i.as_str() == "double-credit/codex/openai"));
    // tokens: Claude Code 2 requests in the window from s1 (two blocks = one, the replay dropped) + the old one
    let cc = inv.hosts.iter().find(|h| h.id == "claude-code").unwrap();
    assert_eq!(
        cc.requests,
        4,
        "{:?}",
        inv.usage
            .iter()
            .map(|u| (&u.host, &u.model, u.tokens.requests))
            .collect::<Vec<_>>()
    );
    assert_eq!(cc.tokens.cache_read, 80_000);
    // the Max plan line is "included in plan", priced as API-equivalent, excluded from the total
    let sonnet = inv
        .usage
        .iter()
        .find(|u| {
            u.host == "claude-code"
                && u.model == "claude-sonnet-5-5"
                && u.project.as_deref() == Some(home.join("proj").as_path())
        })
        .unwrap();
    assert!(matches!(sonnet.credit, Credit::Subscription { .. }));
    assert!(sonnet.usd.unwrap() > 0.0);
    assert!(sonnet
        .pricing_note
        .as_deref()
        .unwrap()
        .contains("included in plan"));
    // DeepSeek on OpenCode is pay-as-you-go: 1M in + 100k out at off-peak 0.66 / 1.98 (×2 at peak)
    let ds = inv.usage.iter().find(|u| u.host == "opencode").unwrap();
    assert_eq!(ds.credit, Credit::ApiKey);
    let usd = ds.usd.unwrap();
    assert!(
        (usd - 0.858).abs() < 1e-6 || (usd - 1.716).abs() < 1e-6,
        "{usd}"
    );
    assert_eq!(ds.host_cost_usd, Some(9.99));
    assert!(
        (inv.total_usd() - usd).abs() < 1e-9,
        "only pay-as-you-go lines add up: {}",
        inv.total_usd()
    );
    // quota
    assert_eq!(inv.quotas.len(), 2);
    assert_eq!(inv.quotas[0].used_percent, 91.0);
    // the report conforms and carries no secret
    let report = billfold_core::report::build(&inv, 1).to_json();
    assert!(bbl_report::conformance::check(&report).is_ok());
    assert!(!report.contains("SECRET"));
}

/// `BILLFOLD_FIXTURE_OUT=/some/dir cargo test -p billfold-core --test scan write_fixture`
/// writes the fixture home there, for looking at the CLI and the app by hand.
#[test]
fn write_fixture() {
    if let Ok(dir) = std::env::var("BILLFOLD_FIXTURE_OUT") {
        let p = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&p).unwrap();
        fixture(&p);
    }
}
