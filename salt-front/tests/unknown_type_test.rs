// saltc must refuse a type name that nothing declares instead of lowering
// it to an MLIR alias with no definition. Box is not in the prelude
// (docs/SPEC.md, section 12: Ptr, Option, Result, Status, DefaultAllocator
// and print), so without `use std.core.boxed.Box;` saltc emitted
// `!struct_Box_i32` with no definition, and salt-opt failed with
// "undefined symbol alias id 'struct_Box_i32'".
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Output;

/// salt-opt/tests/opt_option_test.salt without its `use std.core.boxed.Box;`.
const OPTION_OF_BOX: &str = r#"
fn is_some(opt: Option<Box<i32>>) -> i32 {
    match opt {
        Some(x) => { return 1; }
        None => { return 0; }
    }
}

fn make_some(v: Box<i32>) -> Option<Box<i32>> {
    return Some(v)
}

fn make_none() -> Option<Box<i32>> {
    return None
}

fn main(argc: i32, argv: Box<i32>) -> i32 {
    let s = is_some(make_some(argv));
    let n = is_some(make_none());
    return s * 10 + n
}
"#;

/// A scratch directory under the gitignored target/ tree, emptied first.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("unknown_type").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs the saltc binary on `source`, writing `<dir>/out.mlir`.
fn saltc(dir: &Path, source: &str) -> Output {
    let entry = dir.join("main.salt");
    std::fs::write(&entry, source).unwrap();
    std::process::Command::new(env!("CARGO_BIN_EXE_saltc"))
        .current_dir(dir)
        .arg(&entry)
        .arg("-o")
        .arg(dir.join("out.mlir"))
        .output()
        .expect("failed to spawn saltc")
}

/// Type aliases the module uses without a `!name = ...` definition. MLIR
/// reads `!name` as an alias only when the name has no dot: `!llvm.ptr` is
/// a dialect type.
fn undefined_aliases(mlir: &str) -> Vec<String> {
    let code: Vec<String> = mlir.lines().map(strip_strings_and_comment).collect();
    let defined: HashSet<String> = code
        .iter()
        .filter_map(|l| l.strip_prefix('!')?.split_once(" = ").map(|(name, _)| name.to_string()))
        .collect();
    let mut undefined: Vec<String> = code
        .iter()
        .flat_map(|l| l.split('!').skip(1).map(alias_name))
        .filter(|name| !name.is_empty() && !name.contains('.') && !defined.contains(name))
        .collect();
    undefined.sort();
    undefined.dedup();
    undefined
}

fn alias_name(after_bang: &str) -> String {
    after_bang.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$')).collect()
}

/// `line` without string literal contents or a trailing `//` comment.
fn strip_strings_and_comment(line: &str) -> String {
    let mut out = String::new();
    let mut in_string = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (in_string, c) {
            (true, '\\') => { chars.next(); }
            (true, '"') => in_string = false,
            (true, _) => {}
            (false, '"') => in_string = true,
            (false, '/') if chars.clone().next() == Some('/') => break,
            (false, _) => out.push(c),
        }
    }
    out
}

fn compile_error(source: &str) -> String {
    match saltc::compile(source, false, None, true) {
        Ok(_) => panic!("compiled, but an unknown type was expected:\n{}", source),
        Err(e) => e.to_string(),
    }
}

/// The reported program, through the binary the salt-opt harness runs.
#[test]
fn box_without_its_import_is_an_unknown_type() {
    let dir = scratch("box_without_import");
    let out = saltc(&dir, OPTION_OF_BOX);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "saltc accepted Box without its import:\n{}", stderr);
    assert!(stderr.contains("unknown type `Box` in fn `is_some`"), "stderr:\n{}", stderr);
    assert!(!dir.join("out.mlir").exists(), "saltc wrote MLIR for a program it rejected");
}

/// With the import, every alias the MLIR uses is defined, Box's included.
#[test]
fn box_with_its_import_lowers_to_defined_aliases() {
    let dir = scratch("box_with_import");
    let out = saltc(&dir, &format!("use std.core.boxed.Box;\n{}", OPTION_OF_BOX));
    assert!(out.status.success(), "saltc failed:\n{}", String::from_utf8_lossy(&out.stderr));
    let mlir = std::fs::read_to_string(dir.join("out.mlir")).unwrap();
    assert!(mlir.contains("\n!struct_std__core__boxed__Box_i32 = "), "Box_i32 is not defined:\n{}", mlir);
    assert_eq!(undefined_aliases(&mlir), Vec::<String>::new(), "MLIR:\n{}", mlir);
}

/// Holder's definition used to embed an undefined `!struct_Frob_i32`,
/// though no function touched Holder.
#[test]
fn unknown_field_type_is_rejected() {
    let err = compile_error("struct Holder { b: Frob<i32> }\nfn main() -> i32 {\n    return 0;\n}\n");
    assert!(err.contains("unknown type `Frob` in struct `Holder`"), "{}", err);
}

/// Nested in Option, Frob never reached the MLIR by name: the program
/// compiled, with a payload size guessed for a type that doesn't exist.
#[test]
fn unknown_type_argument_is_rejected() {
    let err = compile_error(
        "fn f(o: Option<Frob<i32>>) -> i32 {\n    return 3;\n}\nfn main() -> i32 {\n    return f(None);\n}\n",
    );
    assert!(err.contains("unknown type `Frob` in fn `f`"), "{}", err);
}
