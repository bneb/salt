//! Test-only helpers shared across modules.

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
    old: Option<String>,
    _lock: MutexGuard<'static, ()>,
}

impl HomeGuard {
    pub fn new(new_home: &Path) -> Self {
        // A panicking holder poisons the lock, but its guard already restored
        // $HOME during unwind, so there's no bad state to refuse.
        let lock = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let old = std::env::var("HOME").ok();
        std::env::set_var("HOME", new_home);
        HomeGuard { old, _lock: lock }
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}
