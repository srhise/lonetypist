//! Timed backups, in the spirit of WordPerfect's periodic save.
//!
//! Backups never overwrite the user's file: they go to Application
//! Support, and a backup that outlives its document offers recovery on
//! the next launch.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const INTERVAL_MS: u64 = 30_000;

/// `~/Library/Application Support/lonetypist/backup`
pub fn dir() -> Option<PathBuf> {
    crate::config::app_dir().map(|d| d.join("backup"))
}

pub fn path_for_in(dir: &Path, doc: Option<&Path>) -> PathBuf {
    let stem = doc
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "UNTITLED".to_string());
    dir.join(format!("{stem}.BK1"))
}

pub fn write_in(dir: &Path, doc: Option<&Path>, text: &str) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    crate::fileio::save(&path_for_in(dir, doc), text, false)
}

/// A backup worth offering: it exists, and either the document has no
/// path at all or the backup is strictly newer than the document.
pub fn pending_in(dir: &Path, doc: Option<&Path>) -> Option<PathBuf> {
    let backup = path_for_in(dir, doc);
    let backup_time = fs::metadata(&backup).ok()?.modified().ok()?;
    match doc {
        None => Some(backup),
        Some(d) => match fs::metadata(d).ok().and_then(|m| m.modified().ok()) {
            Some(doc_time) if doc_time >= backup_time => None,
            _ => Some(backup),
        },
    }
}

pub fn clear_in(dir: &Path, doc: Option<&Path>) {
    let _ = fs::remove_file(path_for_in(dir, doc));
}

// Thin wrappers over the real directory, for the shell.

pub fn write(doc: Option<&Path>, text: &str) -> io::Result<()> {
    let d = dir().ok_or_else(|| io::Error::other("no data directory"))?;
    write_in(&d, doc, text)
}

pub fn pending(doc: Option<&Path>) -> Option<PathBuf> {
    pending_in(&dir()?, doc)
}

pub fn clear(doc: Option<&Path>) {
    if let Some(d) = dir() {
        clear_in(&d, doc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn an_untitled_document_backs_up_under_a_fixed_name() {
        let dir = tempdir().expect("tempdir");
        let p = path_for_in(dir.path(), None);
        assert_eq!(p.file_name().expect("name"), "UNTITLED.BK1");
    }

    #[test]
    fn a_named_document_backs_up_under_its_own_name() {
        let dir = tempdir().expect("tempdir");
        let doc = Path::new("/tmp/chapter-one.txt");
        let p = path_for_in(dir.path(), Some(doc));
        assert_eq!(p.file_name().expect("name"), "chapter-one.txt.BK1");
    }

    #[test]
    fn writing_a_backup_creates_the_directory() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("nested").join("backup");
        write_in(&target, None, "draft text").expect("write");
        assert_eq!(
            fs::read_to_string(target.join("UNTITLED.BK1")).expect("read"),
            "draft text"
        );
    }

    #[test]
    fn no_backup_pending_when_none_was_written() {
        let dir = tempdir().expect("tempdir");
        assert!(pending_in(dir.path(), None).is_none());
    }

    #[test]
    fn a_backup_of_an_untitled_document_is_always_pending() {
        let dir = tempdir().expect("tempdir");
        write_in(dir.path(), None, "unsaved work").expect("write");
        assert!(pending_in(dir.path(), None).is_some());
    }

    #[test]
    fn a_backup_older_than_its_document_is_not_pending() {
        let docs = tempdir().expect("tempdir");
        let backups = tempdir().expect("tempdir");
        let doc = docs.path().join("a.txt");
        fs::write(&doc, "old").expect("write");
        write_in(backups.path(), Some(&doc), "older backup").expect("write");
        // Touch the document so it is strictly newer.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&doc, "new").expect("write");
        assert!(pending_in(backups.path(), Some(&doc)).is_none());
    }

    #[test]
    fn a_backup_newer_than_its_document_is_pending() {
        let docs = tempdir().expect("tempdir");
        let backups = tempdir().expect("tempdir");
        let doc = docs.path().join("a.txt");
        fs::write(&doc, "saved").expect("write");
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_in(backups.path(), Some(&doc), "later work").expect("write");
        assert!(pending_in(backups.path(), Some(&doc)).is_some());
    }

    #[test]
    fn clearing_removes_the_backup() {
        let dir = tempdir().expect("tempdir");
        write_in(dir.path(), None, "x").expect("write");
        clear_in(dir.path(), None);
        assert!(pending_in(dir.path(), None).is_none());
    }

    #[test]
    fn clearing_a_backup_that_is_not_there_is_not_an_error() {
        let dir = tempdir().expect("tempdir");
        clear_in(dir.path(), None); // must not panic
    }
}
