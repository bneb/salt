// Regression tests: QUALIFIED TRAIT IDENTITY at the method-dispatch layer.
//
// Two modules may each define a trait with the SAME bare name; identity at
// dispatch must come from the fully-qualified module path plus the
// implementing type, never from the bare name alone.
//
// Locked here:
// 1. Same-named traits in two modules, impls on distinct types - each call
//    resolves to its own module's impl (distinct qualified symbols).
// 2. Generic variant: same-named traits implemented for generic structs.
// 3. Single-module trait usage stays unchanged.
// 4. A trait-method body calling a function IMPORTED from another module
//    resolves by qualified path (was misrouted into the intrinsic
//    dispatcher by an over-broad name-prefix heuristic).
// 5. A trait-method body constructing an IMPORTED generic enum (std
//    Result) canonicalizes to the enum's true home module even though
//    hydration tasks do not carry the defining module's wildcard imports.
use saltc::compile;

const IT_ROOT: &str = "target/qi_trait_identity_it";

const ALPHA_GREET: &str = r#"
    package target.qi_trait_identity_it.qti_alpha_greet.agreet

    struct AlphaBox { v: i64 }

    trait Greeter {
        fn greet(&self) -> i64;
    }

    impl Greeter for AlphaBox {
        fn greet(&self) -> i64 {
            let v = self.v;
            return v + 100000;
        }
    }

    pub fn make_alpha(n: i64) -> AlphaBox {
        return AlphaBox { v: n };
    }
"#;

const BETA_GREET: &str = r#"
    package target.qi_trait_identity_it.qti_beta_greet.bgreet

    struct BetaBox { v: i64 }

    trait Greeter {
        fn greet(&self) -> i64;
    }

    impl Greeter for BetaBox {
        fn greet(&self) -> i64 {
            let v = self.v;
            return v + 200000;
        }
    }

    pub fn make_beta(n: i64) -> BetaBox {
        return BetaBox { v: n };
    }
"#;

const ALPHA_SUPPLY: &str = r#"
    package target.qi_trait_identity_it.qti_alpha_gen.asupply

    struct WrapA<T> { v: T }

    trait Supplier {
        fn supply(&self) -> i32;
    }

    impl<T> Supplier for WrapA<T> {
        fn supply(&self) -> i32 {
            return self.v as i32 + 300000;
        }
    }

    pub fn make_wrap_a(n: i64) -> WrapA<i64> {
        return WrapA { v: n };
    }
"#;

const BETA_SUPPLY: &str = r#"
    package target.qi_trait_identity_it.qti_beta_gen.bsupply

    struct WrapB<T> { v: T }

    trait Supplier {
        fn supply(&self) -> i32;
    }

    impl<T> Supplier for WrapB<T> {
        fn supply(&self) -> i32 {
            return self.v as i32 + 400000;
        }
    }

    pub fn make_wrap_b(n: i64) -> WrapB<i64> {
        return WrapB { v: n };
    }
"#;

const HELPERS_MOD: &str = r#"
    package target.qi_trait_identity_it.qti_helper.helpers

    pub fn double_it(n: i64) -> i64 {
        return n * 2;
    }
"#;

const CALLER_MOD: &str = r#"
    package target.qi_trait_identity_it.qti_caller.users

    use target.qi_trait_identity_it.qti_helper.helpers;

    struct Caller { v: i64 }

    trait Invoker {
        fn invoke(&self) -> i64;
    }

    impl Invoker for Caller {
        fn invoke(&self) -> i64 {
            return helpers.double_it(self.v);
        }
    }

    pub fn make_caller(n: i64) -> Caller {
        return Caller { v: n };
    }
"#;

const FINDER_MOD: &str = r#"
    package target.qi_trait_identity_it.qti_result_user.finder

    use std.core.result.*;
    use std.status.Status;

    struct Holder { v: i64 }

    trait Finder {
        fn find(&self) -> Result<i64>;
    }

    impl Finder for Holder {
        fn find(&self) -> Result<i64> {
            let v = self.v;
            if v > 0 {
                return Result::Ok(v);
            }
            return Result::Err(Status::with_detail(3, 0));
        }
    }

    pub fn make_holder(n: i64) -> Holder {
        return Holder { v: n };
    }
"#;

fn write_module(slug: &str, name: &str, src: &str) -> String {
    let dir = std::env::current_dir()
        .expect("cwd available")
        .join(IT_ROOT)
        .join(slug);
    std::fs::create_dir_all(&dir).expect("create module dir");
    std::fs::write(dir.join(format!("{}.salt", name)), src).expect("write module");
    format!("{}.{}.{}", IT_ROOT.replace('/', "."), slug, name)
}

/// Definition chunk of one emitted function: the text of the func.func
/// unit whose signature names the given symbol as the final mangle segment
/// (bare or `pkg__mod__Symbol`), so per-body sentinels pin to the exact symbol.
fn function_body<'a>(mlir: &'a str, symbol: &str) -> &'a str {
    let call = format!("{}(", symbol);
    for unit in mlir.split("func.func ").skip(1) {
        let sig_end = unit.find('{').unwrap_or(unit.len());
        if signature_names(&unit[..sig_end], &call) {
            return unit;
        }
    }
    ""
}

/// True when the signature references @...Symbol( with the symbol as the
/// last mangle segment (preceding char is the @ itself or a __ separator).
fn signature_names(sig: &str, call: &str) -> bool {
    sig.match_indices(call).any(|(at, _)| {
        sig[..at].ends_with("__") || sig[..at].ends_with('@')
    })
}

fn cleanup(slug: &str) {
    let dir = std::env::current_dir()
        .expect("cwd available")
        .join(IT_ROOT)
        .join(slug);
    let _ = std::fs::remove_dir_all(dir);
}

/// (1) Two same-named Greeter traits in two modules; impls on distinct
///     types carry distinct body sentinels so a cross-module mixup would
///     emit the wrong constant under the right symbol.
#[test]
fn same_named_traits_in_two_modules_coexist() {
    let ns_a = write_module("qti_alpha_greet", "agreet", ALPHA_GREET);
    let ns_b = write_module("qti_beta_greet", "bgreet", BETA_GREET);
    let code = format!(r#"
        package main

        use {ns_a};
        use {ns_b};

        pub fn main() -> i64 {{
            let a = agreet.make_alpha(1);
            let b = bgreet.make_beta(2);
            let ra = a.greet();
            let rb = b.greet();
            return 0;
        }}
    "#);
    let result = compile(&code, false, None, true);
    cleanup("qti_alpha_greet");
    cleanup("qti_beta_greet");
    assert!(result.is_ok(), "same-named traits coexistence failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(mlir.contains("AlphaBox__greet"), "alpha impl symbol missing");
    assert!(mlir.contains("BetaBox__greet"), "beta impl symbol missing");
    // The sentinel constant must live INSIDE its own impl's body: a
    // bare-name collision would emit one body under the other symbol.
    let alpha_body = function_body(&mlir, "AlphaBox__greet");
    assert!(alpha_body.contains("100000"), "alpha symbol lost its own body");
    let beta_body = function_body(&mlir, "BetaBox__greet");
    assert!(beta_body.contains("200000"), "beta symbol lost its own body");
}

/// (2) Generic variant: same-named traits implemented for generic structs,
///     monomorphized at the call site.
#[test]
fn same_named_traits_on_generic_structs() {
    let ns_a = write_module("qti_alpha_gen", "asupply", ALPHA_SUPPLY);
    let ns_b = write_module("qti_beta_gen", "bsupply", BETA_SUPPLY);
    let code = format!(r#"
        package main

        use {ns_a};
        use {ns_b};

        pub fn main() -> i32 {{
            let a = asupply.make_wrap_a(3);
            let b = bsupply.make_wrap_b(4);
            let ra = a.supply();
            let rb = b.supply();
            return 0;
        }}
    "#);
    let result = compile(&code, false, None, true);
    cleanup("qti_alpha_gen");
    cleanup("qti_beta_gen");
    assert!(result.is_ok(), "generic same-named traits failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(mlir.contains("WrapA_i64__supply"), "alpha generic impl symbol missing");
    assert!(mlir.contains("WrapB_i64__supply"), "beta generic impl symbol missing");
    // Monomorphized bodies must stay bound to their own generic structs.
    let alpha_body = function_body(&mlir, "WrapA_i64__supply");
    assert!(alpha_body.contains("300000"), "WrapA supply lost its own body");
    let beta_body = function_body(&mlir, "WrapB_i64__supply");
    assert!(beta_body.contains("400000"), "WrapB supply lost its own body");
}

/// (3) Single-module trait usage is unchanged by qualified identity work.
#[test]
fn single_module_trait_usage_unchanged() {
    let code = r#"
        package main

        struct Local { v: i64 }

        trait Greeter {
            fn greet(&self) -> i64;
        }

        impl Greeter for Local {
            fn greet(&self) -> i64 {
                let v = self.v;
                return v + 500000;
            }
        }

        pub fn main() -> i64 {
            let l = Local { v: 5 };
            return l.greet();
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "single-module trait failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(mlir.contains("Local__greet"), "local impl symbol missing");
    let local_body = function_body(&mlir, "main__Local__greet");
    assert!(local_body.contains("500000"), "local impl body missing");
}

/// (4) A trait-method body calling a function IMPORTED from another module
///     must resolve by qualified path. Regression: canonicalized names like
///     `target__...__helper` were swallowed by an over-broad intrinsic
///     name-prefix heuristic and died as "Intrinsic not found".
#[test]
fn trait_method_calls_imported_function_by_qualified_path() {
    let _ns_h = write_module("qti_helper", "helpers", HELPERS_MOD);
    let ns_m = write_module("qti_caller", "users", CALLER_MOD);
    let code = format!(r#"
        package main

        use {ns_m};

        pub fn main() -> i64 {{
            let c = users.make_caller(21);
            return c.invoke();
        }}
    "#);
    let result = compile(&code, false, None, true);
    cleanup("qti_helper");
    cleanup("qti_caller");
    assert!(result.is_ok(), "qualified call inside trait body failed: {:?}", result.err());
    assert!(
        result.unwrap().contains("double_it"),
        "imported callee symbol missing from MLIR"
    );
}

/// (5) A trait-method body constructing an IMPORTED generic enum (std
///     Result) must canonicalize to the enum's true home module. The
///     hydration task for a trait method does not carry the defining
///     module's wildcard imports, so the caller-local package fallback
///     used to mis-prefix the enum and resolution failed outright.
#[test]
fn trait_method_constructs_imported_generic_enum() {
    let ns = write_module("qti_result_user", "finder", FINDER_MOD);
    let code = format!(r#"
        package main

        use {ns};

        pub fn main() -> i64 {{
            let h = finder.make_holder(9);
            let r = h.find();
            return 0;
        }}
    "#);
    let result = compile(&code, false, None, true);
    cleanup("qti_result_user");
    assert!(result.is_ok(), "enum ctor inside trait body failed: {:?}", result.err());
}
