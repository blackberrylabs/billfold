//! How git sees a file, without running git: the repository root is found by walking up,
//! a tracked file is one whose path is in `.git/index`, an ignored one matches a
//! `.gitignore` on its way down (or `.git/info/exclude`). Read-only.

use std::path::{Path, PathBuf};

use ignore::gitignore::GitignoreBuilder;

use crate::model::GitState;

/// The repository root above `path`, with its `.git` directory (a worktree's `.git` file is
/// followed).
pub fn repo_root(path: &Path) -> Option<(PathBuf, PathBuf)> {
    let mut dir = if path.is_dir() { path } else { path.parent()? };
    loop {
        let dotgit = dir.join(".git");
        if dotgit.is_dir() {
            return Some((dir.to_path_buf(), dotgit));
        }
        if dotgit.is_file() {
            let text = std::fs::read_to_string(&dotgit).ok()?;
            let target = text.trim().strip_prefix("gitdir:")?.trim();
            let gd = if Path::new(target).is_absolute() {
                PathBuf::from(target)
            } else {
                dir.join(target)
            };
            return Some((dir.to_path_buf(), gd));
        }
        dir = dir.parent()?;
    }
}

/// Is `rel` (slash-separated, relative to the root) an entry of the index?
fn tracked(gitdir: &Path, rel: &str) -> bool {
    let Ok(bytes) = std::fs::read(gitdir.join("index")) else {
        return false;
    };
    let needle = rel.as_bytes();
    if needle.is_empty() || bytes.len() < 12 || &bytes[..4] != b"DIRC" {
        return false;
    }
    // an entry's path is followed by a NUL and preceded by the 2-byte flags
    let mut i = 12;
    while i + needle.len() < bytes.len() {
        if &bytes[i..i + needle.len()] == needle && bytes[i + needle.len()] == 0 {
            let before = bytes[i - 1];
            if before == 0 || !(before.is_ascii_graphic() && before != b'/') {
                return true;
            }
            // also accept when the preceding byte is part of the flags (non-path byte)
            if i >= 2 && (bytes[i - 2] & 0x30) == 0 {
                return true;
            }
        }
        i += 1;
    }
    false
}

pub fn state(path: &Path) -> GitState {
    let Some((root, gitdir)) = repo_root(path) else {
        return GitState::NotInRepo;
    };
    let Ok(rel) = path.strip_prefix(&root) else {
        return GitState::NotInRepo;
    };
    let rel_s = rel.to_string_lossy().replace('\\', "/");
    if tracked(&gitdir, &rel_s) {
        return GitState::Tracked;
    }
    let mut b = GitignoreBuilder::new(&root);
    let _ = b.add(gitdir.join("info").join("exclude"));
    // every .gitignore from the root down to the file's directory
    let mut dir = root.clone();
    let _ = b.add(dir.join(".gitignore"));
    for comp in rel
        .parent()
        .map(|p| p.components().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        dir = dir.join(comp);
        let _ = b.add(dir.join(".gitignore"));
    }
    match b.build() {
        Ok(gi) => {
            if gi.matched_path_or_any_parents(path, false).is_ignore() {
                GitState::Ignored
            } else {
                GitState::Untracked
            }
        }
        Err(_) => GitState::Untracked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracked_ignored_untracked_and_outside() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".gitignore"), ".env\n").unwrap();
        std::fs::write(root.join(".env"), "A=1\n").unwrap();
        std::fs::write(root.join("config.yml"), "x\n").unwrap();
        std::fs::write(root.join("loose.txt"), "x\n").unwrap();
        // a minimal index: header, one entry for config.yml
        let mut idx = Vec::new();
        idx.extend_from_slice(b"DIRC");
        idx.extend_from_slice(&2u32.to_be_bytes());
        idx.extend_from_slice(&1u32.to_be_bytes());
        idx.extend_from_slice(&[0u8; 60]); // ctime..sha
        let name = b"config.yml";
        idx.extend_from_slice(&(name.len() as u16).to_be_bytes());
        idx.extend_from_slice(name);
        idx.push(0);
        std::fs::write(root.join(".git").join("index"), idx).unwrap();
        assert_eq!(state(&root.join("config.yml")), GitState::Tracked);
        assert_eq!(state(&root.join(".env")), GitState::Ignored);
        assert_eq!(state(&root.join("loose.txt")), GitState::Untracked);
        std::fs::write(tmp.path().join("outside.txt"), "x").unwrap();
        assert_eq!(state(&tmp.path().join("outside.txt")), GitState::NotInRepo);
    }
}
