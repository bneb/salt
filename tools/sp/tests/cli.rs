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
