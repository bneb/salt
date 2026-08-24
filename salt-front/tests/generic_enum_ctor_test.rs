// Regression tests: qualified constructor calls on GENERIC enums.
//
// Before the fix, `let o = Opt::Some(5);` failed at codegen with
// [E003] Undefined function or symbol: 'main__Opt__Some', because
// resolve_path_to_enum only specialized generic enum templates when the
// concrete generic arguments were already known (turbofish or an expected
// type annotation). Constructor argument inference now covers the
// un-annotated case, for local and imported enums alike.

use saltc::compile;

/// The original repro: un-annotated constructor on a LOCAL generic enum.
#[test]
fn test_local_generic_enum_ctor_infers_from_argument() {
    let code = r#"
        package main

        enum Opt<T> {
            Some(T),
            None,
        }

        pub fn main() -> i32 {
            let o = Opt::Some(5);
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Local generic enum ctor failed: {:?}", result.err());
}

/// Un-annotated ctor on an IMPORTED generic enum (std Option).
#[test]
fn test_imported_option_ctor_infers_from_argument() {
    let code = r#"
        package main

        use std.core.option.*;

        pub fn main() -> i32 {
            let o = Option::Some(5);
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Imported Option ctor failed: {:?}", result.err());
}

/// Un-annotated ctor on std Result<T> (Err payload is concrete Status).
#[test]
fn test_imported_result_ctor_infers_from_argument() {
    let code = r#"
        package main

        use std.core.result.*;

        pub fn main() -> i32 {
            let r = Result::Ok(1);
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Imported Result ctor failed: {:?}", result.err());
}

/// Multi-parameter template: both generics inferred from two payload args.
#[test]
fn test_two_param_enum_ctor_infers_both_generics() {
    let code = r#"
        package main

        enum Pair<A, B> {
            P(A, B),
        }

        pub fn main() -> i32 {
            let p = Pair::P(7, 9);
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Two-param enum ctor failed: {:?}", result.err());
}

/// Non-generic qualified ctors must keep using the registry fast path.
#[test]
fn test_non_generic_enum_ctor_still_resolves() {
    let code = r#"
        package main

        enum Color {
            Red(i32),
            Green,
        }

        pub fn main() -> i32 {
            let c = Color::Red(5);
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Non-generic enum ctor broke: {:?}", result.err());
}

/// Explicit annotations must keep driving specialization via expected_ty.
#[test]
fn test_annotated_generic_enum_ctor_still_resolves() {
    let code = r#"
        package main

        use std.core.option.*;

        pub fn main() -> i32 {
            let o: Option<i32> = Option::Some(5);
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Annotated Option ctor broke: {:?}", result.err());
}

/// PINNED BEHAVIOR (verified against emitted MLIR): with an annotation AND a
/// compound expression arg, the ANNOTATION wins - Some(1 / 2 + 0) under
/// Option<f64> specializes T=f64 and the literal division promotes into the
/// f64 slot at emission (sitofp). Expected types always override when they
/// match; bare-literal inference only fires when final_generics is empty.
/// The is_ok assertion pins compilability under this priority.
#[test]
fn test_annotated_ctor_with_expression_arg_pins_current_priority() {
    let code = r#"
        package main

        use std.core.option.*;

        pub fn main() -> i32 {
            let o: Option<f64> = Option::Some(1 / 2 + 0);
            match o {
                Option::Some(v) => { return v as i32; }
                Option::None => { return 0; }
            }
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Annotated ctor with compound arg failed: {:?}", result.err());
}

/// Inferred instance must be matchable with correct payload type flow.
#[test]
fn test_inferred_option_supports_match_with_payload() {
    let code = r#"
        package main

        use std.core.option.*;

        pub fn main() -> i32 {
            let x: i64 = 5;
            let o = Option::Some(x);
            match o {
                Option::Some(v) => { return v as i32; }
                Option::None => { return 0; }
            }
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Match over inferred Option failed: {:?}", result.err());
}

/// NEGATIVE: when inference cannot bind every generic (payload-blind variant
/// like Err(Status), no expected type, no prior specialization) the compiler
/// must reject cleanly rather than guess. Expression args DO feed inference
/// since the tracer gained Binary/Unary/Paren arms; only genuinely unbindable
/// cases land here - this pins that rejection, which must stay.
#[test]
fn test_payload_blind_ctor_without_context_fails_cleanly() {
    let code = r#"
        package main

        use std.core.result.*;

        pub fn main() -> i32 {
            let r = Result::Err(7);
            match r {
                Result::Ok(v) => { return v as i32; }
                Result::Err(s) => { return s.code; }
            }
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_err(), "Payload-blind Err compiled without any way to bind T: {:?}", result.ok());
}

/// B1 NEGATIVE: a float value traced into an INTEGER payload slot must be
/// rejected up front by verify_ctor_arg_types (emission has no f32->i64
/// promotion and would fail late with 'Numeric promotion not supported').
/// The diagnostic must name the expected slot ('expected').
#[test]
fn test_float_into_int_slot_rejected_with_expected_message() {
    let code = r#"
        package main

        enum Opt<T> {
            Some(T),
            None,
        }

        pub fn main() -> i32 {
            let o: Opt<i64> = Opt::Some(1.5);
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    let msg = format!("{:?}", result.as_ref().err());
    assert!(result.is_err(), "Float-into-int-slot ctor compiled: {:?}", result.ok());
    assert!(msg.contains("expected"), "Rejection lacks 'expected' diagnostic: {}", msg);
}

/// B1 POSITIVE: an integer value may coerce into a narrower/wider integer
/// slot - emission lowers it via index_cast/trunci downstream.
#[test]
fn test_usize_arg_coerces_into_i32_slot() {
    let code = r#"
        package main

        enum Opt<T> {
            Some(T),
            None,
        }

        pub fn main() -> i32 {
            let u: usize = 4;
            let o: Opt<i32> = Opt::Some(u);
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Usize-into-i32-slot coercion failed: {:?}", result.err());
}

/// PIN FLIPPED (payload-slot conformance ticket CLOSED): the tracer now
/// types bare fn items as `Type::Fn` (tracer_lowering trace_path probes
/// discovery.globals under the mangled fn key), so conformance no longer
/// defers them. Ok(g) under Opt<i64> is a hard resolution-time error —
/// emission used to silently ptrtoint the fn address into the i64 slot.
#[test]
fn test_fn_item_ctor_arg_rejected() {
    let code = r#"
        package main

        enum Opt<T> {
            Some(T),
            None,
        }

        fn g() -> i64 {
            return 7;
        }

        pub fn main() -> i32 {
            let o: Opt<i64> = Opt::Some(g);
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    let err = format!("{}", result.expect_err("fn item into i64 slot must be rejected"));
    assert!(err.contains("function item"), "expected 'function item' in error, got: {}", err);
}

/// R5: unannotated TUPLE-payload ctors infer via the tracer's Expr::Tuple
/// arm - before it, Result::Ok((1, 2)) failed with the misleading
/// 'Undefined function or symbol' because tuples were untraceable.
#[test]
fn test_unannotated_tuple_ctor_infers_payload() {
    let code = r#"
        package main

        enum Box2<T> {
            Packed(T),
            Empty,
        }

        pub fn main() -> i32 {
            let b = Box2::Packed((1, 2));
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Tuple-payload ctor failed: {:?}", result.err());
}

/// Expression arguments now trace statically: binops yield the wider operand
/// type, so Some(1 + 2 * 3) binds T = i64.
#[test]
fn test_expression_arg_infers_wider_int_type() {
    let code = r#"
        package main

        enum Opt<T> {
            Some(T),
            None,
        }

        pub fn main() -> i32 {
            let o = Opt::Some(1 + 2 * 3);
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Expression-arg ctor failed: {:?}", result.err());
}
