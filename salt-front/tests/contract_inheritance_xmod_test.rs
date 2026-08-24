// WS-1 hardening: inherited trait-default contracts must ENFORCE through
// module imports -- the default body and its requires clause are cloned
// into the module impl by expand_trait_defaults, and call sites in OTHER
// modules must see the contract (satisfying calls compile; violating
// literals are rejected with [E009] counterexample).
use saltc::compile;

const ENTRY_OK: &str = r#"
    package main

    use tests.fixtures.contract_xmod.guarded.Guarded;
    use tests.fixtures.contract_xmod.guarded.Widget;

    pub fn main() -> i32 {
        let w = Widget { v: 1 };
        let r = w.wrapped(5);
        return r as i32;
    }
"#;

const ENTRY_VIOLATING: &str = r#"
    package main

    use tests.fixtures.contract_xmod.guarded.Guarded;
    use tests.fixtures.contract_xmod.guarded.Widget;

    pub fn main() -> i32 {
        let w = Widget { v: 1 };
        let r = w.wrapped(-3);
        return r as i32;
    }
"#;

#[test]
fn cross_module_inherited_default_enforces_contract() {
    let ok = compile(ENTRY_OK, false, None, true);
    assert!(ok.is_ok(), "satisfying cross-module call failed: {:?}", ok.err());
    let mlir = ok.unwrap();
    assert!(mlir.contains("tests__fixtures__contract_xmod__guarded__Widget__wrapped"),
            "inherited default must be emitted under the implementing module");
}

#[test]
fn cross_module_violating_call_rejected() {
    let result = compile(ENTRY_VIOLATING, false, None, true);
    let err = format!("{}", result.expect_err("violating call must be rejected"));
    assert!(err.contains("E009") || err.contains("could not prove"),
            "expected contract rejection, got: {}", err);
}

// Stage-2 semantic refinement THROUGH module imports: same clause count,
// weaker meaning (`k != 0` admits negatives that `k > 0` forbids).
#[test]
fn cross_module_semantic_weakening_rejected() {
    let src = r#"
        package main

        use tests.fixtures.contract_xmod.guarded.Guarded;
        use tests.fixtures.contract_xmod.guarded.Widget;

        impl Guarded for Widget {
            fn wrapped(&self, k: i64) -> i64
                requires(k != 0)
            {
                return self.core(k);
            }
        }

        pub fn main() -> i32 {
            let w = Widget { v: 1 };
            return w.wrapped(-5) as i32;
        }
    "#;
    let result = compile(src, false, None, true);
    let err = format!("{}", result.expect_err("semantic weakening must be rejected"));
    assert!(err.contains("weakens"), "expected weakening diagnostic, got: {}", err);
}
