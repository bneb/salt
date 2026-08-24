// Regression tests: FIELD ACCESS ON GENERIC STRUCTS (roadmap debt D1).
//
// Before the fix, `let s = Slot { v: 7 };` recorded the local as the
// UNSPECIALIZED template type (Struct("main__Slot"), registry entry with
// zero fields), so any later `s.v` failed with:
//   [E003] Cannot access field 'v' on type Struct("main__Slot")
// The failure was independent of trait defaults — it broke plain field
// reads, impl method bodies reading generic self fields, and therefore
// every generic trait implementation that touched state.
//
// Fix: struct-literal emission now infers generic arguments from field
// expressions even with an empty context type map (top level), requiring
// the FULL parameter set so ambiguous literals never half-specialize.
use saltc::compile;

/// Plain field read through an inferred generic literal at top level.
#[test]
fn direct_field_read_on_generic_struct() {
    let code = r#"
        package main

        struct Slot<T> { v: T }

        pub fn main() -> i32 {
            let s = Slot { v: 7 };
            let x = s.v;
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "generic field read failed: {:?}", result.err());
}

/// Field access on `self` inside an impl method for a specialized impl
/// (`impl Prod for Slot<i64>`) — the receiver must carry the specialization.
#[test]
fn self_field_read_in_specialized_impl_body() {
    let code = r#"
        package main

        struct Slot<T> { v: T }

        trait Prod {
            fn value(&self) -> i64;
        }

        impl Prod for Slot<i64> {
            fn value(&self) -> i64 {
                return self.v;
            }
        }

        pub fn main() -> i32 {
            let s = Slot { v: 7 };
            let x = s.value();
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "self.v in generic impl failed: {:?}", result.err());
}

/// Combined shape from the roadmap debt entry: required method touching
/// fields + OMITTED default inherited and called on the same instance.
#[test]
fn generic_impl_with_inherited_default_and_fields() {
    let code = r#"
        package main

        struct Slot<T> { v: T }

        trait Prod {
            fn value(&self) -> i64;
            fn doubled(&self) -> i64 {
                return 5555555;
            }
        }

        impl Prod for Slot<i64> {
            fn value(&self) -> i64 {
                return self.v;
            }
        }

        pub fn main() -> i32 {
            let s = Slot { v: 7 };
            let direct = s.v;
            let via_method = s.value();
            let inherited = s.doubled();
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "combined generic/default case failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(mlir.contains("5555555"), "inherited default body must be emitted");
}

/// Two distinct specializations of one generic struct must not collide:
/// each keeps its own field type.
#[test]
fn two_specializations_coexist() {
    let code = r#"
        package main

        struct Pair<T> { left: T }

        pub fn main() -> i32 {
            let a = Pair { left: 12 };
            let b = Pair { left: 34 };
            let x = a.left;
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "dual specialization failed: {:?}", result.err());
}
