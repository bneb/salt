//! Tests that run the sp binary, for behavior only visible from outside,
//! such as what it prints.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// `<system temp dir>/<name>-<pid>`, unique to this test process; see
/// test_support::temp_path in the crate.
fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}-{}", name, std::process::id()))
}

/// Runs sp in `dir` with $HOME set to `home`, or unset.
fn sp(dir: &Path, home: Option<&Path>, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_sp"));
    cmd.args(args).current_dir(dir);
    match home {
        Some(home) => cmd.env("HOME", home),
        None => cmd.env_remove("HOME"),
    };
    cmd.output().unwrap()
}

fn write_package(dir: &Path, name: &str) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/lib.salt"), format!("package {}\n", name)).unwrap();
    fs::write(
        dir.join("salt.toml"),
        format!("[package]\nname = \"{}\"\nversion = \"1.0.0\"\n", name),
    )
    .unwrap();
}

#[test]
fn add_warns_when_nothing_by_that_name_is_published() {
    let tmp = temp_path("sp_cli_add_unpublished");
    let _ = fs::remove_dir_all(&tmp);
    let home = tmp.join("home");
    let app = tmp.join("app");
    write_package(&app, "app");

    let out = sp(&app, Some(&home), &["add", "foo"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "the dependency is still added: {stderr}");
    assert!(fs::read_to_string(app.join("salt.toml")).unwrap().contains("foo = \"*\""));
    assert!(stderr.contains("warning"), "{stderr}");
    assert!(stderr.contains("nothing named 'foo' is published"), "{stderr}");
    assert!(stderr.contains("`sp build` will fail"), "{stderr}");

    // Published, there's nothing to warn about.
    write_package(&tmp.join("foo"), "foo");
    assert!(sp(&tmp.join("foo"), Some(&home), &["publish"]).status.success());
    let out = sp(&app, Some(&home), &["add", "foo"]);
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");

    // Nothing resolves dev-dependencies, so no build fails over one.
    let out = sp(&app, Some(&home), &["add", "--dev", "baz"]);
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");

    // Without $HOME there's no publish dir to check, which is said too.
    let out = sp(&app, None, &["add", "bar"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("couldn't check whether 'bar' is published"), "{stderr}");

    let _ = fs::remove_dir_all(&tmp);
}

/// `tmp/app`, depending on `foo` 1.0.0, which is published into `home`.
fn app_with_published_dep(tmp: &Path, home: &Path) -> PathBuf {
    write_package(&tmp.join("foo"), "foo");
    assert!(sp(&tmp.join("foo"), Some(home), &["publish"]).status.success());
    let app = tmp.join("app");
    write_package(&app, "app");
    let manifest = fs::read_to_string(app.join("salt.toml")).unwrap();
    fs::write(app.join("salt.toml"), manifest + "\n[dependencies]\nfoo = \"1.0\"\n").unwrap();
    app
}

// Both build tests hold whether or not a saltc is found: salt.lock is
// written before the cache check and before compiling.
#[test]
fn build_writes_salt_lock() {
    let tmp = temp_path("sp_cli_build_lock");
    let _ = fs::remove_dir_all(&tmp);
    let home = tmp.join("home");
    let app = app_with_published_dep(&tmp, &home);

    let out = sp(&app, Some(&home), &["build"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Wrote salt.lock"), "{stdout}");
    let lock = fs::read_to_string(app.join("salt.lock")).expect("sp build writes salt.lock");
    assert!(lock.contains("[packages.foo]\nversion = \"1.0.0\"\nhash = \"sha256:"), "{lock}");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn build_only_warns_when_salt_lock_cannot_be_written() {
    let tmp = temp_path("sp_cli_build_lock_warn");
    let _ = fs::remove_dir_all(&tmp);
    let home = tmp.join("home");
    let app = app_with_published_dep(&tmp, &home);
    // A non-empty directory where salt.lock goes: nothing can replace it.
    fs::create_dir_all(app.join("salt.lock/inside")).unwrap();

    let out = sp(&app, Some(&home), &["build"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("warning") && stderr.contains("salt.lock not written"), "{stderr}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Compiling"), "the build goes on past the lockfile: {stdout}");

    let _ = fs::remove_dir_all(&tmp);
}
