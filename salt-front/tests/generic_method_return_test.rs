// Regression test: RENAMED IMPL PARAMS on generic impls (item 6 edge).
//
// The bug: `impl<T> Pair<A>` renames the struct's param; method hydration
// bound only the STRUCT template's names (A -> I64), so a body returning
// the impl name (`fn get_first(&self) -> T`) hydrated with the raw
// placeholder -- later casts failed with
// "Unsupported explicit cast T -> i32".
//
// Fix chain: pre-scan registration now stores the MERGED wrapper
// (impl generics folded into fn generics) so the rank-0 registry entry
// carries the impl names; request_specialization binds them positionally
// to the receiver's concrete args; emit-side refinement catches any
// residual bare placeholders.
use saltc::compile;

#[test]
fn renamed_impl_param_method_returns_concrete_type() {
    let src = r#"
        package main

        struct Pair<A> { first: A }

        impl<T> Pair<T> {
            pub fn get_first(&self) -> T { return self.first; }
        }

        pub fn main() -> i32 {
            let p1 = Pair::<i64> { first: 10 };
            let p2 = Pair::<f32> { first: 0.5 };
            let a = p1.get_first();
            let b = p2.get_first();
            return (a as i32) + (b as i32);
        }
    "#;
    let result = compile(src, false, None, true);
    assert!(result.is_ok(), "renamed impl param method failed: {:?}", result.err());
    let mlir = result.unwrap();

    // Both instantiations hydrate with concrete types.
    assert!(mlir.contains("Pair_i64__get_first"), "i64 instance missing");
    assert!(mlir.contains("Pair_f32__get_first"), "f32 instance missing");
}

/// Identity case (`impl<A> Pair<A>` -- impl param IS the struct param)
/// must be unaffected by the merged-registration change.
#[test]
fn identity_impl_param_case_still_compiles() {
    let src = r#"
        package main

        struct Wrap<A> { v: A }

        impl<A> Wrap<A> {
            pub fn value(&self) -> A { return self.v; }
        }

        pub fn main() -> i32 {
            let w = Wrap::<i64> { v: 3 };
            let x = w.value();
            return x as i32;
        }
    "#;
    let result = compile(src, false, None, true);
    assert!(result.is_ok(), "identity impl param case failed: {:?}", result.err());
}

/// WS-2 done-criteria: const-generic CONSTRUCTOR calls keyed by resolved
/// VALUE produce per-value struct identities -- locals must allocate
/// Cache_64/Cache_128 (never SIZE-spelled ghosts), and both
/// instantiations coexist.
#[test]
fn const_value_keyed_identities_distinct() {
    let src = r#"
        package main

        struct Cache<const SIZE: i64> {
            next: i64,
            scale: i64,
        }

        impl<const SIZE: i64> Cache<SIZE> {
            pub fn new(scale: i64) -> Cache<SIZE> {
                return Cache { next: 0, scale: scale };
            }
            pub fn scaled(&self) -> i64 {
                return self.next + self.scale;
            }
        }

        pub fn main() -> i32 {
            let a = Cache::<64>::new(1);
            let b = Cache::<128>::new(2);
            let s = b.scaled() - a.scaled();
            return (a.next + s) as i32;
        }
    "#;
    let result = compile(src, false, None, true);
    assert!(result.is_ok(), "const-value keying failed: {:?}", result.err());
    let mlir = result.unwrap();

    assert!(mlir.contains("Cache_64"), "Cache_64 identity missing");
    assert!(mlir.contains("Cache_128"), "Cache_128 identity missing");
    // Uses (allocas, call returns) must never spell the param-name ghost;
    // a vestigial unused DECL may remain from scan-time registration.
    let uses_ghost = mlir.lines().filter(|l| l.contains("Cache_SIZE"))
        .any(|l| !l.contains("=") || l.contains("-> "));
    assert!(!uses_ghost, "param-name ghost used in an op");
    assert!(!mlir.contains("main__Cache_main__SIZE"), "composed ghost must not appear");
}
