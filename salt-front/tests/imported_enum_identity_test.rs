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
use saltc::compile;

const SINGLE_FORM: &str = "struct_std__core__result__Result_i64";
const DOUBLED_GHOST: &str = "Result__Result_i64";

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
}

/// Same convergence requirement for a project-local imported generic enum
/// (non-stdlib path): ctor, signature and match must share one identity.
#[test]
fn local_imported_generic_enum_identity_converges() {
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
    assert!(result.is_ok(), "local imported Box2 failed: {:?}", result.err());
    let mlir = result.unwrap();

    assert!(mlir.contains("struct_target__enum_emission_it__boxen__Box2_i64"),
            "specialized Box2_i64 identity missing");
    let spellings = ["Box2__Box2_i64", "Box2_i64_i64"];
    for ghost in spellings {
        assert!(!mlir.contains(ghost), "ghost spelling {} must not appear", ghost);
    }
}
