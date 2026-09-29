// Bare unit variants (`return Empty`, not `return Beta::Empty`) resolve
// against the expected enum type when that enum declares the variant, as
// bare payload-variant calls do (unqualified_enum_ctor_test.rs).
//
// Before, the value came from the first enum, sorted by name, declaring a
// variant of that name. With `enum Alpha` and `enum Beta<T>` both declaring
// `Empty`, `return Empty` from a function returning Beta<i32> built
// Alpha::Empty and failed with "Numeric promotion not supported from
// Enum(\"main__Alpha\") to Concrete(\"main__Beta\", [I32])". Two
// specializations of one template collided the same way: `return None` from
// a function returning Option<i64> built the Option_i32 value when the
// program also used Option<i32>.
//
// Without an expected enum that declares the name, the first enum by name
// still wins.

use saltc::compile;

/// Alpha sorts before Beta and declares `Empty` as variant 0; Beta declares
/// it as variant 1, so a built tag shows which enum a bare `Empty` came from.
const ENUMS: &str = r#"
    package main

    enum Alpha {
        Empty,
        Tagged(i32),
    }

    enum Beta<T> {
        Tagged(T),
        Empty,
    }

    fn take(b: Beta<i32>) -> i32 {
        match b {
            Beta::Tagged(v) => { return v; }
            Beta::Empty => { return 7; }
        }
    }

    fn mk(v: i32) -> Beta<i32> {
        return Beta::Tagged(v)
    }
"#;

fn with_enums(rest: &str) -> String {
    format!("{ENUMS}\n{rest}")
}

/// The MLIR from the definition whose header contains `header` up to the
/// next function.
fn fn_body<'a>(mlir: &'a str, header: &str) -> &'a str {
    let start = mlir.find(header).unwrap_or_else(|| panic!("{header} not emitted:\n{mlir}"));
    let rest = &mlir[start..];
    rest[1..].find("func.func").map_or(rest, |end| &rest[..end + 1])
}

/// (MLIR type, tag) of each bare unit-variant value built in `body`.
fn unit_variants_built(body: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = body.lines().map(str::trim).collect();
    let tag_of = |reg: &str| {
        let prefix = format!("{reg} = arith.constant ");
        lines.iter().find_map(|l| l.strip_prefix(prefix.as_str())).map(|v| v.to_string())
    };
    lines.iter()
        .filter(|l| l.starts_with("%enum_val_") && l.contains(" = llvm.insertvalue "))
        .map(|l| {
            let ty = l.rsplit(" : ").next().unwrap_or_default().to_string();
            let tag_reg = l.split("llvm.insertvalue ").nth(1).and_then(|s| s.split(',').next());
            (ty, tag_reg.and_then(tag_of).unwrap_or_default())
        })
        .collect()
}

fn built_in(mlir: &str, header: &str) -> Vec<(String, String)> {
    unit_variants_built(fn_body(mlir, header))
}

fn beta_empty() -> Vec<(String, String)> {
    vec![("!struct_main__Beta_i32".to_string(), "1 : i32".to_string())]
}

/// The reported repro: Alpha is not generic, Beta is, and both declare
/// `Tagged` and `Empty` in the same order. The bare payload call already
/// built Beta::Tagged; the bare unit variant built Alpha::Empty.
#[test]
fn test_bare_unit_variant_takes_the_expected_enum() {
    let code = r#"
        package main

        enum Alpha {
            Tagged(i32),
            Empty,
        }

        enum Beta<T> {
            Tagged(T),
            Empty,
        }

        fn make_tagged(v: i32) -> Beta<i32> {
            return Tagged(v)
        }

        fn make_empty() -> Beta<i32> {
            return Empty
        }

        pub fn main() -> i32 {
            match make_empty() {
                Beta::Tagged(v) => { return v; }
                Beta::Empty => { return 0; }
            }
        }
    "#;
    let mlir = compile(code, false, None, true).expect("return Empty builds Beta::Empty");
    assert_eq!(built_in(&mlir, "func.func private @main__make_empty()"), beta_empty());
}

/// The tag comes from the expected enum too, not only the type.
#[test]
fn test_bare_unit_variant_builds_the_expected_variant() {
    let code = with_enums(r#"
        fn make_empty() -> Beta<i32> {
            return Empty
        }

        pub fn main() -> i32 {
            return take(make_empty());
        }
    "#);
    let mlir = compile(&code, false, None, true).expect("compiles");
    assert_eq!(built_in(&mlir, "func.func private @main__make_empty()"), beta_empty());
}

/// Every place that hands the value an expected type anchors it. Each of
/// these failed before with the Alpha value built. A plain function-call
/// argument gets no expected type (emit_function_args passes None), so
/// `take(Empty)` still builds Alpha::Empty and fails.
#[test]
fn test_bare_unit_variant_in_each_expected_type_context() {
    let cases = [
        ("let", "let b: Beta<i32> = Empty;\n return take(b);"),
        ("paren", "let b: Beta<i32> = (Empty);\n return take(b);"),
        ("struct field", "let h = Holder { b: Empty };\n return take(h.b);"),
        ("assignment", "let mut b: Beta<i32> = mk(3);\n b = Empty;\n return take(b);"),
        ("if branch", "let c = 1;\n let b: Beta<i32> = if c > 0 { Empty } else { mk(2) };\n return take(b);"),
        ("array element", "let arr: [Beta<i32>; 2] = [Empty, Empty];\n return take(arr[0]);"),
        ("method argument", "let h = Holder { b: mk(1) };\n return h.put(Empty);"),
    ];
    let holder = r#"
        struct Holder {
            b: Beta<i32>,
        }

        impl Holder {
            fn put(self, b: Beta<i32>) -> i32 { return take(b); }
        }
    "#;
    for (context, body) in cases {
        let code = with_enums(&format!("{holder}\npub fn main() -> i32 {{\n {body}\n}}"));
        let mlir = compile(&code, false, None, true)
            .unwrap_or_else(|e| panic!("{context}: {e:#}"));
        let built = built_in(&mlir, "func.func public @main()");
        assert!(!built.is_empty() && built.iter().all(|b| *b == beta_empty()[0]), "{context}: {built:?}");
    }
}

/// A comparison hands the left operand's type to the right one. `g` is typed
/// `Type::Enum("main__Gamma")` (from the bare `OnlyG`), unlike the
/// `Concrete` return and annotation types above.
#[test]
fn test_bare_unit_variant_compared_with_an_enum_typed_local() {
    let code = r#"
        package main

        enum Alpha {
            Empty,
            Tagged(i32),
        }

        enum Gamma {
            Tagged(i32),
            Empty,
            OnlyG,
        }

        pub fn main() -> i32 {
            let mut g = OnlyG;
            g = Gamma::Empty;
            if g == Empty { return 1; }
            return 0;
        }
    "#;
    let mlir = compile(code, false, None, true).expect("g == Empty compares Gamma values");
    let gamma = |tag: &str| ("!struct_main__Gamma".to_string(), format!("{tag} : i32"));
    assert_eq!(built_in(&mlir, "func.func public @main()"), vec![gamma("2"), gamma("1")]);
}

/// A constructor payload slot supplies the expected type as well.
#[test]
fn test_bare_unit_variant_as_a_constructor_payload() {
    let code = with_enums(r#"
        enum Wrap {
            W(Beta<i32>),
            Nothing,
        }

        fn wrap_empty() -> Wrap {
            return Wrap::W(Empty)
        }

        pub fn main() -> i32 {
            match wrap_empty() {
                Wrap::W(b) => { return take(b); }
                Wrap::Nothing => { return 9; }
            }
        }
    "#);
    let mlir = compile(&code, false, None, true).expect("compiles");
    assert_eq!(built_in(&mlir, "func.func private @main__wrap_empty()"), beta_empty());
}

/// Two specializations of one template: Opt_i32 sorts first, so a bare
/// `None` for an Opt<i64> slot used to build the Opt_i32 value.
#[test]
fn test_bare_unit_variant_picks_the_expected_specialization() {
    let code = r#"
        package main

        enum Opt<T> {
            Some(T),
            None,
        }

        fn small(v: i32) -> Opt<i32> {
            if v > 0 { return Some(v); }
            return None
        }

        fn large(v: i64) -> Opt<i64> {
            if v > 0 { return Some(v); }
            return None
        }

        pub fn main() -> i32 {
            let s = small(1);
            match large(2) {
                Opt::Some(v) => { return 1; }
                Opt::None => { return 0; }
            }
        }
    "#;
    let mlir = compile(code, false, None, true).expect("return None builds Opt<i64>::None");
    let none_of = |ty: &str| vec![(ty.to_string(), "1 : i32".to_string())];
    assert_eq!(built_in(&mlir, "func.func private @main__small("), none_of("!struct_main__Opt_i32"));
    assert_eq!(built_in(&mlir, "func.func private @main__large("), none_of("!struct_main__Opt_i64"));
}

/// Same collision on std's Option.
#[test]
fn test_bare_none_picks_the_expected_option() {
    let code = r#"
        package main

        use std.core.option.*;

        fn small(v: i32) -> Option<i32> {
            if v > 0 { return Some(v); }
            return None
        }

        fn large(v: i64) -> Option<i64> {
            if v > 0 { return Some(v); }
            return None
        }

        pub fn main() -> i32 {
            let s = small(1);
            match large(2) {
                Some(v) => { return 1; }
                None => { return 0; }
            }
        }
    "#;
    let mlir = compile(code, false, None, true).expect("return None builds Option<i64>::None");
    let built = built_in(&mlir, "func.func private @main__large(");
    assert_eq!(built.len(), 1, "{built:?}");
    assert_eq!(built[0].0, "!struct_std__core__option__Option_i64");
}

/// Without a package the enums are unqualified, and a non-generic enum's
/// expected type arrives as `Concrete("Beta", [])`.
#[test]
fn test_bare_unit_variant_of_a_non_generic_enum() {
    let code = r#"
        enum Alpha {
            Empty,
            Tagged(i32),
        }

        enum Beta {
            Tagged(i32),
            Empty,
        }

        fn make_empty() -> Beta {
            return Empty
        }

        pub fn main() -> i32 {
            match make_empty() {
                Beta::Tagged(v) => { return v; }
                Beta::Empty => { return 2; }
            }
        }
    "#;
    let mlir = compile(code, false, None, true).expect("compiles");
    let built = built_in(&mlir, "func.func private @make_empty()");
    assert_eq!(built, vec![("!struct_Beta".to_string(), "1 : i32".to_string())]);
}

/// With no expected type the first enum by name still wins: `let a = Empty`
/// is Alpha::Empty, as before.
#[test]
fn test_bare_unit_variant_without_expected_type_is_unchanged() {
    let code = with_enums(r#"
        pub fn main() -> i32 {
            let a = Empty;
            match a {
                Alpha::Empty => { return 1; }
                Alpha::Tagged(v) => { return v; }
            }
        }
    "#);
    let mlir = compile(&code, false, None, true).expect("compiles");
    let built = built_in(&mlir, "func.func public @main()");
    assert_eq!(built, vec![("!struct_main__Alpha".to_string(), "0 : i32".to_string())]);
}

/// The expected enum decides what the name means: when it declares the name
/// as a payload variant, the bare name is that variant without arguments,
/// not Alpha's unit variant of the same name.
#[test]
fn test_bare_payload_variant_of_the_expected_enum_is_rejected() {
    let code = r#"
        package main

        enum Alpha {
            X,
        }

        enum Beta {
            X(i32),
            Y,
        }

        fn f() -> Beta {
            return X
        }

        pub fn main() -> i32 {
            match f() {
                Beta::X(v) => { return v; }
                Beta::Y => { return 2; }
            }
        }
    "#;
    let err = compile(code, false, None, true).expect_err("X needs its payload");
    let msg = format!("{err:#}");
    assert!(msg.contains("Cannot use tuple variant 'X' as value without arguments"), "{msg}");
}

/// A variant the expected enum lacks still comes from the first enum by
/// name, and the mismatch is reported against that enum as before.
#[test]
fn test_variant_the_expected_enum_lacks_is_unchanged() {
    let code = with_enums(r#"
        enum Gamma {
            Other,
        }

        fn f() -> Beta<i32> {
            return Other
        }

        pub fn main() -> i32 {
            return take(f());
        }
    "#);
    let err = compile(&code, false, None, true).expect_err("Gamma::Other is not a Beta<i32>");
    let msg = format!("{err:#}");
    assert!(msg.contains(r#"Enum("main__Gamma")"#), "{msg}");
}

/// Only a single-segment path is a bare variant. `Foo::Bar` is looked up
/// by its first segment too, and Beta declares a `Foo`; anchoring that on
/// the expected type would quietly build Beta::Foo for Foo::Bar.
#[test]
fn test_qualified_path_is_not_taken_for_a_bare_variant() {
    let code = r#"
        package main

        enum Alpha {
            Foo,
            Other,
        }

        enum Beta {
            Other2,
            Foo,
        }

        enum Foo {
            Bar,
        }

        fn f() -> Beta {
            return Foo::Bar
        }

        pub fn main() -> i32 {
            match f() {
                Beta::Other2 => { return 1; }
                Beta::Foo => { return 2; }
            }
        }
    "#;
    let err = compile(code, false, None, true).expect_err("Foo::Bar is not a Beta");
    let msg = format!("{err:#}");
    assert!(msg.contains("Numeric promotion not supported"), "{msg}");
}
