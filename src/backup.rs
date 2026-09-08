//! Backup of game files overwritten by this tool, restored on Remove.
//!
//! Layout next to the game exe:
//!   original_files/<relative path as installed>
//! Only files that already existed before we wrote over them are stashed.
//! Files we created from scratch are simply deleted on uninstall.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub const ORIGINAL_FILES_DIR: &str = "original_files";

fn backup_root(game_dir: &Path) -> PathBuf {
    game_dir.join(ORIGINAL_FILES_DIR)
}

/// Normalise a relative install path (forward slashes, no `..`).
fn clean_rel(rel: &str) -> Option<PathBuf> {
    let normalized = rel.replace('\\', "/");
    let parts: Vec<&str> = normalized
        .split('/')
        .filter(|p| !p.is_empty() && *p != "." && *p != "..")
        .collect();
    if parts.is_empty() {
        return None;
    }
    Some(parts.iter().fold(PathBuf::new(), |acc, p| acc.join(p)))
}

/// If `game_dir/rel` already exists and we have not backed it up yet, copy it
/// into `original_files/`. Returns true when a new backup was written.
pub fn stash(game_dir: &Path, rel: &str) -> Result<bool> {
    let Some(rel_path) = clean_rel(rel) else {
        return Ok(false);
    };
    let src = game_dir.join(&rel_path);
    if !src.is_file() {
        return Ok(false);
    }
    let dest = backup_root(game_dir).join(&rel_path);
    if dest.is_file() {
        return Ok(false);
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    fs::copy(&src, &dest).with_context(|| {
        format!(
            "cannot back up {} → {}",
            src.display(),
            dest.display()
        )
    })?;
    Ok(true)
}

/// Restore every file under `original_files/` into the game folder, then remove
/// the backup tree. Paths already restored are reported in `restored`.
pub fn restore_all(game_dir: &Path, restored: &mut Vec<String>) -> Result<()> {
    let root = backup_root(game_dir);
    if !root.is_dir() {
        return Ok(());
    }
    restore_walk(game_dir, &root, &root, restored)?;
    // Remove leftover empty directories under original_files, then the root.
    remove_dir_contents(&root)?;
    let _ = fs::remove_dir(&root);
    Ok(())
}

fn restore_walk(
    game_dir: &Path,
    root: &Path,
    dir: &Path,
    restored: &mut Vec<String>,
) -> Result<()> {
    let rd = fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))?;
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            restore_walk(game_dir, root, &path, restored)?;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let dest = game_dir.join(
            path.strip_prefix(root)
                .unwrap_or(&path),
        );
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&path, &dest)
            .with_context(|| format!("cannot restore {} → {}", path.display(), dest.display()))?;
        restored.push(format!("{ORIGINAL_FILES_DIR} → {rel}"));
    }
    Ok(())
}

fn remove_dir_contents(dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            remove_dir_contents(&path)?;
            let _ = fs::remove_dir(&path);
        } else {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stash_once_and_restore() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        fs::write(d.join("libxess.dll"), b"game").unwrap();
        assert!(stash(d, "libxess.dll").unwrap());
        assert!(!stash(d, "libxess.dll").unwrap()); // already backed up
        assert_eq!(
            fs::read(d.join(ORIGINAL_FILES_DIR).join("libxess.dll")).unwrap(),
            b"game"
        );
        fs::write(d.join("libxess.dll"), b"opti").unwrap();
        let mut restored = Vec::new();
        restore_all(d, &mut restored).unwrap();
        assert_eq!(fs::read(d.join("libxess.dll")).unwrap(), b"game");
        assert!(!d.join(ORIGINAL_FILES_DIR).exists());
        assert!(restored.iter().any(|r| r.contains("libxess.dll")));
    }

    #[test]
    fn stash_skips_missing_and_rejects_traversal() {
        let t = tempfile::tempdir().unwrap();
        assert!(!stash(t.path(), "missing.dll").unwrap());
        assert!(!stash(t.path(), "../evil.dll").unwrap());
    }
}
