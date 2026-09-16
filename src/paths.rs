//! Turning what someone types in the Retrieve or Save As box into a path.
//!
//! A bare name lands in the writing folder; anything with a slash or a
//! tilde is read as a path, the way a shell would. Sandboxed, that last
//! part has to be fenced: the sandbox would happily "save" a file into
//! the container where the user will never find it, so a path that
//! leaves the writing folder is refused out loud instead.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// Sandboxed, and the path points outside the writing folder.
    Outside,
    /// Nothing but whitespace was typed.
    Empty,
}

/// `base` is the writing folder, `home` the user's home directory when
/// one is known, and `fenced` whether paths must stay under `base`.
pub fn resolve(base: &Path, home: Option<&Path>, value: &str, fenced: bool) -> Result<PathBuf, Error> {
    let value = value.trim();
    if value.is_empty() {
        return Err(Error::Empty);
    }

    let joined = if let Some(rest) = value.strip_prefix("~/") {
        match home {
            Some(h) => h.join(rest),
            None => base.join(rest),
        }
    } else {
        let p = Path::new(value);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            base.join(p)
        }
    };

    let cleaned = lexically_normal(&joined);
    if fenced && !cleaned.starts_with(lexically_normal(base)) {
        return Err(Error::Outside);
    }
    Ok(cleaned)
}

/// Resolve `.` and `..` without touching the disk, so that a name like
/// `../../secrets.txt` is fenced out before anything is opened.
fn lexically_normal(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> PathBuf {
        PathBuf::from("/Users/who/Documents")
    }

    fn home() -> PathBuf {
        PathBuf::from("/Users/who")
    }

    #[test]
    fn a_bare_name_lands_in_the_writing_folder() {
        assert_eq!(
            resolve(&base(), Some(&home()), "chapter-one.txt", false),
            Ok(base().join("chapter-one.txt"))
        );
    }

    #[test]
    fn surrounding_space_is_ignored() {
        assert_eq!(
            resolve(&base(), Some(&home()), "  chapter-one.txt  ", false),
            Ok(base().join("chapter-one.txt"))
        );
    }

    #[test]
    fn a_tilde_means_home() {
        assert_eq!(
            resolve(&base(), Some(&home()), "~/notes/todo.txt", false),
            Ok(home().join("notes/todo.txt"))
        );
    }

    #[test]
    fn an_absolute_path_is_taken_as_given() {
        assert_eq!(
            resolve(&base(), Some(&home()), "/tmp/a.txt", false),
            Ok(PathBuf::from("/tmp/a.txt"))
        );
    }

    #[test]
    fn a_subfolder_is_fine_when_fenced() {
        assert_eq!(
            resolve(&base(), Some(&home()), "book/ch1.txt", true),
            Ok(base().join("book/ch1.txt"))
        );
    }

    #[test]
    fn fenced_refuses_a_path_that_leaves_the_folder() {
        assert_eq!(
            resolve(&base(), Some(&home()), "/etc/hosts", true),
            Err(Error::Outside)
        );
        assert_eq!(
            resolve(&base(), Some(&home()), "~/Desktop/a.txt", true),
            Err(Error::Outside)
        );
    }

    #[test]
    fn fenced_refuses_climbing_out_with_dot_dot() {
        assert_eq!(
            resolve(&base(), Some(&home()), "../../etc/hosts", true),
            Err(Error::Outside)
        );
    }

    #[test]
    fn dot_dot_inside_the_folder_is_allowed() {
        assert_eq!(
            resolve(&base(), Some(&home()), "book/../ch1.txt", true),
            Ok(base().join("ch1.txt"))
        );
    }

    #[test]
    fn unfenced_allows_anywhere() {
        assert_eq!(
            resolve(&base(), Some(&home()), "/etc/hosts", false),
            Ok(PathBuf::from("/etc/hosts"))
        );
    }

    #[test]
    fn nothing_typed_is_an_error() {
        assert_eq!(resolve(&base(), Some(&home()), "   ", false), Err(Error::Empty));
    }

    #[test]
    fn a_missing_home_falls_back_to_the_writing_folder() {
        assert_eq!(
            resolve(&base(), None, "~/a.txt", true),
            Ok(base().join("a.txt"))
        );
    }
}
