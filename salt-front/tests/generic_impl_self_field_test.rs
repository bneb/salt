// Regression tests: FIELD ACCESS ON ERASED GENERIC SELF-TYPES (roadmap D1).
//
// Complements generic_field_access_test.rs, which covers explicitly
// specialized impls (`impl Prod for Slot<i64>`). These tests pin the
// still-generic `impl<T> Slot<T>` form, where the impl is registered with an
// erased self type and monomorphized per call site.
//
// Two defects broke these shapes before:
//   1. A deferred expansion (arg-count mismatch) cached an empty-fields stub
//      in the struct registry under the bare template name
//      ("main__Slot"), shadowing real lookups.
//   2. Impl-method hydration installed the erased self type verbatim, so
//      bodies could not resolve `self.v` against any registered instance.
use saltc::compile;

/// Required method on a still-generic impl reading a field of self.
#[test]
fn ref_self_field_read_in_generic_impl() {
    let code = r#"
        package main

        struct Slot<T> { v: T }

        impl<T> Slot<T> {
            fn get(&self) -> T {
                return self.v;
            }
        }

        pub fn main() -> i32 {
            let s = Slot { v: 42 };
            let x = s.get();
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "ref-self generic impl failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(
        mlir.contains("main__Slot_i32__get"),
        "method must be monomorphized for the i32 instance"
    );
    assert!(
        mlir.contains("!struct_main__Slot_i32 = "),
        "specialized struct layout must be emitted"
    );
}

/// By-value self receiver: body reads self.v, return type is concrete.
#[test]
fn value_self_field_read_in_generic_impl() {
    let code = r#"
        package main

        struct Slot<T> { v: T }

        impl<T> Slot<T> {
            fn peek(self) -> i32 {
                let n = self.v as i32;
                return n;
            }
        }

        pub fn main() -> i32 {
            let s = Slot { v: 42 };
            let r = s.peek();
            return r;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "value-self generic impl failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(
        mlir.contains("llvm.getelementptr"),
        "field read must lower to a GEP into the specialized layout"
    );
}

/// Method returning the generic parameter T: the caller must see the
/// substituted concrete type, not an unresolved placeholder.
#[test]
fn generic_return_type_propagates_to_caller() {
    let code = r#"
        package main

        struct Slot<T> { v: T }

        impl<T> Slot<T> {
            fn get(self) -> T {
                return self.v;
            }
        }

        pub fn main() -> i32 {
            let s = Slot { v: 7 };
            let x = s.get();
            return x;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "T-returning method failed: {:?}", result.err());
}

/// Trait-required method on a still-generic trait impl touching self.v.
#[test]
fn trait_required_method_reads_field_in_generic_impl() {
    let code = r#"
        package main

        struct Slot<T> { v: T }

        trait Peeker {
            fn peek(&self) -> i32;
        }

        impl<T> Peeker for Slot<T> {
            fn peek(&self) -> i32 {
                return self.v as i32;
            }
        }

        pub fn main() -> i32 {
            let s = Slot { v: 42 };
            let r = s.peek();
            return r;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "trait impl field access failed: {:?}", result.err());
}

/// Trait-DEFAULT method, omitted by the impl, whose inherited body touches
/// self.v of a still-generic struct: the body must be monomorphized against
/// the concrete instance (exit criterion: required AND defaulted methods).
#[test]
fn trait_default_method_reads_field_in_generic_impl() {
    let code = r#"
        package main

        struct Slot<T> { v: T }

        trait Peeker {
            fn peek(&self) -> i32 {
                return self.v as i32;
            }
        }

        impl<T> Peeker for Slot<T> {
        }

        pub fn main() -> i32 {
            let s = Slot { v: 42 };
            let r = s.peek();
            return r;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "defaulted method field access failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(
        mlir.contains("main__Slot_i32__peek"),
        "inherited default must be monomorphized for the i32 instance"
    );
    assert!(
        mlir.contains("llvm.getelementptr"),
        "inherited default must lower its self.v read to a GEP"
    );
}
