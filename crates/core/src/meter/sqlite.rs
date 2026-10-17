//! SQLite stores, read-only and immutable: OpenCode and Kilo (`session_message` /
//! `message` rows with the OpenCode data shape), Goose (`usage_ledger`), Zed (`threads`
//! with JSON or zstd blobs).

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{in_window, str_at, u64_at, Read, Request};
use crate::model::{Billing, Credit, Tokens};
use crate::util::open_ro;

fn ms(v: Option<i64>) -> Option<DateTime<Utc>> {
    v.and_then(|n| {
        if n > 100_000_000_000 {
            DateTime::<Utc>::from_timestamp_millis(n)
        } else {
            DateTime::<Utc>::from_timestamp(n, 0)
        }
    })
}

/// OpenCode / Kilo: assistant messages with `modelID`, `providerID`, `cost`, `tokens`.
pub fn read_opencode(files: &[PathBuf], since: DateTime<Utc>) -> Read {
    let mut out = Read::default();
    for f in files {
        match read_opencode_one(f, since) {
            Ok((reqs, n)) => {
                out.files.push((f.clone(), n));
                out.requests.extend(reqs);
            }
            Err(e) => out.errors.push(format!("{}: {e}", f.display())),
        }
    }
    out
}

fn read_opencode_one(f: &Path, since: DateTime<Utc>) -> Result<(Vec<Request>, u64), String> {
    let conn = open_ro(f)?;
    // the messages table is `session_message` (OpenCode) or `message` (Kilo)
    let table = ["session_message", "message"]
        .into_iter()
        .find(|t| conn.prepare(&format!("SELECT 1 FROM {t} LIMIT 1")).is_ok())
        .ok_or("no message table")?;
    // sessions: directory and fork boundary when the columns exist
    let mut dirs: std::collections::HashMap<String, (Option<String>, Option<i64>)> =
        Default::default();
    for st in ["session_v2", "session"] {
        if let Ok(mut s) = conn.prepare(&format!("SELECT id, directory, fork_boundary FROM {st}")) {
            if let Ok(rows) = s.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1).ok().flatten(),
                    r.get::<_, Option<i64>>(2).ok().flatten(),
                ))
            }) {
                for (id, d, b) in rows.flatten() {
                    dirs.insert(id, (d, b));
                }
            }
            break;
        } else if let Ok(mut s) = conn.prepare(&format!("SELECT id, directory FROM {st}")) {
            if let Ok(rows) = s.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1).ok().flatten(),
                ))
            }) {
                for (id, d) in rows.flatten() {
                    dirs.insert(id, (d, None));
                }
            }
            break;
        }
    }
    let has_seq = conn
        .prepare(&format!("SELECT seq FROM {table} LIMIT 1"))
        .is_ok();
    let sql = if has_seq {
        format!("SELECT session_id, time_created, data, seq FROM {table}")
    } else {
        format!("SELECT session_id, time_created, data, 0 FROM {table}")
    };
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<i64>>(1).ok().flatten(),
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3).unwrap_or(0),
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut reqs = Vec::new();
    let mut n = 0u64;
    for (session, created, data, seq) in rows.flatten() {
        let Ok(v) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        if v.get("role").and_then(Value::as_str).unwrap_or("assistant") != "assistant" {
            continue;
        }
        let Some(t) = v.get("tokens").filter(|x| x.is_object()) else {
            continue;
        };
        let (dir, boundary) = dirs.get(&session).cloned().unwrap_or((None, None));
        if let Some(b) = boundary {
            if seq <= b {
                continue; // copied from the parent session by a fork
            }
        }
        let when = ms(v
            .get("time")
            .and_then(|x| x.get("created"))
            .and_then(Value::as_i64))
        .or_else(|| ms(created));
        if !in_window(when, since) {
            continue;
        }
        let provider = str_at(&v, "providerID");
        let cost = v.get("cost").and_then(Value::as_f64);
        n += 1;
        reqs.push(Request {
            model: str_at(&v, "modelID").unwrap_or_else(|| "unknown".into()),
            when,
            tokens: Tokens {
                input: u64_at(t, "input"),
                output: u64_at(t, "output"),
                cache_write: u64_at(t, "cache.write"),
                cache_read: u64_at(t, "cache.read"),
                reasoning: u64_at(t, "reasoning"),
                requests: 1,
            },
            billing: Billing::default(),
            project: dir.map(PathBuf::from),
            host_cost_usd: cost,
            credit_hint: match (provider.as_deref(), cost) {
                // the ChatGPT-plan plugin writes no cost: tokens are included in the plan
                (Some("openai"), Some(c))
                    if c == 0.0 && t.get("input").and_then(Value::as_u64).unwrap_or(0) > 0 =>
                {
                    Some(Credit::Subscription { plan: None })
                }
                _ => None,
            },
            provider_hint: provider.map(|p| crate::wiring::normalise_provider(&p)),
            source: f.to_path_buf(),
        });
    }
    Ok((reqs, n))
}

/// Goose: `usage_ledger` rows, with the session's provider and working directory.
pub fn read_goose(files: &[PathBuf], since: DateTime<Utc>) -> Read {
    let mut out = Read::default();
    for f in files {
        match read_goose_one(f, since) {
            Ok((reqs, n)) => {
                out.files.push((f.clone(), n));
                out.requests.extend(reqs);
            }
            Err(e) => out.errors.push(format!("{}: {e}", f.display())),
        }
    }
    out
}

fn read_goose_one(f: &Path, since: DateTime<Utc>) -> Result<(Vec<Request>, u64), String> {
    let conn = open_ro(f)?;
    let with_join = "SELECT u.model, u.input_tokens, u.output_tokens, u.cache_read_tokens, u.cache_write_tokens, u.cost, u.cost_source, u.is_compaction, u.created_timestamp, s.provider_name, s.working_dir FROM usage_ledger u LEFT JOIN sessions s ON s.id = u.session_id";
    let without = "SELECT model, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, cost, cost_source, is_compaction, created_timestamp, NULL, NULL FROM usage_ledger";
    let mut stmt = conn
        .prepare(with_join)
        .or_else(|_| conn.prepare(without))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<f64>>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<i64>>(7)?,
                r.get::<_, Option<String>>(8)?,
                r.get::<_, Option<String>>(9)?,
                r.get::<_, Option<String>>(10)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut reqs = Vec::new();
    let mut n = 0u64;
    for row in rows.flatten() {
        let (model, i, o, cr, cw, cost, _source, _compaction, ts, provider, dir) = row;
        let when = ts.as_deref().and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|t| t.with_timezone(&Utc))
                .or_else(|| {
                    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                        .ok()
                        .map(|nd| DateTime::<Utc>::from_naive_utc_and_offset(nd, Utc))
                })
        });
        if !in_window(when, since) {
            continue;
        }
        n += 1;
        reqs.push(Request {
            model: model.unwrap_or_else(|| "unknown".into()),
            when,
            tokens: Tokens {
                input: i.unwrap_or(0).max(0) as u64,
                output: o.unwrap_or(0).max(0) as u64,
                cache_write: cw.unwrap_or(0).max(0) as u64,
                cache_read: cr.unwrap_or(0).max(0) as u64,
                reasoning: 0,
                requests: 1,
            },
            billing: Billing::default(),
            project: dir.map(PathBuf::from),
            host_cost_usd: cost,
            provider_hint: provider.map(|p| crate::wiring::normalise_provider(&p)),
            credit_hint: None,
            source: f.to_path_buf(),
        });
    }
    Ok((reqs, n))
}

/// Zed: one row per thread, a JSON (or zstd) blob with per-request usage and the model.
pub fn read_zed(files: &[PathBuf], since: DateTime<Utc>) -> Read {
    let mut out = Read::default();
    for f in files {
        match read_zed_one(f, since) {
            Ok((reqs, n)) => {
                out.files.push((f.clone(), n));
                out.requests.extend(reqs);
            }
            Err(e) => out.errors.push(format!("{}: {e}", f.display())),
        }
    }
    out
}

fn read_zed_one(f: &Path, since: DateTime<Utc>) -> Result<(Vec<Request>, u64), String> {
    let conn = open_ro(f)?;
    let mut stmt = conn
        .prepare("SELECT id, updated_at, data_type, data FROM threads")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Vec<u8>>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut reqs = Vec::new();
    let mut n = 0u64;
    for (_id, updated, dt, data) in rows.flatten() {
        let bytes = if dt.as_deref() == Some("zstd") {
            zstd::decode_all(&data[..]).map_err(|e| e.to_string())?
        } else {
            data
        };
        let Ok(v) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        let when = updated.as_deref().and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|t| t.with_timezone(&Utc))
                .or_else(|| {
                    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f")
                        .ok()
                        .map(|nd| DateTime::<Utc>::from_naive_utc_and_offset(nd, Utc))
                })
        });
        if !in_window(when, since) {
            continue;
        }
        let provider = str_at(&v, "model.provider");
        let model = str_at(&v, "model.model").unwrap_or_else(|| "unknown".into());
        let credit = provider
            .as_deref()
            .filter(|p| *p == "zed.dev")
            .map(|_| Credit::Subscription { plan: None });
        let Some(Value::Object(per)) = v.get("request_token_usage") else {
            continue;
        };
        for (_mid, u) in per {
            n += 1;
            reqs.push(Request {
                model: model.clone(),
                when,
                tokens: Tokens {
                    input: u64_at(u, "input_tokens"),
                    output: u64_at(u, "output_tokens"),
                    cache_write: u64_at(u, "cache_creation_input_tokens"),
                    cache_read: u64_at(u, "cache_read_input_tokens"),
                    reasoning: 0,
                    requests: 1,
                },
                billing: Billing::default(),
                project: None,
                host_cost_usd: None,
                provider_hint: provider.as_deref().map(|p| {
                    if p == "zed.dev" {
                        "zed".to_string()
                    } else {
                        crate::wiring::normalise_provider(p)
                    }
                }),
                credit_hint: credit.clone(),
                source: f.to_path_buf(),
            });
        }
    }
    Ok((reqs, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_rows_and_forks() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("opencode.db");
        let c = rusqlite::Connection::open(&f).unwrap();
        c.execute_batch(
            "CREATE TABLE session_v2(id TEXT, directory TEXT, fork_boundary INTEGER);
             CREATE TABLE session_message(id TEXT, session_id TEXT, type TEXT, seq INTEGER, time_created INTEGER, data TEXT);
             INSERT INTO session_v2 VALUES ('s1','/home/me/p',NULL),('s2','/home/me/p',1);
             INSERT INTO session_message VALUES ('m1','s1','assistant',1,1792281600000,'{\"role\":\"assistant\",\"modelID\":\"claude-sonnet-5-5\",\"providerID\":\"anthropic\",\"cost\":0.01,\"tokens\":{\"input\":100,\"output\":10,\"reasoning\":0,\"cache\":{\"read\":50,\"write\":20}},\"time\":{\"created\":1792281600000}}');
             INSERT INTO session_message VALUES ('m2','s2','assistant',1,1792281601000,'{\"role\":\"assistant\",\"modelID\":\"claude-sonnet-5-5\",\"providerID\":\"anthropic\",\"cost\":0.01,\"tokens\":{\"input\":100,\"output\":10,\"cache\":{\"read\":0,\"write\":0}}}');
             INSERT INTO session_message VALUES ('m3','s2','assistant',2,1792281602000,'{\"role\":\"assistant\",\"modelID\":\"gpt-6.1-sol\",\"providerID\":\"openai\",\"cost\":0,\"tokens\":{\"input\":10,\"output\":1,\"cache\":{\"read\":0,\"write\":0}}}');",
        )
        .unwrap();
        drop(c);
        let r = read_opencode(&[f], "2026-10-01T00:00:00Z".parse().unwrap());
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(r.requests.len(), 2, "the fork copy is skipped");
        assert_eq!(r.requests[0].tokens.cache_write, 20);
        assert_eq!(
            r.requests[0].project.as_deref(),
            Some(Path::new("/home/me/p"))
        );
        assert_eq!(
            r.requests[1].credit_hint,
            Some(Credit::Subscription { plan: None })
        );
    }

    #[test]
    fn zed_blobs_plain_and_zstd() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("threads.db");
        let c = rusqlite::Connection::open(&f).unwrap();
        c.execute_batch("CREATE TABLE threads(id TEXT, summary TEXT, updated_at TEXT, data_type TEXT, data BLOB);").unwrap();
        let json = r#"{"model":{"provider":"zed.dev","model":"claude-sonnet-5-5"},"request_token_usage":{"u1":{"input_tokens":5,"output_tokens":6,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#;
        let z = zstd::encode_all(json.as_bytes(), 3).unwrap();
        c.execute(
            "INSERT INTO threads VALUES ('t1','x','2026-10-17T10:00:00Z','json',?1)",
            [json.as_bytes()],
        )
        .unwrap();
        c.execute(
            "INSERT INTO threads VALUES ('t2','y','2026-10-17T11:00:00Z','zstd',?1)",
            [z],
        )
        .unwrap();
        drop(c);
        let r = read_zed(&[f], "2026-10-01T00:00:00Z".parse().unwrap());
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(r.requests.len(), 2);
        assert_eq!(
            r.requests[0].credit_hint,
            Some(Credit::Subscription { plan: None })
        );
    }
}
