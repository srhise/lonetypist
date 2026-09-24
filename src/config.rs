//! Settings. A missing or malformed file is replaced with defaults
//! rather than reported: nobody should be shown a dialog about their
//! preferences file on the way into a writing app.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub effects: bool,
    pub dense: bool,
    pub fullscreen: bool,
    pub window: (u32, u32),
    pub recent: Option<PathBuf>,
    /// Where bare filenames land. Defaults to ~/Documents.
    pub base_dir: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            effects: true,
            dense: false,
            fullscreen: false,
            window: (1080, 810),
            recent: None,
            base_dir: None,
        }
    }
}

const DIR_NAME: &str = "lonetypist";
const OLD_DIR_NAME: &str = "phosphor";

/// `~/Library/Application Support/lonetypist`: settings, backups and the
/// crash log.
pub fn app_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(DIR_NAME))
}

/// Carry the folder from the app's previous name across, so a pending
/// backup is still offered for recovery.
pub fn migrate_old_dir() {
    if let Some(root) = dirs::data_dir() {
        migrate_in(&root);
    }
}

fn migrate_in(root: &Path) {
    let old = root.join(OLD_DIR_NAME);
    let new = root.join(DIR_NAME);
    if !old.is_dir() {
        return;
    }
    // An earlier launch can leave the new folder behind empty -- a crash
    // log that was never written, a settings save that never happened.
    // An empty folder is not data, and must not strand the old one.
    let occupied = fs::read_dir(&new)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false);
    if occupied {
        return;
    }
    let _ = fs::remove_dir(&new);
    let _ = fs::rename(&old, &new);
}

fn path() -> Option<PathBuf> {
    app_dir().map(|d| d.join("config.toml"))
}

pub fn parse(text: &str) -> Config {
    toml::from_str(text).unwrap_or_default()
}

pub fn render(c: &Config) -> String {
    toml::to_string_pretty(c).unwrap_or_default()
}

pub fn load() -> Config {
    path()
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|t| parse(&t))
        .unwrap_or_default()
}

pub fn save(c: &Config) {
    let Some(p) = path() else { return };
    if let Some(dir) = p.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(p, render(c));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_authentic_ones() {
        let c = Config::default();
        assert!(c.effects, "the CRT is the point");
        assert!(!c.dense, "80x25 is the default text mode");
        assert!(!c.fullscreen);
        assert_eq!(c.window, (1080, 810), "4:3");
    }

    #[test]
    fn a_config_round_trips() {
        let c = Config {
            effects: false,
            dense: true,
            window: (1440, 1080),
            recent: Some(PathBuf::from("/tmp/a.txt")),
            ..Default::default()
        };
        assert_eq!(parse(&render(&c)), c);
    }

    #[test]
    fn a_corrupt_file_yields_defaults_rather_than_an_error() {
        assert_eq!(parse("this is not toml {{{"), Config::default());
    }

    #[test]
    fn an_empty_file_yields_defaults() {
        assert_eq!(parse(""), Config::default());
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let c = parse("effects = false\nfuture_option = 42\n");
        assert!(!c.effects);
        assert!(!c.dense, "the rest stay at their defaults");
    }

    #[test]
    fn the_old_folder_moves_to_the_new_name() {
        let root = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(root.path().join(OLD_DIR_NAME).join("backup")).expect("mkdir");
        fs::write(root.path().join(OLD_DIR_NAME).join("config.toml"), "dense = true\n")
            .expect("write");

        migrate_in(root.path());

        assert!(!root.path().join(OLD_DIR_NAME).exists());
        assert!(root.path().join(DIR_NAME).join("backup").is_dir());
        assert_eq!(
            fs::read_to_string(root.path().join(DIR_NAME).join("config.toml")).expect("read"),
            "dense = true\n"
        );
    }

    #[test]
    fn an_empty_new_folder_does_not_block_the_move() {
        let root = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(root.path().join(OLD_DIR_NAME).join("backup")).expect("mkdir");
        fs::write(root.path().join(OLD_DIR_NAME).join("config.toml"), "dense = true\n")
            .expect("write");
        // The shape that actually happened: the new folder exists, empty.
        fs::create_dir_all(root.path().join(DIR_NAME)).expect("mkdir");

        migrate_in(root.path());

        assert!(!root.path().join(OLD_DIR_NAME).exists(), "old folder moved");
        assert_eq!(
            fs::read_to_string(root.path().join(DIR_NAME).join("config.toml")).expect("read"),
            "dense = true\n"
        );
        assert!(root.path().join(DIR_NAME).join("backup").is_dir());
    }

    #[test]
    fn an_existing_new_folder_is_never_overwritten() {
        let root = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(root.path().join(OLD_DIR_NAME)).expect("mkdir");
        fs::create_dir_all(root.path().join(DIR_NAME)).expect("mkdir");
        fs::write(root.path().join(DIR_NAME).join("config.toml"), "new").expect("write");

        migrate_in(root.path());

        assert!(root.path().join(OLD_DIR_NAME).is_dir(), "old folder left alone");
        assert_eq!(
            fs::read_to_string(root.path().join(DIR_NAME).join("config.toml")).expect("read"),
            "new"
        );
    }

    #[test]
    fn nothing_to_migrate_is_not_an_error() {
        let root = tempfile::tempdir().expect("tempdir");
        migrate_in(root.path());
        assert!(!root.path().join(DIR_NAME).exists());
    }

    #[test]
    fn a_partial_file_keeps_the_other_defaults() {
        let c = parse("dense = true\n");
        assert!(c.dense);
        assert!(c.effects, "unspecified keys stay default");
        assert_eq!(c.window, (1080, 810));
    }
}
