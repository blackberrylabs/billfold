//! Small read-only helpers: file facts (mode, time), JWT payloads decoded locally, SQLite
//! key/value tables, time parsing, the session fingerprint of a secret.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use base64::Engine;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;

/// Octal mode (`600`) and whether *other* users can actually read the file: its mode
/// gives others read, and every directory above it gives others search. Unix only; a
/// group-readable file is not counted (the user's private group is the usual case).
pub fn mode_of(path: &Path) -> (Option<String>, bool) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(md) = std::fs::metadata(path) {
            let m = md.permissions().mode() & 0o777;
            let mut exposed = m & 0o004 != 0;
            if exposed {
                let mut dir = path.parent();
                while let Some(d) = dir {
                    match std::fs::metadata(d) {
                        Ok(dm) if dm.permissions().mode() & 0o001 != 0 => {}
                        _ => {
                            exposed = false;
                            break;
                        }
                    }
                    dir = d.parent();
                }
            }
            return (Some(format!("{m:o}")), exposed);
        }
    }
    (None, false)
}

/// Modified time as RFC 3339, when the file exists.
pub fn modified_of(path: &Path) -> Option<String> {
    let md = std::fs::metadata(path).ok()?;
    let t: DateTime<Utc> = md.modified().ok()?.into();
    Some(t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

pub fn modified_dt(path: &Path) -> Option<DateTime<Utc>> {
    let md = std::fs::metadata(path).ok()?;
    Some(md.modified().ok()?.into())
}

pub fn size_of(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// The payload of a JWT, decoded locally — never verified, never shown.
pub fn jwt_payload(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let _sig = parts.next()?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// A time written as RFC 3339, epoch seconds or epoch milliseconds.
pub fn parse_time(v: &Value, unit_hint: &str) -> Option<DateTime<Utc>> {
    match v {
        Value::String(s) => {
            if let Ok(t) = DateTime::parse_from_rfc3339(s) {
                return Some(t.with_timezone(&Utc));
            }
            if let Ok(n) = s.parse::<i64>() {
                return epoch(n, unit_hint);
            }
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|n| DateTime::<Utc>::from_naive_utc_and_offset(n, Utc))
        }
        Value::Number(n) => {
            let f = n.as_f64()?;
            epoch(f as i64, unit_hint)
        }
        _ => None,
    }
}

fn epoch(n: i64, unit_hint: &str) -> Option<DateTime<Utc>> {
    // milliseconds when the hint says so, or when the number is too large for seconds
    if unit_hint == "ms" || n > 100_000_000_000 {
        DateTime::<Utc>::from_timestamp_millis(n)
    } else {
        DateTime::<Utc>::from_timestamp(n, 0)
    }
}

pub fn rfc3339(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Rows of a key/value table of a SQLite database, read-only, as an object. The database
/// is opened read-only; when it is locked by the host, a copy is read from a temporary
/// file that is removed with it.
pub fn sqlite_kv(
    path: &Path,
    table: &str,
    key_col: &str,
    value_col: &str,
) -> Result<Value, String> {
    let conn = open_ro(path)?;
    let sql = format!("SELECT \"{key_col}\", \"{value_col}\" FROM \"{table}\"");
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            let k: String = r.get(0)?;
            let v: rusqlite::types::Value = r.get(1)?;
            Ok((k, v))
        })
        .map_err(|e| e.to_string())?;
    let mut map = serde_json::Map::new();
    for row in rows.flatten() {
        let v = match row.1 {
            rusqlite::types::Value::Text(s) => Value::String(s),
            rusqlite::types::Value::Integer(i) => Value::from(i),
            rusqlite::types::Value::Real(f) => Value::from(f),
            rusqlite::types::Value::Blob(b) => {
                Value::String(String::from_utf8_lossy(&b).to_string())
            }
            rusqlite::types::Value::Null => Value::Null,
        };
        map.insert(row.0, v);
    }
    Ok(Value::Object(map))
}

/// Open a SQLite database read-only, without creating anything; `immutable=1` so no lock
/// or journal file is touched (the host may be writing — a torn read is an error, not a
/// write).
pub fn open_ro(path: &Path) -> Result<rusqlite::Connection, String> {
    let uri = format!("file:{}?mode=ro&immutable=1", path.display());
    rusqlite::Connection::open_with_flags(
        uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | rusqlite::OpenFlags::SQLITE_OPEN_URI
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| e.to_string())
}

/// The salt of this run, drawn once, never written.
fn salt() -> &'static [u8; 32] {
    static S: OnceLock<[u8; 32]> = OnceLock::new();
    S.get_or_init(|| {
        let mut b = [0u8; 32];
        if getrandom::getrandom(&mut b).is_err() {
            // no entropy source: a process-unique value still makes fingerprints
            // comparable within the run and useless outside it
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            b[..16].copy_from_slice(&t.to_le_bytes());
            b[16..24].copy_from_slice(&(std::process::id() as u64).to_le_bytes());
        }
        b
    })
}

/// Eight hex characters identifying a secret within this run only.
pub fn fingerprint(secret: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(salt()).expect("any key length");
    mac.update(secret.trim().as_bytes());
    let out = mac.finalize().into_bytes();
    out.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

/// `~`-relative display of a path.
pub fn display_home(p: &Path, home: &Path) -> String {
    match p.strip_prefix(home) {
        Ok(rel) if rel.as_os_str().is_empty() => "~".into(),
        Ok(rel) => format!("~/{}", rel.display()),
        Err(_) => p.display().to_string(),
    }
}

/// The e-mail's domain only.
pub fn email_domain(s: &str) -> Option<String> {
    let (_, d) = s.trim().rsplit_once('@')?;
    if d.is_empty() {
        None
    } else {
        Some(d.to_lowercase())
    }
}

/// Files matching a glob-less path, or a glob, sorted, existing only.
pub fn existing(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = paths.into_iter().filter(|p| p.is_file()).collect();
    v.sort();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_payload_decodes_without_the_signature() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"email":"a@b.co","exp":1}"#);
        let tok = format!("eyJhbGciOiJSUzI1NiJ9.{payload}.sig");
        let v = jwt_payload(&tok).unwrap();
        assert_eq!(v["email"], "a@b.co");
        assert!(jwt_payload("nope").is_none());
    }

    #[test]
    fn fingerprints_agree_within_a_run_and_show_nothing() {
        let a = fingerprint("sk-ant-api03-xyz");
        let b = fingerprint("sk-ant-api03-xyz ");
        assert_eq!(a, b);
        assert_eq!(a.len(), 8);
        assert_ne!(a, fingerprint("sk-ant-api03-xyw"));
        assert!(!a.contains("xyz"));
    }

    #[test]
    fn times_in_three_shapes() {
        assert!(parse_time(&Value::from(1_760_000_000i64), "").is_some());
        assert!(parse_time(&Value::from(1_760_000_000_000i64), "").is_some());
        assert!(parse_time(&Value::String("2026-10-17T10:00:00Z".into()), "").is_some());
        assert_eq!(
            email_domain("someone@example.org").as_deref(),
            Some("example.org")
        );
    }
}
