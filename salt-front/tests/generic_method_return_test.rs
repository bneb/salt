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
