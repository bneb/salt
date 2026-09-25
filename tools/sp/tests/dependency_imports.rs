//! End-to-end: `sp build` / `sp check` on projects that import their
//! dependencies and their own sibling modules.
//!
//! Drives the real `sp` binary against the saltc built from this checkout, so
//! `salt-front/target/release/saltc` must exist (see `saltc()`). Each test
//! works in its own directory with its own $HOME, so sp's artifact cache and
//! ~/.salt/publish start empty and a cache hit can't mask a compile failure.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MYLIB_MANIFEST: &str =
    "[package]\nname = \"mylib\"\nversion = \"1.0.0\"\nentry = \"src/lib.salt\"\n";

const MYLIB_SRC: &str = "\
package mylib

pub fn add(a: i32, b: i32) -> i32 {
    return a + b;
}
";

const APP_MAIN: &str = "\
package main

use mylib.add

fn main() -> i32 {
    return add(2, 3);
}
";

/// The saltc from this checkout. Panics rather than skipping when it isn't
/// built: a skipped end-to-end test reads as a passing one.
fn saltc() -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../salt-front/target/release/saltc");
    assert!(
        path.is_file(),
        "{} not found; build it first: (cd salt-front && cargo build --release)",
        path.display()
    );
    path.canonicalize().unwrap()
}

/// A scratch directory holding one test's projects and its $HOME.
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("sp_e2e_{}", name));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("home")).unwrap();
        Sandbox { root }
    }

    fn write(&self, rel: &str, contents: &str) {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn read(&self, rel: &str) -> String {
        let path = self.root.join(rel);
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
    }

    /// Runs `sp <args>` from `<root>/<dir>`, the way a user would, with
    /// this checkout's saltc first on PATH. sp consults PATH only after
    /// searching upward from the project, which finds no saltc under the
    /// temp dir, and after SALT_REPO_ROOT, which is cleared.
    fn sp(&self, dir: &str, args: &[&str]) -> Output {
        let saltc_dir = saltc().parent().unwrap().to_path_buf();
        let path = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(
            std::iter::once(saltc_dir).chain(std::env::split_paths(&path)),
        )
        .unwrap();
        Command::new(env!("CARGO_BIN_EXE_sp"))
            .args(args)
            .current_dir(self.root.join(dir))
            .env("HOME", self.root.join("home"))
            .env("PATH", path)
            .env_remove("SALT_REPO_ROOT")
            .output()
            .expect("failed to spawn sp")
    }

    fn cleanup(self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_success(out: &Output, what: &str) {
    assert!(
        out.status.success(),
        "{} failed ({}):\nstdout:\n{}\nstderr:\n{}",
        what,
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `sp build` writes saltc's MLIR to target/debug/<name>. A definition of
/// the function (a `func.func` with a body, not a call or a declaration)
/// proves the imported module was loaded and compiled in.
fn assert_defines(mlir: &str, symbol: &str) {
    let signature = format!("@{}(", symbol);
    let defined = mlir
        .lines()
        .map(str::trim)
        .any(|l| l.starts_with("func.func") && l.contains(&signature) && l.ends_with('{'));
    assert!(defined, "expected a definition of @{} in the build output:\n{}", symbol, mlir);
}

fn app_manifest(dependencies: &str) -> String {
    format!(
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n{}\n",
        dependencies
    )
}

#[test]
fn build_imports_path_dependency() {
    let sb = Sandbox::new("path_dep");
    sb.write("mylib/salt.toml", MYLIB_MANIFEST);
    sb.write("mylib/src/lib.salt", MYLIB_SRC);
    sb.write("app/salt.toml", &app_manifest("mylib = { path = \"../mylib\" }"));
    sb.write("app/src/main.salt", APP_MAIN);

    assert_success(&sb.sp("app", &["build"]), "sp build");
    assert_defines(&sb.read("app/target/debug/app"), "mylib__add");

    sb.cleanup();
}

#[test]
fn build_imports_version_dependency() {
    let sb = Sandbox::new("version_dep");
    sb.write("mylib/salt.toml", MYLIB_MANIFEST);
    sb.write("mylib/src/lib.salt", MYLIB_SRC);
    assert_success(&sb.sp("mylib", &["publish"]), "sp publish");

    sb.write("app/salt.toml", &app_manifest("mylib = \"1.0\""));
    sb.write("app/src/main.salt", APP_MAIN);

    assert_success(&sb.sp("app", &["build"]), "sp build");
    assert_defines(&sb.read("app/target/debug/app"), "mylib__add");

    sb.cleanup();
}

/// The library `sp new --lib` scaffolds is importable as a dependency as-is.
#[test]
fn build_imports_scaffolded_library() {
    let sb = Sandbox::new("scaffolded_lib");
    assert_success(&sb.sp(".", &["new", "mylib", "--lib"]), "sp new --lib");
    sb.write("app/salt.toml", &app_manifest("mylib = { path = \"../mylib\" }"));
    sb.write("app/src/main.salt", APP_MAIN);

    assert_success(&sb.sp("app", &["build"]), "sp build");
    assert_defines(&sb.read("app/target/debug/app"), "mylib__add");

    sb.cleanup();
}

/// app -> mid -> mylib: mid's own `use mylib.add` must resolve too.
#[test]
fn build_imports_transitive_path_dependency() {
    let sb = Sandbox::new("transitive_dep");
    sb.write("mylib/salt.toml", MYLIB_MANIFEST);
    sb.write("mylib/src/lib.salt", MYLIB_SRC);
    sb.write(
        "mid/salt.toml",
        "[package]\nname = \"mid\"\nversion = \"0.1.0\"\nentry = \"src/lib.salt\"\n\n\
         [dependencies]\nmylib = { path = \"../mylib\" }\n",
    );
    sb.write(
        "mid/src/lib.salt",
        "package mid\n\nuse mylib.add\n\npub fn add_twice(a: i32, b: i32) -> i32 {\n    return add(add(a, b), b);\n}\n",
    );
    sb.write("app/salt.toml", &app_manifest("mid = { path = \"../mid\" }"));
    sb.write(
        "app/src/main.salt",
        "package main\n\nuse mid.add_twice\n\nfn main() -> i32 {\n    return add_twice(2, 3);\n}\n",
    );

    assert_success(&sb.sp("app", &["build"]), "sp build");
    let mlir = sb.read("app/target/debug/app");
    assert_defines(&mlir, "mid__add_twice");
    assert_defines(&mlir, "mylib__add");

    sb.cleanup();
}

/// A project's own src/ is a search root even when sp runs from the
/// project directory rather than from inside src/.
#[test]
fn build_imports_sibling_module() {
    let sb = Sandbox::new("sibling_module");
    sb.write("app/salt.toml", "[package]\nname = \"app\"\nversion = \"0.1.0\"\n");
    sb.write(
        "app/src/util.salt",
        "package util\n\npub fn twice(x: i32) -> i32 {\n    return x + x;\n}\n",
    );
    sb.write(
        "app/src/main.salt",
        "package main\n\nuse util.twice\n\nfn main() -> i32 {\n    return twice(21);\n}\n",
    );

    assert_success(&sb.sp("app", &["build"]), "sp build");
    assert_defines(&sb.read("app/target/debug/app"), "util__twice");

    sb.cleanup();
}

// `sp check` runs saltc with --lib, which lowers only `pub` functions: with a
// private `main` as the only caller, an unresolved import goes unnoticed and
// the check passes vacuously. Hence the `pub` callers below.

#[test]
fn check_imports_path_dependency() {
    let sb = Sandbox::new("check_path_dep");
    sb.write("mylib/salt.toml", MYLIB_MANIFEST);
    sb.write("mylib/src/lib.salt", MYLIB_SRC);
    sb.write("app/salt.toml", &app_manifest("mylib = { path = \"../mylib\" }"));
    sb.write(
        "app/src/main.salt",
        "package main\n\nuse mylib.add\n\npub fn five() -> i32 {\n    return add(2, 3);\n}\n",
    );

    assert_success(&sb.sp("app", &["check"]), "sp check");

    sb.cleanup();
}

#[test]
fn check_enforces_dependency_contract() {
    let sb = Sandbox::new("check_dep_contract");
    sb.write("mylib/salt.toml", MYLIB_MANIFEST);
    sb.write(
        "mylib/src/lib.salt",
        "package mylib\n\npub fn checked_div(a: i32, b: i32) -> i32 requires { b != 0 } {\n    return a / b;\n}\n",
    );
    sb.write("app/salt.toml", &app_manifest("mylib = { path = \"../mylib\" }"));
    sb.write(
        "app/src/main.salt",
        "package main\n\nuse mylib.checked_div\n\npub fn broken() -> i32 {\n    return checked_div(1, 0);\n}\n",
    );

    let out = sb.sp("app", &["check"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "sp check accepted a call violating the dependency's contract");
    assert!(stderr.contains("[E009]"), "expected a contract violation (E009), got:\n{}", stderr);

    sb.cleanup();
}
