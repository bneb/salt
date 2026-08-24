// Regression tests: INHERITED TRAIT DEFAULT METHODS across MODULES.
//
// Locks the end-to-end behavior for implementors that live inside
// IMPORTED MODULES rather than the entry file. This works because
// expand_trait_defaults (called from emit_mlir after load_modules)
// expands every AST in loader.loaded_files BEFORE signature
// registration; init_registry_impls' ModuleInfo.impls snapshots are
// stale by then but only re-register non-default methods afterwards.
// If anyone reorders those phases or "deduplicates" registration,
// these tests must fail loudly instead of breaking user code.
//
// Fixtures live in tests/fixtures/trait_defaults/ and resolve via the
// loader's default search roots relative to the manifest directory
// (cargo test runs test binaries with cwd = CARGO_MANIFEST_DIR).
use saltc::compile;

const DEFAULT_SENTINEL: &str = "8888888";
const OVERRIDE_SENTINEL: &str = "9999999";
const PT_SENTINEL: &str = "4242424";

fn cross_module_entry() -> String {
    r#"
        package main

        use tests.fixtures.trait_defaults.impls.Dog;
        use tests.fixtures.trait_defaults.impls.Cat;
        use tests.fixtures.trait_defaults.impls.dog_hello;

        pub fn main() -> i32 {
            let dog = Dog { n: 3 };
            let x = dog.hello();
            let l = dog.label();
            let cat = Cat { n: 5 };
            let y = cat.hello();
            let z = dog_hello();
            return 0;
        }
    "#.to_string()
}

/// Trait in one fixture module, impls omitting BOTH defaults in another;
/// entry calls them. Inherited defaults must resolve and be emitted under
/// the implementing type's mangled name, including a default body that
/// constructs the trait-home-module `Pt` struct type.
#[test]
fn module_impl_omitting_defaults_resolves_and_emits() {
    let result = compile(&cross_module_entry(), false, None, true);
    assert!(result.is_ok(), "cross-module inherited call failed: {:?}", result.err());
    let mlir = result.unwrap();

    assert!(mlir.contains("impls__Dog__hello"), "Dog__hello symbol missing");
    assert!(mlir.contains(DEFAULT_SENTINEL), "inherited default body missing");

    // Default body referencing the trait's HOME-MODULE struct type.
    assert!(mlir.contains("impls__Dog__label"), "Dog__label symbol missing");
    assert!(mlir.contains("trait_defaults__greet__Pt"), "home-module Pt type missing");
    assert!(mlir.contains(PT_SENTINEL), "label default body sentinel missing");
}

/// An impl in another module overriding a defaulted method: the override
/// body wins and exactly ONE definition of the overriding method exists.
#[test]
fn module_override_wins_across_module_boundary() {
    let result = compile(&cross_module_entry(), false, None, true);
    assert!(result.is_ok(), "cross-module override failed: {:?}", result.err());
    let mlir = result.unwrap();

    assert!(mlir.contains(OVERRIDE_SENTINEL), "override body must be emitted");
    let defs = mlir.lines()
        .filter(|l| l.contains("func.func") && l.contains("impls__Cat__hello"))
        .count();
    assert_eq!(defs, 1, "Cat__hello must be defined exactly once, got {}", defs);
}

/// A call to an omitted-default method made INSIDE module code (not the
/// entry file) resolves through the same registry path.
#[test]
fn module_internal_call_uses_inherited_default() {
    let result = compile(&cross_module_entry(), false, None, true);
    assert!(result.is_ok(), "module-internal inherited call failed: {:?}", result.err());
    let mlir = result.unwrap();

    assert!(mlir.contains("impls__dog_hello"), "fixture helper missing");
    assert!(mlir.contains("call @tests__fixtures__trait_defaults__impls__Dog__hello"),
            "helper must dispatch to inherited Dog__hello");
}
