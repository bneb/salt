// Regression test: QUALIFIED ENUM IDENTITY AT EMISSION for imported
// generic enums (round-1 remaining-work item #2).
//
// The bug: an item-level stdlib import (`use std.core.result.Result;`)
// made ModuleLoader register the bundled module under the ITEM path
// (`std.core.result.Result`) instead of the source's own package. That
// phantom ModuleInfo re-mangled every enum template into a DOUBLED key
// (`std__core__result__Result__Result`), so two resolvers disagreed:
// constructor emission specialized the single canonical form while
// signature/match handling resolved bare names to the doubled ghost —
// producing two distinct LLVM struct types for one enum (and, before
// earlier fixes, hard failures like `Unknown enum ...Result__Err__...`).
//
// The fix under test: the loader registers bundled sources under the
// bundle key matching their `package` declaration; both resolution paths
// now converge on one specialized identity per enum instantiation.
//
// Only the stdlib half exercises that fix. The project-local half resolves
// through the filesystem import path, which never took the bundled-stdlib
// branch; it pins the same one-identity property on that other path.
use saltc::compile;
use std::collections::BTreeSet;

const SINGLE_FORM: &str = "struct_std__core__result__Result_i64";
const DOUBLED_GHOST: &str = "Result__Result_i64";

/// Distinct `!llvm.struct<"...">` identities in `mlir` whose name contains
/// `needle`. Asserting this set is exactly {canonical} catches a split
/// identity under any spelling, not just the ghost spellings listed by name.
fn struct_identities<'a>(mlir: &'a str, needle: &str) -> BTreeSet<&'a str> {
    mlir.match_indices("!llvm.struct<\"")
        .filter_map(|(at, open)| {
            let rest = &mlir[at + open.len()..];
            rest.find('"').map(|end| &rest[..end])
        })
        .filter(|name| name.contains(needle))
        .collect()
}

// Harness for the project-local half of this suite.
//
// Unlike the stdlib half, a non-stdlib import resolves through
// `ModuleLoader::resolve_filepath` (filesystem) rather than the bundled-source
// table, so nothing serves `target.enum_emission_it.boxen` unless a file
// exists. Same convention as cross_module_trait_defaults_test.rs and
// trait_qualified_identity_test.rs: write the module under the gitignored
// `target/` tree, which is a module-loader search root via the compiler's
// cwd-based roots.
//
// The fixture MUST be written by the test, not committed: `target/` is
// gitignored, so a checked-in copy cannot survive a clone.
//
// NOT slug-scoped per-test like cross_module_trait_defaults_test.rs /
// trait_qualified_identity_test.rs's write_module(slug, ..): cleanup() below
// wipes the whole IT_ROOT tree, and #[test] fns run in parallel by default
// (no serial_test dep, no test-threads config in this repo). Only one test
// here uses this fixture, so threads within one `cargo test` run can't
// collide on it. If you add a second fixture-based test here, adopt the
// sibling files' slug parameter first, or the two tests' writes/cleanups
// will race.
const IT_ROOT: &str = "target/enum_emission_it";

/// Generic enum + two associated functions, imported item-wise by the test.
const BOXEN_SRC: &str = r#"
package target.enum_emission_it.boxen

pub enum Box2<T> {
    Packed(T),
    Empty
}

pub fn make_packed(n: i64) -> Box2<i64> {
    return Box2::Packed(n);
}

pub fn unpack(b: Box2<i64>) -> i64 {
    match b {
        Box2::Packed(inner) => return inner,
        Box2::Empty => return 0
    }
}
"#;

fn it_dir() -> std::path::PathBuf {
    std::env::current_dir().expect("cwd available").join(IT_ROOT)
}

/// Writes the `boxen` module reachable as `target.enum_emission_it.boxen`.
fn write_boxen() {
    let dir = it_dir();
    std::fs::create_dir_all(&dir).expect("create module dir");
    std::fs::write(dir.join("boxen.salt"), BOXEN_SRC).expect("write module");
}

fn cleanup() {
    let _ = std::fs::remove_dir_all(it_dir());
}

/// Constructing, returning, matching and dispatching an imported generic
/// enum must use exactly ONE specialized type identity end to end.
#[test]
fn imported_result_uses_single_specialized_identity() {
    let src = r#"
        package main

        use std.core.result.Result;
        use std.status.Status;

        fn parse(v: i64) -> Result<i64> {
            if v > 0 {
                return Result::Ok(v);
            }
            return Result::Err(Status::from_code(22));
        }

        pub fn main() -> i32 {
            let r = parse(5);
            let out = match r {
                Ok(val) => val,
                Err(_) => 0,
            };
            return out as i32;
        }
    "#;
    let result = compile(src, false, None, true);
    assert!(result.is_ok(), "imported Result ctor/match failed: {:?}", result.err());
    let mlir = result.unwrap();

    assert!(mlir.contains(SINGLE_FORM), "canonical specialized form missing");
    assert!(!mlir.contains(DOUBLED_GHOST),
            "doubled-leaf ghost type must not exist alongside the canonical form");
    assert_eq!(struct_identities(&mlir, "Result_i64"),
               BTreeSet::from(["std__core__result__Result_i64"]),
               "Result<i64> must have exactly one LLVM struct identity");
}

/// Same convergence requirement for a project-local imported generic enum
/// (non-stdlib path): ctor, signature and match must share one identity.
#[test]
fn local_imported_generic_enum_identity_converges() {
    write_boxen();
    let src = r#"
        package main

        use target.enum_emission_it.boxen.Box2;
        use target.enum_emission_it.boxen.make_packed;
        use target.enum_emission_it.boxen.unpack;

        pub fn main() -> i32 {
            let b = make_packed(7);
            let x = unpack(b);
            let local = Box2::<i64>::Packed(9);
            let y = match local {
                Box2::Packed(inner) => inner,
                Box2::Empty => 1,
            };
            return ((x + y) % 2) as i32;
        }
    "#;
    let result = compile(src, false, None, true);
    cleanup();
    assert!(result.is_ok(), "local imported Box2 failed: {:?}", result.err());
    let mlir = result.unwrap();

    assert!(mlir.contains("struct_target__enum_emission_it__boxen__Box2_i64"),
            "specialized Box2_i64 identity missing");
    let spellings = ["Box2__Box2_i64", "Box2_i64_i64"];
    for ghost in spellings {
        assert!(!mlir.contains(ghost), "ghost spelling {} must not appear", ghost);
    }
    assert_eq!(struct_identities(&mlir, "Box2"),
               BTreeSet::from(["target__enum_emission_it__boxen__Box2_i64"]),
               "Box2<i64> must have exactly one LLVM struct identity");
}
