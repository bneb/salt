// Regression tests: UNQUALIFIED constructor calls on enum variants
// (`Some(x)`, not `Option::Some(x)`).
//
// Before the fix, resolve_path_to_enum (src/codegen/expr/enum_ctor.rs)
// required a qualified path: it split the call target on "__" and bailed
// out immediately when fewer than 2 parts were present. A bare call like
// `Some(v)` mangles to the single segment "Some", so it never reached the
// registry/template resolution at all and fell through to the generic
// "Undefined function or symbol: 'Some'" error - even though the exact same
// scrutinee's `match opt { Some(x) => .., None => .. }` already resolved
// bare variant *patterns* fine (NameResolver handles patterns separately),
// and a bare unit-variant *value* like `return None` already resolved
// (literals.rs looks bare unit variants up by name). Only the
// call-expression construction path for a payload variant was missing.
//
// The fix anchors unqualified call resolution on the expected type: with
// no expected enum type, the target is genuinely ambiguous and resolution
// is deliberately left unresolved (see
// test_unqualified_ctor_without_expected_type_fails_cleanly below).

use saltc::compile;

/// The reported repro, shrunk to a self-contained local enum: a function
/// returning an unqualified payload-variant call, matched by a sibling
/// function using unqualified variant patterns (which already worked).
#[test]
fn test_unqualified_local_enum_ctor_in_return_position() {
    let code = r#"
        package main

        enum Opt<T> {
            Some(T),
            None,
        }

        fn is_some(o: Opt<i32>) -> i32 {
            match o {
                Some(x) => { return 1; }
                None => { return 0; }
            }
        }

        fn make_some(v: i32) -> Opt<i32> {
            return Some(v)
        }

        fn make_none() -> Opt<i32> {
            return None
        }

        pub fn main() -> i32 {
            let s = is_some(make_some(5));
            let n = is_some(make_none());
            return s - n;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Unqualified local enum ctor failed: {:?}", result.err());
}

/// Same shape against std's `Option<T>` - the exact bug report
/// (salt-opt/tests/opt_option_test.salt), minus the unrelated `Box<i32>`
/// parameter (a separate, pre-existing struct-emission gap: bare `Box<T>`
/// used only as a parameter/return type never gets a struct definition
/// emitted, regardless of enums - confirmed independent of this fix).
///
/// Explicit `use std.core.option.*` (matching every other std-Option test in
/// generic_enum_ctor_test.rs) rather than relying on the bare prelude alone:
/// under this test harness's reduced `compile(.., registry: None, ..)` mode,
/// implicit-prelude-only resolution names the type differently than an
/// explicit import does, which is orthogonal to enum-ctor resolution and
/// reproduces identically with the already-working qualified `Option::Some`.
#[test]
fn test_unqualified_option_ctor_in_return_position() {
    let code = r#"
        package main

        use std.core.option.*;

        fn is_some(opt: Option<i32>) -> i32 {
            match opt {
                Some(x) => { return 1; }
                None => { return 0; }
            }
        }

        fn make_some(v: i32) -> Option<i32> {
            return Some(v)
        }

        fn make_none() -> Option<i32> {
            return None
        }

        pub fn main() -> i32 {
            let s = is_some(make_some(5));
            let n = is_some(make_none());
            return s - n;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Unqualified Option ctor failed: {:?}", result.err());
}

/// Unqualified ctor on a `let` binding driven by a type annotation, rather
/// than a function return type - a different expected_ty source.
#[test]
fn test_unqualified_ctor_from_let_annotation() {
    let code = r#"
        package main

        use std.core.option.*;

        pub fn main() -> i32 {
            let o: Option<i32> = Some(5);
            match o {
                Some(v) => { return v; }
                None => { return 0; }
            }
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Unqualified ctor from let annotation failed: {:?}", result.err());
}

/// Unqualified NON-generic enum ctors must keep resolving too - parity with
/// the qualified-path non-generic case (see generic_enum_ctor_test.rs's
/// test_non_generic_enum_ctor_still_resolves).
#[test]
fn test_unqualified_non_generic_enum_ctor_resolves() {
    let code = r#"
        package main

        enum Color {
            Red(i32),
            Green,
        }

        fn make_red(v: i32) -> Color {
            return Red(v)
        }

        pub fn main() -> i32 {
            let c = make_red(5);
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Unqualified non-generic enum ctor failed: {:?}", result.err());
}

/// Two distinct enums declaring a variant with the same name: the expected
/// type must pick the one that actually matches, not just the first one
/// found by name.
#[test]
fn test_unqualified_ctor_disambiguates_via_expected_type() {
    let code = r#"
        package main

        enum Alpha<T> {
            Tagged(T),
            Empty,
        }

        enum Beta<T> {
            Tagged(T),
            Empty,
        }

        fn make_beta(v: i32) -> Beta<i32> {
            return Tagged(v)
        }

        pub fn main() -> i32 {
            let b = make_beta(5);
            match b {
                Beta::Tagged(v) => { return v; }
                Beta::Empty => { return 0; }
            }
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Expected-type disambiguation between same-named variants failed: {:?}", result.err());
}

/// The MLIR from the definition whose header contains `header` up to the
/// next function.
fn fn_body<'a>(mlir: &'a str, header: &str) -> &'a str {
    let start = mlir.find(header).unwrap_or_else(|| panic!("{header} not emitted:\n{mlir}"));
    let rest = &mlir[start..];
    rest[1..].find("func.func").map_or(rest, |end| &rest[..end + 1])
}

/// The bare call must build the variant it names, with its payload: B is
/// variant 1 of E, not the first one. With no `package`, the expected type
/// arrives as `Type::Enum("E")` rather than a package-qualified Concrete.
#[test]
fn test_unqualified_ctor_builds_the_named_variant() {
    let code = r#"
        enum E {
            A(i32),
            B(i32),
        }

        fn mk() -> E {
            return B(5)
        }

        pub fn main() -> i32 {
            match mk() {
                E::A(v) => { return 1; }
                E::B(v) => { return v; }
            }
        }
    "#;
    let mlir = compile(code, false, None, true).expect("compiles");
    let body = fn_body(&mlir, "func.func private @mk()");
    let tags: Vec<&str> = body.lines().map(str::trim).filter(|l| l.starts_with("%disc_")).collect();
    assert_eq!(tags.len(), 1, "one tag constant:\n{body}");
    let (tag, value) = tags[0].split_once(" = ").unwrap();
    assert_eq!(value, "arith.constant 1 : i32", "B is variant 1:\n{body}");
    assert!(body.contains(&format!("llvm.insertvalue {tag}, ")), "the tag is stored:\n{body}");
    assert!(body.contains("= arith.constant 5 : i32"), "payload 5:\n{body}");
}

/// A function named like a variant of the expected enum still wins a bare
/// call, as it did before bare variant calls resolved at all: the variant
/// is only a fallback for a name nothing else defines.
#[test]
fn test_unqualified_call_prefers_a_same_named_function() {
    let code = r#"
        package main

        enum Wrap {
            Val(i32),
            Nothing,
        }

        fn Val(x: i32) -> Wrap {
            return Wrap::Nothing
        }

        fn make() -> Wrap {
            return Val(3)
        }

        pub fn main() -> i32 {
            match make() {
                Wrap::Val(v) => { return v; }
                Wrap::Nothing => { return 7; }
            }
        }
    "#;
    let mlir = compile(code, false, None, true).expect("compiles");
    assert!(mlir.contains("call @main__Val("), "bare Val(3) must call fn Val, not build Wrap::Val:\n{mlir}");
}

/// NEGATIVE: with no expected type to anchor resolution (no return type, no
/// annotation), an unqualified payload-variant call must still be rejected
/// cleanly: nothing binds T. This must keep failing, not start guessing.
///
/// The binding is consumed by a match (not left unused): an unused `let` is
/// dead-code-eliminated before resolution runs, which would make this pass
/// for the wrong reason - see stdlib-health.md's negative-test hygiene note.
#[test]
fn test_unqualified_ctor_without_expected_type_fails_cleanly() {
    let code = r#"
        package main

        enum Opt<T> {
            Some(T),
            None,
        }

        pub fn main() -> i32 {
            let o = Some(5);
            match o {
                Some(v) => { return v; }
                None => { return 0; }
            }
        }
    "#;
    let err = compile(code, false, None, true)
        .expect_err("Unannotated unqualified ctor compiled without any way to bind T");
    // Rejected for the unresolvable call itself, not some unrelated error.
    let msg = format!("{err:#}");
    assert!(msg.contains("Undefined function or symbol") && msg.contains("Some"), "{msg}");
}
