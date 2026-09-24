//! Test-only helpers shared across modules.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// `<system temp dir>/<name>-<pid>`. A name unique to its test keeps tests
/// in one process apart, but concurrent `cargo test` runs (say, in two
/// worktrees) share the temp dir, and with the name alone one run deletes
/// or overwrites another's files.
pub fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}-{}", name, std::process::id()))
}

/// Serializes every test that overrides $HOME: the variable is
/// process-global and `cargo test` runs tests on parallel threads. Tests
/// that only read $HOME don't take it, so they must not depend on its value.
static HOME_LOCK: Mutex<()> = Mutex::new(());

/// Overrides $HOME for the guard's lifetime, restoring it on drop (even on
/// panic) before releasing HOME_LOCK.
pub struct HomeGuard {
    old: Option<OsString>,
    _lock: MutexGuard<'static, ()>,
}

impl HomeGuard {
    pub fn new(new_home: &Path) -> Self {
        // A panicking holder poisons the lock, but its guard already restored
        // $HOME during unwind, so there's no bad state to refuse.
        let lock = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        HomeGuard { old: set_home(new_home), _lock: lock }
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        restore_home(self.old.as_deref());
    }
}

/// Points $HOME at `new_home`, returning the previous value for
/// restore_home. The caller must hold HOME_LOCK.
fn set_home(new_home: &Path) -> Option<OsString> {
    let old = std::env::var_os("HOME");
    std::env::set_var("HOME", new_home);
    old
}

/// The caller must hold HOME_LOCK.
fn restore_home(old: Option<&OsStr>) {
    match old {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
    }
}

#[cfg(unix)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn test_restores_non_utf8_home() {
        // The outer guard holds HOME_LOCK and makes $HOME non-UTF-8, the
        // state a guard must put back.
        let non_utf8 = Path::new(OsStr::from_bytes(b"/nonexistent/home-\xff"));
        let _guard = HomeGuard::new(non_utf8);

        let old = set_home(Path::new("/nonexistent/elsewhere"));
        restore_home(old.as_deref());

        assert_eq!(std::env::var_os("HOME").as_deref(), Some(non_utf8.as_os_str()));
    }
}
