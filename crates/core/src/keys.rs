//! The keys: every provider credential found on the machine, where it sits, who reads it,
//! how git sees it — masked the moment it is read, fingerprinted for this run only. The
//! clear value never leaves this module.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use bbl_hostrules::env::Env;
use bbl_hostrules::read::{dig, parse, read_tree};
use bbl_hostrules::resolve::resolve;
use bbl_hostrules::rules::{Format, Location};
use bbl_hostrules::secrets;
use serde_json::Value;

use crate::gitstat;
use crate::model::{GitState, Key, KeyPlace};
use crate::providers::{Provider, Providers};
use crate::sheets::{BillLocation, Sheets};
use crate::util;

/// A candidate before classification: the clear value lives here only.
struct Candidate {
    name: String,
    value: String,
    place: KeyPlace,
    path: PathBuf,
    line: Option<usize>,
    used_by: Vec<String>,
}

/// Provider-style masking: the prefix through its type tag (at most 14 characters), an
/// ellipsis, the last four characters. A short value shows nothing.
pub fn mask_key(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < 16 {
        return "••••••••".into();
    }
    // keep the prefix through its last separator within the first 14 characters
    // (`sk-ant-api03-`, `sk-proj-`, `gsk_`, `sk-or-v1-`); a key without one shows 6
    let mut head_len = 0;
    for (i, c) in chars.iter().enumerate().take(14) {
        if *c == '-' || *c == '_' {
            head_len = i + 1;
        }
    }
    if head_len == 0 || head_len + 4 >= chars.len() {
        head_len = 6;
    }
    let head: String = chars[..head_len].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

fn is_reference(v: &str) -> bool {
    let t = v.trim();
    secrets::is_reference(t)
        || t.starts_with("op://")
        || t.starts_with("ENC[")
        || t.starts_with("os.environ/")
        || t.starts_with("{env:")
        || t.starts_with("{file:")
        || t.starts_with("${{")
}

/// Does the value look like a credential at all (not a placeholder, not a URL)?
fn plausible(v: &str) -> bool {
    let t = v.trim();
    if t.len() < 12 || t.contains(' ') || t.starts_with("http://") || t.starts_with("https://") {
        return false;
    }
    let l = t.to_lowercase();
    !(l.contains("your") && l.contains("key")
        || l.contains("changeme")
        || l.contains("xxxx")
        || l == "null"
        || l == "none"
        || l.starts_with('<') && l.ends_with('>'))
}

fn classify(c: Candidate, providers: &Providers, out: &mut Vec<Key>) {
    let reference = is_reference(&c.value);
    let by_name = providers.by_name(&c.name);
    let provider: Option<&Provider> = match by_name {
        Some(p) if reference || providers.plausible(p, &c.value) || plausible(&c.value) => Some(p),
        Some(_) => None,
        None if !reference => providers.by_shape(c.value.trim()),
        None => None,
    };
    let Some(p) = provider else { return };
    if !reference && !plausible(&c.value) && p.shapes.is_empty() {
        return;
    }
    let (mode, exposed) = if c.place == KeyPlace::ProcessEnv {
        (None, false)
    } else {
        util::mode_of(&c.path)
    };
    let git = if c.place == KeyPlace::ProcessEnv {
        GitState::NotInRepo
    } else {
        gitstat::state(&c.path)
    };
    out.push(Key {
        provider: p.id.clone(),
        provider_name: p.name.clone(),
        name: c.name,
        shown: if reference {
            c.value.trim().to_string()
        } else {
            mask_key(c.value.trim())
        },
        fingerprint: if reference {
            String::new()
        } else {
            util::fingerprint(&c.value)
        },
        place: c.place,
        path: c.path,
        line: c.line,
        mode,
        exposed,
        git,
        used_by: c.used_by,
        reference,
    });
}

/// `NAME=value` lines of a shell start-up file: `export NAME=value`, `NAME=value`, fish
/// `set -x NAME value`. Quotes stripped, nothing expanded.
fn shell_assignments(text: &str) -> Vec<(usize, String, String)> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, value) = if let Some(rest) = line
            .strip_prefix("set -x ")
            .or_else(|| line.strip_prefix("set -gx "))
        {
            let mut it = rest.splitn(2, ' ');
            match (it.next(), it.next()) {
                (Some(n), Some(v)) => (n.to_string(), v.to_string()),
                _ => continue,
            }
        } else {
            let l = line
                .strip_prefix("export ")
                .map(str::trim_start)
                .unwrap_or(line);
            let Some((n, v)) = l.split_once('=') else {
                continue;
            };
            (n.trim().to_string(), v.to_string())
        };
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let mut v = value.trim();
        if let Some(q) = v.chars().next().filter(|q| *q == '"' || *q == '\'') {
            if let Some(end) = v[1..].find(q) {
                v = &v[1..1 + end];
            }
        } else if let Some(k) = v.find(" #") {
            v = v[..k].trim_end();
        }
        out.push((i + 1, name, v.to_string()));
    }
    out
}

fn push_text_file(path: &Path, place: KeyPlace, used_by: Vec<String>, cands: &mut Vec<Candidate>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for (line, name, value) in shell_assignments(&text) {
        cands.push(Candidate {
            name,
            value,
            place,
            path: path.to_path_buf(),
            line: Some(line),
            used_by: used_by.clone(),
        });
    }
}

/// Walk a tree under `root_path` (`""` = whole tree) and collect every string leaf as
/// (name, value); the name is the leaf's key, or `<key>=` list items split for Aider's
/// `api-key: [provider=key]`.
fn leaves(
    tree: &Value,
    key_path: &str,
    text: &str,
    path: &Path,
    place: KeyPlace,
    used_by: &[String],
    cands: &mut Vec<Candidate>,
) {
    let roots: Vec<(Vec<String>, &Value)> = if key_path.is_empty() {
        vec![(Vec::new(), tree)]
    } else {
        dig(tree, key_path)
    };
    for (_, node) in roots {
        collect(
            node,
            key_path.rsplit('.').next().unwrap_or(""),
            text,
            path,
            place,
            used_by,
            cands,
            0,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn collect(
    node: &Value,
    name: &str,
    text: &str,
    path: &Path,
    place: KeyPlace,
    used_by: &[String],
    cands: &mut Vec<Candidate>,
    depth: u8,
) {
    if depth > 6 {
        return;
    }
    match node {
        Value::String(s) => {
            let (n, v) = match s.split_once('=') {
                Some((p, k))
                    if !p.contains(' ') && name.contains("key") && !s.starts_with("sk-") =>
                {
                    (format!("{p}_api_key"), k.to_string())
                }
                _ => (name.to_string(), s.clone()),
            };
            let line = bbl_hostrules::read::line_of(text, &v);
            cands.push(Candidate {
                name: n,
                value: v,
                place,
                path: path.to_path_buf(),
                line,
                used_by: used_by.to_vec(),
            });
        }
        Value::Object(m) => {
            for (k, v) in m {
                collect(v, k, text, path, place, used_by, cands, depth + 1);
            }
        }
        Value::Array(a) => {
            for v in a {
                collect(v, name, text, path, place, used_by, cands, depth + 1);
            }
        }
        _ => {}
    }
}

/// `~/.netrc`: a login/password per machine; the machine names the provider.
fn netrc(path: &Path, providers: &Providers, cands: &mut Vec<Candidate>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let mut machine = String::new();
    for (i, line) in text.lines().enumerate() {
        let toks: Vec<&str> = line.split_whitespace().collect();
        let mut j = 0;
        while j + 1 < toks.len() {
            match toks[j] {
                "machine" => machine = toks[j + 1].to_string(),
                "password" => {
                    let host = machine.to_lowercase();
                    let name = providers
                        .list
                        .iter()
                        .find(|p| {
                            host.contains(&p.id)
                                || p.config_names
                                    .iter()
                                    .any(|c| host.contains(&c.replace("-api-key", "")))
                        })
                        .map(|p| format!("{}_api_key", p.id))
                        .unwrap_or_else(|| format!("netrc:{machine}"));
                    cands.push(Candidate {
                        name,
                        value: toks[j + 1].to_string(),
                        place: KeyPlace::Other,
                        path: path.to_path_buf(),
                        line: Some(i + 1),
                        used_by: Vec::new(),
                    });
                }
                _ => {}
            }
            j += 2;
        }
    }
}

/// Every key on the machine, in a stable order.
pub fn keys(
    sheets: &Sheets,
    providers: &Providers,
    env: &Env,
    present: &[String],
    roots: &[PathBuf],
) -> Vec<Key> {
    let mut cands: Vec<Candidate> = Vec::new();
    // 1. the process environment
    for (k, v) in &env.vars {
        cands.push(Candidate {
            name: k.clone(),
            value: v.clone(),
            place: KeyPlace::ProcessEnv,
            path: PathBuf::from(format!("env:{k}")),
            line: None,
            used_by: Vec::new(),
        });
    }
    // 2. shell start-up files
    for rc in [
        ".bashrc",
        ".bash_profile",
        ".profile",
        ".zshrc",
        ".zprofile",
        ".zshenv",
        ".config/fish/config.fish",
    ] {
        let p = env.home.join(rc);
        if p.is_file() {
            push_text_file(&p, KeyPlace::ShellRc, Vec::new(), &mut cands);
        }
    }
    // 3. systemd user environment
    let mut envd: Vec<PathBuf> = Vec::new();
    for spec in [
        "~/.config/environment.d/*.conf",
        "/etc/environment.d/*.conf",
    ] {
        if let Some(p) = env.expand(spec) {
            envd.extend(bbl_hostrules::resolve::expand_glob(&p));
        }
    }
    envd.push(PathBuf::from("/etc/environment"));
    for p in util::existing(envd) {
        push_text_file(&p, KeyPlace::EnvironmentD, Vec::new(), &mut cands);
    }
    // 4. project dotenv files and direnv
    let mut seen_roots: BTreeSet<PathBuf> = BTreeSet::new();
    for root in roots {
        if !seen_roots.insert(root.clone()) || !root.is_dir() {
            continue;
        }
        if let Ok(rd) = std::fs::read_dir(root) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let p = e.path();
                if !p.is_file() {
                    continue;
                }
                if name == ".env" || name.starts_with(".env.") || name == ".envrc" {
                    push_text_file(&p, KeyPlace::Dotenv, Vec::new(), &mut cands);
                }
            }
        }
    }
    let home_env = env.home.join(".env");
    if home_env.is_file() {
        push_text_file(
            &home_env,
            KeyPlace::Dotenv,
            vec!["aider".into()],
            &mut cands,
        );
    }
    // 5. the hosts' own files, as the sheets describe them
    for host in &sheets.hosts {
        if !present.contains(&host.id) {
            continue;
        }
        for loc in &host.locations {
            if loc.ext.keys_in.is_empty() {
                continue;
            }
            for r in resolve(loc, env, roots) {
                if !r.path.is_file() {
                    continue;
                }
                let Ok((text, tree)) = read_tree(&r.path, loc.format) else {
                    continue;
                };
                let place = if loc.ext.keys_in.iter().any(|k| k.contains("mcpServers")) {
                    KeyPlace::McpConfig
                } else {
                    KeyPlace::ToolConfig
                };
                for k in &loc.ext.keys_in {
                    leaves(
                        &tree,
                        k,
                        &text,
                        &r.path,
                        place,
                        std::slice::from_ref(&host.id),
                        &mut cands,
                    );
                }
            }
        }
    }
    // 6. other well-known stores
    for (spec, format, key_path, used_by) in [
        (
            "~/.config/io.datasette.llm/keys.json",
            Format::Json,
            "",
            "llm",
        ),
        (
            "~/.continue/config.yaml",
            Format::Yaml,
            "models.*.apiKey",
            "continue",
        ),
        ("~/.aws/credentials", Format::Toml, "", "aws-cli"),
    ] {
        let Some(p) = env.expand(spec) else { continue };
        if !p.is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        let tree = match format {
            Format::Toml => parse(&text, Format::Toml).or_else(|_| parse(&text, Format::Dotenv)),
            f => parse(&text, f),
        };
        if let Ok(tree) = tree {
            leaves(
                &tree,
                key_path,
                &text,
                &p,
                KeyPlace::Other,
                &[used_by.to_string()],
                &mut cands,
            );
        }
    }
    if let Some(p) = env.expand("~/.cache/huggingface/token") {
        if let Ok(text) = std::fs::read_to_string(&p) {
            cands.push(Candidate {
                name: "HF_TOKEN".into(),
                value: text.trim().to_string(),
                place: KeyPlace::Other,
                path: p,
                line: Some(1),
                used_by: vec!["huggingface_hub".into()],
            });
        }
    }
    if let Some(p) = env.expand("~/.netrc") {
        if p.is_file() {
            netrc(&p, providers, &mut cands);
        }
    }
    // classify, then order
    let mut out = Vec::new();
    for c in cands {
        classify(c, providers, &mut out);
    }
    out.sort_by_key(|a| (a.provider.clone(), a.path.clone(), a.line));
    out.dedup_by(|a, b| a.path == b.path && a.line == b.line && a.name == b.name);
    out
}

/// Attach the hosts that read a location to the keys found in it (for `used_by`).
pub fn sheet_readers(sheets: &Sheets, env: &Env, roots: &[PathBuf]) -> Vec<(PathBuf, String)> {
    let mut v = Vec::new();
    for host in &sheets.hosts {
        for loc in &host.locations {
            for r in resolve::<BillLocation>(loc as &Location<BillLocation>, env, roots) {
                v.push((r.path, host.id.clone()));
            }
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_keep_the_prefix_and_four_characters() {
        let k = format!("sk-ant-api03-{}AA", "R".repeat(93));
        let m = mask_key(&k);
        assert!(m.starts_with("sk-ant-api03-…"), "{m}");
        assert!(m.ends_with("RRAA"));
        assert!(!m.contains("RRRRRRRR"));
        assert_eq!(mask_key("short"), "••••••••");
        assert_eq!(
            mask_key(&format!(
                "sk-proj-{}T3BlbkFJ{}",
                "a".repeat(74),
                "b".repeat(74)
            )),
            "sk-proj-…bbbb"
        );
        // assembled at run time: a source file never holds a string of a provider's exact shape
        let d = format!("sk-{0}{0}", "0123456789abcdef");
        assert_eq!(mask_key(&d), "sk-…cdef");
        let g = "gsk_0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOP";
        assert_eq!(mask_key(g), "gsk_…MNOP");
    }

    #[test]
    fn shell_lines_in_three_shells() {
        let v = shell_assignments("export OPENAI_API_KEY=\"sk-x\" # c\nGROQ_API_KEY=gsk_y\nset -x XAI_API_KEY xai-z\nalias ll='ls'\n");
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], (1, "OPENAI_API_KEY".into(), "sk-x".into()));
        assert_eq!(v[2], (3, "XAI_API_KEY".into(), "xai-z".into()));
    }
}
