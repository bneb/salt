// Payload conformance for an enum constructor whose argument is itself a
// bare constructor call (src/codegen/expr/enum_ctor/payload.rs).
//
// `return Ok(Some(x))` under `-> Result<Option<i32>>` failed with
// "Constructor argument type mismatch in std__core__result__Result::Ok
// argument 1: expected Concrete(\"std__core__option__Option\", [I32]), found
// Unit". The payload check traces each argument before emission, and the
// tracer types a call it can't resolve to a function as Unit. Emission
// passes the payload slot's type as the argument's expected type, which
// lets the bare-variant fallback resolve `Some(x)` as Option<i32>::Some, so
// the check leaves those calls to emission. Emission then builds such a call
// only if resolution picks a variant of the slot's type with one argument
// per payload field: an intrinsic, struct or function of the same name, or
// another enum's variant, is an error there.

use saltc::compile;

/// The MLIR from the definition whose header contains `header` up to the
/// next function.
fn fn_body<'a>(mlir: &'a str, header: &str) -> &'a str {
    let start = mlir.find(header).unwrap_or_else(|| panic!("{header} not emitted:\n{mlir}"));
    let rest = &mlir[start..];
    rest[1..].find("func.func").map_or(rest, |end| &rest[..end + 1])
}

/// The discriminant constants a function body builds, in emission order.
fn tags(body: &str) -> Vec<&str> {
    body.lines()
        .map(str::trim)
        .filter(|l| l.starts_with("%disc_"))
        .filter_map(|l| l.split_once(" = ").map(|(_, value)| value))
        .collect()
}

fn compile_err(code: &str, what: &str) -> String {
    let err = compile(code, false, None, true).expect_err(what);
    format!("{err:#}")
}

/// Each `llvm.store VALUE, %payload_buf_N : TYPE, !llvm.ptr` in `body`, as
/// (VALUE, TYPE): what a constructor stores in its payload, typed as its slot.
fn payload_stores(body: &str) -> Vec<(&str, &str)> {
    let stores = body.lines().map(str::trim).filter_map(|l| l.strip_prefix("llvm.store "));
    stores
        .filter_map(|rest| {
            let (value, rest) = rest.split_once(", ")?;
            let (buf, ty) = rest.split_once(" : ")?;
            buf.starts_with("%payload_buf_").then(|| (value, ty.trim_end_matches(", !llvm.ptr")))
        })
        .collect()
}

/// The type `body` gives `value` where it defines it.
fn defined_type<'a>(body: &'a str, value: &str) -> Option<&'a str> {
    let def = format!("{value} = ");
    body.lines().map(str::trim).find(|l| l.starts_with(&def))?.rsplit(" : ").next()
}

/// The reported case: a bare std Option constructor as the payload of a bare
/// std Result constructor.
#[test]
fn bare_ctor_in_bare_result_ctor_payload() {
    let code = r#"
        package main

        use std.core.option.*;
        use std.core.result.*;

        fn wrap(x: i32) -> Result<Option<i32>> {
            return Ok(Some(x))
        }

        pub fn main() -> i32 {
            match wrap(5) {
                Ok(o) => {
                    match o {
                        Some(v) => { return v; }
                        None => { return 1; }
                    }
                }
                Err(s) => { return 2; }
            }
        }
    "#;
    let mlir = compile(code, false, None, true).unwrap_or_else(|e| panic!("Ok(Some(x)) failed: {e:#}"));
    // The Result's payload holds the Option<i32> the inner call built, not
    // just a value stored under that type.
    let body = fn_body(&mlir, "func.func private @main__wrap(");
    let option_i32 = "!struct_std__core__option__Option_i32";
    let &(value, ty) = payload_stores(body).last().expect("the Result's payload store");
    assert_eq!(ty, option_i32, "payload slot type:\n{body}");
    assert_eq!(defined_type(body, value), Some(option_i32), "stored value's type:\n{body}");
}

/// `TYPE<TYPE<i32>>` with `TYPE` std's Option or a local generic enum.
const NESTED_OPTION: &str = r#"
    package main

    DECL

    fn wrap(v: i32) -> TYPE<TYPE<i32>> {
        return BODY
    }

    pub fn main() -> i32 {
        match wrap(5) {
            TYPE::Some(o) => {
                match o {
                    TYPE::Some(v) => { return v; }
                    TYPE::None => { return 1; }
                }
            }
            TYPE::None => { return 2; }
        }
    }
"#;

/// A bare constructor inside a bare or a qualified one of the same enum.
#[test]
fn bare_ctor_in_same_enum_payload() {
    let local = "enum Opt<T> {\n        Some(T),\n        None,\n    }";
    for (ty, decl, body) in [
        ("Option", "use std.core.option.*;", "Some(Some(v))"),
        ("Option", "use std.core.option.*;", "Option::Some(Some(v))"),
        ("Opt", local, "Some(Some(v))"),
        ("Opt", local, "Opt::Some(Some(v))"),
    ] {
        let code = NESTED_OPTION.replace("DECL", decl).replace("TYPE", ty).replace("BODY", body);
        let result = compile(&code, false, None, true);
        assert!(result.is_ok(), "{ty}: {body} failed: {:?}", result.err());
    }
}

/// The inner call builds the variant it names, inside the variant the outer
/// call names: in `A(B(5))` the outer tag is A's (0), then the inner tag is
/// B's (1), with the payload 5.
#[test]
fn nested_bare_ctor_builds_the_named_variant() {
    let code = r#"
        package main

        enum Pick<T> {
            A(T),
            B(T),
        }

        fn mk() -> Pick<Pick<i32>> {
            return A(B(5))
        }

        pub fn main() -> i32 {
            match mk() {
                Pick::A(inner) => {
                    match inner {
                        Pick::A(v) => { return 1; }
                        Pick::B(v) => { return v; }
                    }
                }
                Pick::B(inner) => { return 2; }
            }
        }
    "#;
    let mlir = compile(code, false, None, true).expect("compiles");
    let body = fn_body(&mlir, "func.func private @main__mk()");
    assert_eq!(tags(body), ["arith.constant 0 : i32", "arith.constant 1 : i32"], "A, then B:\n{body}");
    assert!(body.contains("= arith.constant 5 : i32"), "payload 5:\n{body}");
}

/// A non-generic enum takes the registry path, which checks a two-field
/// variant field by field against slots of type `Concrete("Inner", [])`.
/// `Both(B(5), A(6))` builds Both (1) holding B (1), then A (0).
#[test]
fn bare_ctors_in_non_generic_multi_field_payload() {
    let code = r#"
        enum Inner {
            A(i32),
            B(i32),
        }

        enum Outer {
            Empty,
            Both(Inner, Inner),
        }

        fn mk() -> Outer {
            return Both(B(5), A(6))
        }

        pub fn main() -> i32 {
            match mk() {
                Outer::Empty => { return 0; }
                Outer::Both(x, y) => { return 1; }
            }
        }
    "#;
    let mlir = compile(code, false, None, true).expect("compiles");
    let body = fn_body(&mlir, "func.func private @mk()");
    let expected = ["arith.constant 1 : i32", "arith.constant 1 : i32", "arith.constant 0 : i32"];
    assert_eq!(tags(body), expected, "Both, then B, then A:\n{body}");
    assert!(body.contains("= arith.constant 5 : i32"), "payload 5:\n{body}");
    assert!(body.contains("= arith.constant 6 : i32"), "payload 6:\n{body}");
}

/// A payload that isn't a constructor call is still checked before
/// emission: a string can't fill an i64 slot, qualified or bare.
#[test]
fn mistyped_payload_still_rejected() {
    for ctor in ["Result::Ok", "Ok"] {
        let code = r#"
            package main

            use std.core.result.*;

            fn f() -> Result<i64> {
                return CTOR("boom")
            }

            pub fn main() -> i32 {
                match f() {
                    Result::Ok(v) => { return 0; }
                    Result::Err(s) => { return 1; }
                }
            }
        "#.replace("CTOR", ctor);
        let msg = compile_err(&code, ctor);
        let want = "Constructor argument type mismatch in std__core__result__Result::Ok argument 1: \
                    expected I64, found Struct(\"std__core__str__StringView\")";
        assert!(msg.contains(want), "{ctor}: {msg}");
    }
}

/// Leaving the inner call to emission doesn't leave it unchecked: emission
/// resolves `Some("boom")` as Option<i32>::Some, whose own payload check
/// rejects the string.
#[test]
fn mistyped_payload_of_nested_ctor_rejected() {
    let code = r#"
        package main

        use std.core.option.*;
        use std.core.result.*;

        fn wrap() -> Result<Option<i32>> {
            return Ok(Some("boom"))
        }

        pub fn main() -> i32 {
            match wrap() {
                Ok(o) => { return 0; }
                Err(s) => { return 1; }
            }
        }
    "#;
    let msg = compile_err(code, "Ok(Some(\"boom\"))");
    let want = "Constructor argument type mismatch in std__core__option__Option::Some argument 1: \
                expected I32, found Struct(\"std__core__str__StringView\")";
    assert!(msg.contains(want), "{msg}");
}

/// `Opt<Opt<i32>>` built as `Opt::Some(CALL)` after `PRE`; DECL or PRE may
/// give CALL's name a meaning other than the variant.
const OPT_WITH: &str = r#"
    package main

    enum Opt<T> {
        Some(T),
        None,
    }

    DECL

    fn wrap(v: i32) -> Opt<Opt<i32>> {
        PRE
        return Opt::Some(CALL)
    }

    pub fn main() -> i32 {
        match wrap(5) {
            Opt::Some(o) => { return 0; }
            Opt::None => { return 1; }
        }
    }
"#;

fn opt_with(decl: &str, pre: &str, call: &str) -> String {
    OPT_WITH.replace("DECL", decl).replace("PRE", pre).replace("CALL", call)
}

/// The payload check's own rejection of `wrap`'s argument, whatever type the
/// tracer found for it.
const WRAP_ARG_REJECTED: &str =
    "Constructor argument type mismatch in main__Opt::Some argument 1: expected Concrete(\"main__Opt\", [I32])";

/// A function named like the variant claims a bare call, nested or not, so
/// its return type is checked here: Opt<i64> can't fill an Opt<i32> slot.
/// Emission wouldn't reject it: its numeric promotion passes a value between
/// two instances of one enum with the same number of type arguments.
#[test]
fn same_named_function_in_payload_still_checked() {
    let decl = "fn Some(v: i32) -> Opt<i64> {\n        return Opt::None\n    }";
    let msg = compile_err(&opt_with(decl, "", "Some(v)"), "fn Some returns Opt<i64>");
    let want = "Constructor argument type mismatch in main__Opt::Some argument 1: \
                expected Concrete(\"main__Opt\", [I32]), found Concrete(\"main__Opt\", [I64])";
    assert!(msg.contains(want), "{msg}");
}

/// An fn-pointer local named like the variant claims the call as well:
/// emission calls through it (try_emit_indirect_call) before resolving any
/// name. The tracer types that call as Unit, as it does a variant call, so it
/// must still be checked here; emission would store the Opt<i64> it returns
/// in the Opt<i32> slot.
#[test]
fn fn_pointer_local_named_like_variant_still_checked() {
    let decl = "fn g(x: i32) -> Opt<i64> {\n        return Opt::None\n    }";
    let msg = compile_err(&opt_with(decl, "let Some = g;", "Some(v)"), "local Some = g");
    assert!(msg.contains(WRAP_ARG_REJECTED), "{msg}");
}

/// Only a variant of the slot's enum is left to emission: a bare call to a
/// struct, or to another enum's variant, is still rejected here.
#[test]
fn bare_call_to_non_slot_variant_still_checked() {
    for (decl, call) in [
        ("struct Pt {\n        x: i32,\n    }", "Pt(v)"),
        ("enum Beta<T> {\n        Tagged(T),\n        Empty,\n    }", "Tagged(v)"),
    ] {
        let msg = compile_err(&opt_with(decl, "", call), call);
        assert!(msg.contains(WRAP_ARG_REJECTED), "{call}: {msg}");
    }
}

/// Only a bare call is left to emission: a qualified one names its own enum,
/// which need not be the slot's (`Other::Some` isn't Opt's).
#[test]
fn qualified_call_to_another_enum_still_checked() {
    let decl = "enum Other<T> {\n        Some(T),\n        Nothing,\n    }";
    let msg = compile_err(&opt_with(decl, "", "Other::Some(v)"), "Other::Some(v)");
    assert!(msg.contains(WRAP_ARG_REJECTED), "{msg}");
}

/// A name an import binds is not a variant name in a payload either: the
/// call reaches emission, which reports the import's target as undefined,
/// as it does for the same call outside a payload
/// (unqualified_enum_ctor_test.rs, test_imported_name_is_not_taken_for_a_variant).
#[test]
fn imported_name_in_payload_is_not_taken_for_a_variant() {
    let code = r#"
        package main

        use std.core.option.{Tagged};

        enum Beta<T> {
            Tagged(T),
            Empty,
        }

        fn make(v: i32) -> Beta<Beta<i32>> {
            return Beta::Tagged(Tagged(v))
        }

        pub fn main() -> i32 {
            match make(5) {
                Beta::Tagged(b) => { return 0; }
                Beta::Empty => { return 1; }
            }
        }
    "#;
    let msg = compile_err(code, "import binds Tagged");
    assert!(msg.contains("Undefined function or symbol: 'std__core__option__Tagged'"), "{msg}");
}

/// A deferred call must build the variant it names. Resolution tries the
/// zeroed intrinsic first, which would store a zero Cmd<i32> instead of
/// Cmd::zeroed(5).
#[test]
fn deferred_call_resolving_to_an_intrinsic_rejected() {
    let decl = "enum Cmd<T> {\n        zeroed(T),\n        Quit,\n    }";
    let code = opt_with(decl, "", "zeroed(v)").replace("Opt<Opt<i32>>", "Opt<Cmd<i32>>");
    let msg = compile_err(&code, "zeroed(v)");
    assert!(msg.contains("resolves to intrinsic zeroed"), "{msg}");
}

/// A deferred call passes one argument per payload field: `None(v)` would
/// drop v, and `Some()` or `Some(v, 99)` would emit invalid MLIR.
#[test]
fn deferred_call_argument_count_checked() {
    for call in ["None(v)", "Some()", "Some(v, 99)"] {
        let msg = compile_err(&opt_with("", "", call), call);
        assert!(msg.contains("Constructor argument count mismatch"), "{call}: {msg}");
    }
}

/// Type arguments on a bare call aren't checked against the slot, so such a
/// call isn't left to emission: `Some<i64>(v)` in an Opt<i32> slot is still
/// rejected here.
#[test]
fn bare_call_with_type_arguments_still_checked() {
    let code = opt_with("", "", "Some<i64>(v)").replace("fn wrap(v: i32)", "fn wrap(v: i64)");
    let msg = compile_err(&code, "Some<i64>(v)");
    assert!(msg.contains(WRAP_ARG_REJECTED), "{msg}");
}

/// Compiles `Option::Some(Some(v))` under `-> Option<Option<i32>>` with the
/// saltc binary, next to `modules` (path, source) and importing `use_line`,
/// and returns its stderr, which must report a failure.
fn saltc_rejects_with_modules(name: &str, modules: &[(&str, &str)], use_line: &str) -> String {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    for (path, source) in modules {
        std::fs::write(dir.join(path), source).unwrap();
    }
    let main = NESTED_OPTION.replace("DECL", &format!("use std.core.option.*;\n    {use_line}"))
        .replace("TYPE", "Option").replace("BODY", "Option::Some(Some(v))");
    std::fs::write(dir.join("main.salt"), main).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_saltc"))
        .current_dir(&dir)
        .args(["main.salt", "-o", "out.mlir", "--root"])
        .arg(&dir)
        .output()
        .expect("failed to spawn saltc");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "{name} compiled:\n{stderr}");
    stderr
}

/// A function in any loaded module can claim a bare name at emission without
/// the tracer seeing it. lib.conv is loaded only through lib.mid, and its
/// `Some` returns Option<i64>: the deferred call must fail rather than store
/// that value in the Option<i32> slot.
#[test]
fn deferred_call_resolving_to_a_module_function_rejected() {
    let conv = "package lib.conv\n\nuse std.core.option.*;\n\npub struct Marker {\n    x: i32,\n}\n\n\
                pub fn Some(v: i32) -> Option<i64> {\n    let w: i64 = 99;\n    return Option::Some(w);\n}\n";
    let mid = "package lib.mid\n\nuse lib.conv.Marker;\n\npub struct Thing {\n    y: i32,\n}\n";
    let modules = [("lib/conv.salt", conv), ("lib/mid.salt", mid)];
    let stderr = saltc_rejects_with_modules("enum_ctor_payload_module_fn", &modules, "use lib.mid.Thing;");
    assert!(stderr.contains("resolves to function lib__conv__Some"), "{stderr}");
}

/// Resolution can build a variant of another enum: `use lib.alt.*` sends a
/// bare `Some` to lib.alt's own enum named Some. The built value's MLIR type
/// must be the slot's.
#[test]
fn deferred_call_building_another_enum_rejected() {
    let alt = "package lib.alt\n\npub enum Some<T> {\n    Some(T),\n    Nope,\n}\n";
    let stderr = saltc_rejects_with_modules("enum_ctor_payload_other_enum", &[("lib/alt.salt", alt)], "use lib.alt.*;");
    assert!(stderr.contains("resolves to lib__alt__Some::Some of type Concrete(\"lib__alt__Some\", [I32])"), "{stderr}");
}
