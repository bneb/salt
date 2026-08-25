// Regression tests: const-generic identities must be VALUE-keyed
// (Cache_64 / Cache_128) everywhere, and param-name-spelled ghost
// identities must not appear anywhere in the emitted module — neither in
// ops nor as vestigial top-level type declarations.
//
// Locked mechanisms:
//   * codegen::scoped_generic_hydration — pre-scan signature registration
//     parses with impl/fn generic params hydrated, so specialize_template's
//     Generic Guard refuses phantom registrations (handoff rounds 33-34).
//   * codegen::bind_signature_placeholders — stored signatures keep
//     declared params as substitutable placeholders.
use saltc::compile;

const PRIMARY_REPRO: &str = r#"
    package main

    struct Cache<const SIZE: i64> { next: i64 }

    impl<const SIZE: i64> Cache<SIZE> {
        pub fn new() -> Cache<SIZE> {
            return Cache { next: SIZE };
        }
    }

    pub fn main() -> i32 {
        let a = Cache::<64>::new();
        let b = Cache::<128>::new();
        return (a.next + b.next) as i32;
    }
"#;

const EMPTY_IMPL_REPRO: &str = r#"
    package main

    struct Cache<const SIZE: i64> { next: i64 }

    impl<const SIZE: i64> Cache<SIZE> {}

    pub fn main() -> i32 { return 0; }
"#;

const CROSS_PARAM_FN_REPRO: &str = r#"
    package main

    struct Cache<const SIZE: i64> { next: i64 }

    pub fn take<const K: i64>(x: Cache<K>) -> i64 { return x.next; }

    pub fn main() -> i32 { return 7; }
"#;

fn assert_no_size_spelled_identity(mlir: &str) {
    for ghost in ["Cache_SIZE", "main__SIZE", "Cache__new_SIZE"] {
        assert!(!mlir.contains(ghost), "param-name ghost {} leaked", ghost);
    }
}

#[test]
fn const_generic_ctor_identities_are_value_keyed() {
    let result = compile(PRIMARY_REPRO, false, None, true);
    assert!(result.is_ok(), "primary repro failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(mlir.contains("Cache__new_64"), "value-keyed ctor symbol missing");
    assert!(mlir.contains("Cache__new_128"), "value-keyed ctor symbol missing");
    assert!(mlir.contains("!struct_main__Cache_64 "), "value-keyed struct identity missing");
    assert_no_size_spelled_identity(&mlir);
}

#[test]
fn empty_const_impl_mints_no_phantom_decl() {
    // Zero call sites: the phantom used to be minted purely by pre-scan
    // registration of the impl target type (handoff rt_p3 probe).
    let result = compile(EMPTY_IMPL_REPRO, false, None, true);
    assert!(result.is_ok(), "empty impl repro failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert_no_size_spelled_identity(&mlir);
}

#[test]
fn cross_param_fn_signature_mints_no_ghost() {
    // K belongs to the FUNCTION, not to Cache — the guard must refuse the
    // registration regardless of which item declared the param
    // (handoff rt_p7/p7b probes).
    let result = compile(CROSS_PARAM_FN_REPRO, false, None, true);
    assert!(result.is_ok(), "cross-param repro failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(!mlir.contains("Cache_K"), "cross-param ghost leaked");
}

#[test]
fn xmod_const_generic_identity_stays_value_keyed() {
    const ENTRY: &str = r#"
        package main

        use tests.fixtures.const_xmod.cache.Cache;

        pub fn main() -> i32 {
            let a = Cache::<64>::new();
            let b = Cache::<128>::new();
            return (a.next + b.next) as i32;
        }
    "#;
    let result = compile(ENTRY, false, None, true);
    assert!(result.is_ok(), "xmod repro failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(mlir.contains("Cache__new_64"), "xmod value-keyed symbol missing");
    assert!(mlir.contains("Cache__new_128"), "xmod value-keyed symbol missing");
    assert_no_size_spelled_identity(&mlir);
}
