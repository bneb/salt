// `--root <dir>` and `--dep <name>=<file>` through the saltc binary: both
// import resolvers (cli.rs load_imports, which emits W008, and ModuleLoader,
// which feeds codegen) must find the modules the flags point at. Each test
// first compiles WITHOUT the flag and expects the import to fail, so a pass
// can't come from the cwd-relative built-in roots instead.
use std::path::{Path, PathBuf};
use std::process::Output;

/// A scratch directory for one test under the gitignored target/ tree,
/// emptied first so reruns start clean.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("module_search_flags").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// Compiles `entry` to `<dir>/out.mlir` from `dir` with `flags` appended.
fn saltc(dir: &Path, entry: &Path, flags: &[String]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_saltc"))
        .current_dir(dir)
        .arg(entry)
        .arg("-o")
        .arg(dir.join("out.mlir"))
        .args(flags)
        .output()
        .expect("failed to spawn saltc")
}

fn report(out: &Output) -> String {
    format!(
        "{}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Asserts the build failed with imports from `module` unresolved.
fn assert_unresolved(out: &Output, module: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success() && stderr.lines().any(|l| l.contains("[W008]") && l.contains(module)),
        "expected '{}' to go unresolved without the flag: {}",
        module,
        report(out)
    );
}

/// Asserts the build succeeded with no W008 for `module`, and that the
/// output defines each of `symbols` (a `func.func` with a body).
fn assert_resolved(dir: &Path, out: &Output, module: &str, symbols: &[&str]) {
    assert!(out.status.success(), "saltc failed: {}", report(out));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.lines().any(|l| l.contains("[W008]") && l.contains(module)),
        "load_imports didn't find '{}': {}",
        module,
        report(out)
    );
    let mlir = std::fs::read_to_string(dir.join("out.mlir")).unwrap();
    for symbol in symbols {
        let signature = format!("@{}(", symbol);
        assert!(
            mlir.lines().map(str::trim).any(|l| l.starts_with("func.func") && l.contains(&signature) && l.ends_with('{')),
            "expected a definition of @{} in:\n{}",
            symbol,
            mlir
        );
    }
}

#[test]
fn dep_flag_resolves_package_root_module_and_submodule() {
    let dir = scratch("dep");
    let root_module = dir.join("mylib/src/lib.salt");
    write(&root_module, "package mylib\n\npub fn add(a: i32, b: i32) -> i32 {\n    return a + b;\n}\n");
    write(
        &dir.join("mylib/src/util.salt"),
        "package mylib.util\n\npub fn triple(x: i32) -> i32 {\n    return x + x + x;\n}\n",
    );
    let entry = dir.join("app/main.salt");
    write(
        &entry,
        "package main\n\nuse mylib.add\nuse mylib.util.triple\n\nfn main() -> i32 {\n    return triple(add(1, 2));\n}\n",
    );

    assert_unresolved(&saltc(&dir, &entry, &[]), "mylib");

    let dep = format!("mylib={}", root_module.display());
    let out = saltc(&dir, &entry, &["--dep".to_string(), dep]);
    assert_resolved(&dir, &out, "mylib", &["mylib__add", "mylib__util__triple"]);
}

#[test]
fn root_flag_resolves_module_under_root() {
    let dir = scratch("root");
    write(
        &dir.join("app/src/util.salt"),
        "package util\n\npub fn twice(x: i32) -> i32 {\n    return x + x;\n}\n",
    );
    let entry = dir.join("app/src/main.salt");
    write(&entry, "package main\n\nuse util.twice\n\nfn main() -> i32 {\n    return twice(21);\n}\n");

    assert_unresolved(&saltc(&dir, &entry, &[]), "util");

    let root = dir.join("app/src").display().to_string();
    let out = saltc(&dir, &entry, &["--root".to_string(), root]);
    assert_resolved(&dir, &out, "util", &["util__twice"]);
}

/// A `--dep` package owns its namespace: a same-named module reachable
/// through the cwd-relative roots stops loading once the package is declared.
/// `load_modules` collects every unresolved import and fails with `[E008]`
/// (naming the namespace), rather than reaching codegen's undefined-symbol
/// check at all.
#[test]
fn dep_flag_confines_package_namespace() {
    let dir = scratch("dep_confined");
    let root_module = dir.join("mylib/src/lib.salt");
    write(&root_module, "package mylib\n");
    // Beside the package, not in it: <root module's dir>/extra.salt is where mylib.extra lives.
    write(
        &dir.join("mylib/extra.salt"),
        "package mylib.extra\n\npub fn seven() -> i32 {\n    return 7;\n}\n",
    );
    let entry = dir.join("app/main.salt");
    write(&entry, "package main\n\nuse mylib.extra.seven\n\nfn main() -> i32 {\n    return seven();\n}\n");

    let out = saltc(&dir, &entry, &[]);
    assert_resolved(&dir, &out, "mylib", &["mylib__extra__seven"]);

    let dep = format!("mylib={}", root_module.display());
    let out = saltc(&dir, &entry, &["--dep".to_string(), dep]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success()
            && stderr.contains("[E008]")
            && stderr.contains("mylib.extra.seven")
            && stderr.contains("in its --dep package"),
        "the module outside the package must not load: {}",
        report(&out)
    );
}
