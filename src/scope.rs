//! Living inside the App Sandbox.
//!
//! The App Store build runs sandboxed, where `$HOME` is rewritten to the
//! app's own container: a path typed into the Retrieve box would land
//! somewhere the user cannot see, and appear to have saved. So a
//! sandboxed run asks once for a writing folder, and keeps a
//! security-scoped bookmark to it. That bookmark is the only way the
//! grant survives a relaunch.
//!
//! The direct-download build is not sandboxed and skips all of this.

use std::path::{Path, PathBuf};

/// The sandbox sets this for every app it contains, and nothing else
/// does. One binary can therefore serve both builds.
pub fn sandboxed() -> bool {
    std::env::var_os("APP_SANDBOX_CONTAINER_ID").is_some()
}

/// Where the bookmark is kept, beside the settings.
fn bookmark_path() -> Option<PathBuf> {
    crate::config::app_dir().map(|d| d.join("writing-folder.bookmark"))
}

pub fn save_bookmark(data: &[u8]) -> std::io::Result<()> {
    let Some(p) = bookmark_path() else {
        return Err(std::io::Error::other("no application support directory"));
    };
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(p, data)
}

pub fn load_bookmark() -> Option<Vec<u8>> {
    std::fs::read(bookmark_path()?).ok()
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use objc2::rc::Retained;
    use objc2_foundation::{
        NSData, NSString, NSURL, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions,
    };

    /// Access to a folder outside the container, held open for as long as
    /// this value lives. Dropping it gives the access back.
    pub struct Access {
        url: Retained<NSURL>,
        path: PathBuf,
    }

    impl Access {
        pub fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for Access {
        fn drop(&mut self) {
            unsafe { self.url.stopAccessingSecurityScopedResource() };
        }
    }

    /// Bookmark data for a folder the user just picked, to be stored and
    /// resolved on later launches.
    pub fn bookmark(path: &Path) -> Option<Vec<u8>> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let data = url
            .bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
                NSURLBookmarkCreationOptions::WithSecurityScope,
                None,
                None,
            )
            .ok()?;
        Some(data.to_vec())
    }

    /// Resolve stored bookmark data and start using it. `None` means the
    /// folder is gone or the grant no longer holds, and the user has to
    /// pick again.
    pub fn resolve(data: &[u8]) -> Option<Access> {
        let data = NSData::with_bytes(data);
        let mut stale = objc2::runtime::Bool::NO;
        let url = unsafe {
            NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
                &data,
                NSURLBookmarkResolutionOptions::WithSecurityScope,
                None,
                std::ptr::from_mut(&mut stale),
            )
        }
        .ok()?;
        if stale.as_bool() {
            return None;
        }
        if !unsafe { url.startAccessingSecurityScopedResource() } {
            return None;
        }
        let path = PathBuf::from(url.path()?.to_string());
        Some(Access { url, path })
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::*;

    pub struct Access {
        path: PathBuf,
    }

    impl Access {
        pub fn path(&self) -> &Path {
            &self.path
        }
    }

    pub fn bookmark(_path: &Path) -> Option<Vec<u8>> {
        None
    }

    pub fn resolve(_data: &[u8]) -> Option<Access> {
        None
    }
}

pub use imp::{bookmark, resolve, Access};
