// Regression tests: CONST-GENERIC CONSTRUCTORS must not crash the
// compiler (round-1 remaining-work item #4).
//
// The bug: `Pool::<64>::new()` whose body constructs `Cache::<N>::new()`
// — or simply TWO instantiations of one const-generic constructor in a
// single function (`Cache::<64>::new()` + `Cache::<128>::new()`) — drove
// type resolution/substitution into infinite self-recursion ("fatal
// runtime error: stack overflow"). Const params parsed outside
// generic-name context became self-named Concretes (`SIZE ->
// Concrete("SIZE")`), which resolution and substitution kept re-expanding.
//
// The fix under test: self-referential map bindings are recognized in
// `substitute_generics::is_self_ref` and `resolve_codegen_type_concrete`,
// param bindings normalize self-Concretes to Generics at the type-map
// boundary, and substitution carries a totality depth cap.
use saltc::compile;

/// A constructor calling another const-generic constructor must compile.
#[test]
fn nested_const_generic_ctors_compile() {
    let src = r#"
        package main

        struct Cache<const SIZE: i64> {
            next: i64,
        }

        impl<const SIZE: i64> Cache<SIZE> {
            pub fn new() -> Cache<SIZE> {
                return Cache { next: 0 };
            }
        }

        struct Pool<const N: i64> {
            cache: Cache<N>,
        }

        impl<const N: i64> Pool<N> {
            pub fn new() -> Pool<N> {
                return Pool { cache: Cache::<N>::new() };
            }
        }

        pub fn main() -> i32 {
            let p = Pool::<64>::new();
            let n = p.cache.next;
            return n as i32;
        }
    "#;
    let result = compile(src, false, None, true);
    assert!(result.is_ok(), "nested const-generic ctors failed: {:?}", result.err());
}

/// Two instantiations of one const-generic constructor in a single
/// function previously overflowed the stack during compilation.
#[test]
fn two_instantiations_of_one_const_generic_ctor_compile() {
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
    assert!(result.is_ok(), "two const-generic instantiations failed: {:?}", result.err());
}
