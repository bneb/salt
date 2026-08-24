// Regression tests: SAME-NAMED TRAITS IN DIFFERENT MODULES must not
// cross-inherit default bodies.
//
// The bug: `expand_trait_defaults` keyed default methods by the trait's
// BARE name in one flat map. With `alpha.Config` and `beta.Config` both
// defining defaults, collection order (sorted namespaces, entry last)
// let beta's table entry clobber alpha's — every implementor of ANY
// `Config` inherited beta's body. The classic probe: alpha's impl
// returned beta's constant 222222 instead of its own 111111.
//
// The fix under test: defaults are keyed by namespace-qualified identity
// (`package path` + trait name) and each impl resolves its trait through
// the file CONTAINING the impl (local definitions, then that file's own
// imports). Fixtures live in tests/fixtures/trait_collision/ and resolve
// via the loader's cwd search root exactly like production imports.
use saltc::compile;

const ALPHA_SENTINEL: &str = "111111";
const BETA_SENTINEL: &str = "222222";

fn collision_entry() -> String {
    r#"
        package main

        use tests.fixtures.trait_collision.alpha.AlphaCfg;
        use tests.fixtures.trait_collision.beta.BetaCfg;

        pub fn main() -> i32 {
            let a = AlphaCfg { v: 1 };
            let b = BetaCfg { v: 2 };
            let x = a.value();
            let y = b.value();
            return 0;
        }
    "#.to_string()
}

/// Both same-named defaults must reach MLIR with their OWN constant.
/// Before the fix this failed on the first assert: alpha inherited
/// beta's 222222 and no 111111 existed anywhere in the output.
#[test]
fn colliding_traits_keep_their_own_default_bodies() {
    let result = compile(&collision_entry(), false, None, true);
    assert!(result.is_ok(), "collision compile failed: {:?}", result.err());
    let mlir = result.unwrap();

    assert!(mlir.contains(ALPHA_SENTINEL), "alpha's own default body missing from MLIR");
    assert!(mlir.contains(BETA_SENTINEL), "beta's own default body missing from MLIR");
}

/// Each implementor dispatches to ITS module's impl symbol, and each
/// impl method is defined exactly once (no cross-module duplication).
#[test]
fn colliding_impls_emit_per_module_symbols() {
    let result = compile(&collision_entry(), false, None, true);
    assert!(result.is_ok(), "collision compile failed: {:?}", result.err());
    let mlir = result.unwrap();

    for (pkg, ty) in [("alpha", "AlphaCfg"), ("beta", "BetaCfg")] {
        let symbol = format!("tests__fixtures__trait_collision__{}__{}__value", pkg, ty);
        assert!(mlir.contains(&format!("func.call @{}", symbol)),
                "entry must dispatch to {}", symbol);
        let defs = mlir.lines()
            .filter(|l| l.contains("func.func") && l.contains(&symbol))
            .count();
        assert_eq!(defs, 1, "{} must be defined exactly once, got {}", symbol, defs);
    }
}
