// Regression tests: FN-ITEM PAYLOAD CONFORMANCE at enum-constructor
// resolution time (round-1 ticket #1).
//
// The bug: `Probe::Val(answer)` — a bare function name passed as an enum
// constructor argument — traced as untraceable, deferred to emission, and
// emission silently `ptrtoint`ed the function's ENTRY ADDRESS into the
// payload slot: `Val(i64)` stored a code pointer as an integer with no
// diagnostic.
//
// The fix under test: the tracer now resolves bare fn names to their
// signature type (`Type::Fn`), and payload conformance rejects any fn
// item whose slot is not itself a function pointer — while genuine
// `Val(double)`-style storage into a `fn(...)->...` slot keeps working.
use saltc::compile;

const REJECT_MSG: &str = "function item";

/// A fn item into an `i64` payload slot must be a hard error, not a
/// silently-stored entry address.
#[test]
fn fn_item_into_scalar_payload_rejected() {
    let src = r#"
        package main

        enum Probe {
            Val(i64),
        }

        fn answer() -> i64 {
            return 42;
        }

        pub fn main() -> i32 {
            let p = Probe::Val(answer);
            let x = match p {
                Probe::Val(v) => v,
            };
            return 0;
        }
    "#;
    let result = compile(src, false, None, true);
    let err = format!("{}", result.expect_err("fn item into i64 slot must be rejected"));
    assert!(err.contains(REJECT_MSG), "expected '{}' hint in error, got: {}", REJECT_MSG, err);
}

/// Same hole through the generic template path (`Wrap::<i64>::Just`) must
/// also close: generics binding does not bypass conformance.
#[test]
fn generic_fn_item_into_scalar_payload_rejected() {
    let src = r#"
        package main

        enum Wrap<T> {
            Just(T),
        }

        fn answer() -> i64 {
            return 42;
        }

        pub fn main() -> i32 {
            let w = Wrap::<i64>::Just(answer);
            let x = match w {
                Wrap::Just(v) => v,
            };
            return 0;
        }
    "#;
    let result = compile(src, false, None, true);
    let err = format!("{}", result.expect_err("generic path must reject fn item too"));
    assert!(err.contains(REJECT_MSG), "expected '{}' hint in error, got: {}", REJECT_MSG, err);
}

/// Legitimate use stays green: a fn item into a genuine function-pointer
/// slot compiles and dispatches indirectly through the stored pointer.
#[test]
fn fn_item_into_fn_pointer_slot_still_accepted() {
    let src = r#"
        package main

        enum Handler {
            With(fn(i64) -> i64),
            None,
        }

        fn double(x: i64) -> i64 {
            return x * 2;
        }

        pub fn main() -> i32 {
            let h = Handler::With(double);
            let r = match h {
                Handler::With(f) => f(21),
                Handler::None => 0,
            };
            return 0;
        }
    "#;
    let result = compile(src, false, None, true);
    assert!(result.is_ok(), "fn item into fn-pointer slot must compile: {:?}",
             result.err());
}
