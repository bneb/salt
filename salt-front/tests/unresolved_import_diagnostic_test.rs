// Regression test: an unresolvable `use` must fail fast at module-loading
// time with a clear [E008] "Could not resolve module" diagnostic, not
// silently succeed and surface much later as a misleading "Undefined
// function or symbol" error at the call site that happens to reference it
// (see codegen/mod.rs load_modules, which used to discard
// ModuleLoader::load_module's Result entirely).
use saltc::compile;

#[test]
fn unresolvable_import_reports_module_resolution_error() {
    let src = r#"
        package main

        use definitely.not.a.real.module.Thing;

        pub fn main() -> i32 {
            return 0;
        }
    "#;
    let result = compile(src, false, None, true);
    assert!(result.is_err(), "compiling with an unresolvable import must fail");
    let err = result.unwrap_err().to_string();
    assert!(err.contains("[E008]"), "expected E008 diagnostic, got: {}", err);
    assert!(err.contains("Could not resolve module"), "expected resolution failure detail, got: {}", err);
    assert!(err.contains("definitely.not.a.real.module.Thing"), "expected the failing namespace named, got: {}", err);
    assert!(!err.contains("Undefined function or symbol"),
            "must fail at import resolution, not downstream symbol lookup: {}", err);
}

#[test]
fn multiple_unresolvable_imports_are_all_reported_together() {
    let src = r#"
        package main

        use nope.one.Foo;
        use nope.two.Bar;

        pub fn main() -> i32 {
            return 0;
        }
    "#;
    let result = compile(src, false, None, true);
    assert!(result.is_err(), "compiling with unresolvable imports must fail");
    let err = result.unwrap_err().to_string();
    assert!(err.contains("nope.one.Foo"), "missing first unresolved import in diagnostic: {}", err);
    assert!(err.contains("nope.two.Bar"), "missing second unresolved import in diagnostic: {}", err);
}

#[test]
fn valid_imports_still_compile_cleanly() {
    let src = r#"
        package main

        use std.core.result.Result;

        pub fn main() -> i32 {
            return 0;
        }
    "#;
    let result = compile(src, false, None, true);
    assert!(result.is_ok(), "a valid stdlib import must still compile: {:?}", result.err());
}
