// Regression tests: INHERITED TRAIT DEFAULT METHODS across MODULE BOUNDARIES.
//
// Companion to trait_default_methods_test.rs (single-file cases). Here the
// IMPL block — and sometimes the trait itself — lives inside an IMPORTED
// module while the call site stays in the entry file.
//
// Why this needs its own harness: module impl blocks are snapshotted into
// `ModuleInfo::impls` at LOAD time, before trait-default expansion runs, so
// the registry registered stale method sets and a defaulted call failed with
// "Method '<name>' not found for type". Expansion now rewrites the entry
// AST, every loaded module's AST, AND the registry snapshots in one choke
// point (`codegen::trait_defaults::expand_trait_defaults`).
//
// No pre-existing two-file integration harness existed, so each test writes
// its module under the gitignored `target/` tree (a module-loader search
// root via the compiler's cwd-based roots) and compiles through the public
// `saltc::compile` entry point.
use saltc::compile;

const IT_ROOT: &str = "target/cross_module_trait_defaults_it";

/// Writes one module file reachable as namespace
/// `target.cross_module_trait_defaults_it.<slug>.<name>` and returns that
/// namespace. Uses the compiler's cwd-relative search roots, exactly like
/// production module loading.
fn write_module(slug: &str, name: &str, src: &str) -> String {
    let dir = std::env::current_dir()
        .expect("cwd available")
        .join(IT_ROOT)
        .join(slug);
    std::fs::create_dir_all(&dir).expect("create module dir");
    std::fs::write(dir.join(format!("{}.salt", name)), src).expect("write module");
    format!("{}.{}.{}", IT_ROOT.replace('/', "."), slug, name)
}

fn cleanup(slug: &str) {
    let dir = std::env::current_dir()
        .expect("cwd available")
        .join(IT_ROOT)
        .join(slug);
    let _ = std::fs::remove_dir_all(dir);
}

/// (1) Trait AND impl live in the imported module; the impl omits the
///     defaulted method; the entry file calls it. The default body must be
///     monomorphized under the implementing type.
#[test]
fn module_impl_omitting_default_resolves_from_entry() {
    let slug = "trait_and_impl_in_module";
    let ns = write_module(
        slug,
        "widgets",
        r#"
            package target.cross_module_trait_defaults_it.trait_and_impl_in_module.widgets

            struct Widget { id: i64 }

            trait Describe {
                fn tag(&self) -> i64;
                fn describe(&self) -> i64 {
                    return 1111111;
                }
            }

            impl Describe for Widget {
                fn tag(&self) -> i64 {
                    return self.id;
                }
            }

            pub fn make_widget(n: i64) -> Widget {
                return Widget { id: n };
            }
        "#,
    );
    let code = format!(r#"
        package main

        use {ns};

        pub fn main() -> i32 {{
            let w = widgets.make_widget(7);
            let d = w.describe();
            return 0;
        }}
    "#);
    let result = compile(&code, false, None, true);
    cleanup(slug);
    assert!(result.is_ok(), "cross-module inherited default failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(mlir.contains("Widget__describe"),
            "inherited default must be emitted as Widget__describe");
    assert!(mlir.contains("1111111"), "default body sentinel must reach MLIR");
}

/// (2) Trait declared in the ENTRY file, impl inside the module omits the
///     default. Defaults are collected across files, so the module impl must
///     still inherit.
#[test]
fn entry_trait_inherited_by_module_impl() {
    let slug = "entry_trait_module_impl";
    let ns = write_module(
        slug,
        "gauges",
        r#"
            package target.cross_module_trait_defaults_it.entry_trait_module_impl.gauges

            struct Gauge { v: i64 }

            impl Describe for Gauge {
                fn tag(&self) -> i64 {
                    return self.v;
                }
            }

            pub fn make_gauge(n: i64) -> Gauge {
                return Gauge { v: n };
            }
        "#,
    );
    let code = format!(r#"
        package main

        use {ns};

        trait Describe {{
            fn tag(&self) -> i64;
            fn describe(&self) -> i64 {{
                return 2222222;
            }}
        }}

        pub fn main() -> i32 {{
            let g = gauges.make_gauge(3);
            let d = g.describe();
            return 0;
        }}
    "#);
    let result = compile(&code, false, None, true);
    cleanup(slug);
    assert!(result.is_ok(), "module impl inheriting entry-trait default failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(mlir.contains("Gauge__describe"),
            "inherited default must be emitted as Gauge__describe");
    assert!(mlir.contains("2222222"), "default body sentinel must reach MLIR");
}

/// (3) Cross-module explicit override wins: the default body must NOT be
///     emitted for the overriding type (and must not collide as a duplicate
///     symbol — successful MLIR verification implies uniqueness).
#[test]
fn cross_module_override_wins_over_default() {
    let slug = "module_override_wins";
    let ns = write_module(
        slug,
        "meters",
        r#"
            package target.cross_module_trait_defaults_it.module_override_wins.meters

            struct Meter { base: i64 }

            trait Describe {
                fn tag(&self) -> i64;
                fn describe(&self) -> i64 {
                    return 3333333;
                }
            }

            impl Describe for Meter {
                fn tag(&self) -> i64 {
                    return self.base;
                }
                fn describe(&self) -> i64 {
                    return 4444444;
                }
            }

            pub fn make_meter(n: i64) -> Meter {
                return Meter { base: n };
            }
        "#,
    );
    let code = format!(r#"
        package main

        use {ns};

        pub fn main() -> i32 {{
            let m = meters.make_meter(5);
            let d = m.describe();
            return 0;
        }}
    "#);
    let result = compile(&code, false, None, true);
    cleanup(slug);
    assert!(result.is_ok(), "cross-module override failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(mlir.contains("Meter__describe"), "override must be emitted");
    assert!(mlir.contains("4444444"), "override body must be emitted");
    assert!(!mlir.contains("3333333"),
            "default body must NOT be emitted when the module impl overrides it");
}

/// (4) An inherited default whose BODY references other modules through
///     QUALIFIED names (`shape.make_stamp`) resolves and emits under the
///     implementor's imports. Locks the working slice of import-context
///     handling: qualified foreign references are legal in inherited bodies.
#[test]
fn inherited_default_with_qualified_foreign_refs() {
    let slug = "qualified_foreign_refs";
    let shape_ns = write_module(
        slug,
        "shape",
        r#"
            package target.cross_module_trait_defaults_it.qualified_foreign_refs.shape

            pub struct Stamp { pub code: i64 }

            pub fn make_stamp(c: i64) -> Stamp {
                return Stamp { code: c };
            }

            pub fn stamp_code(s: Stamp) -> i64 {
                return s.code;
            }
        "#,
    );
    let impls_ns = write_module(
        slug,
        "boxes",
        r#"
            package target.cross_module_trait_defaults_it.qualified_foreign_refs.boxes

            use target.cross_module_trait_defaults_it.qualified_foreign_refs.shape;

            struct BoxQ { n: i64 }

            impl Stamped for BoxQ {
                fn raw(&self) -> i64 {
                    return self.n;
                }
            }

            pub fn make_boxq(n: i64) -> BoxQ {
                return BoxQ { n: n };
            }
        "#,
    );
    let code = format!(r#"
        package main

        use {impls_ns};
        use {shape_ns};

        trait Stamped {{
            fn raw(&self) -> i64;
            fn stamped_code(&self) -> i64 {{
                let s = shape.make_stamp(5555555);
                return shape.stamp_code(s);
            }}
        }}

        pub fn main() -> i32 {{
            let b = boxes.make_boxq(9);
            let x = b.stamped_code();
            return 0;
        }}
    "#);
    let result = compile(&code, false, None, true);
    cleanup(slug);
    assert!(result.is_ok(), "inherited default with qualified foreign refs failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(mlir.contains("BoxQ__stamped_code"),
            "inherited default must be monomorphized under BoxQ");
    assert!(mlir.contains("5555555"), "foreign-referencing default body must be emitted");
}

/// PROBE (currently expected to fail): a default body referencing the
/// TRAIT'S HOME-MODULE functions UNQUALIFIED. Inside the home module the
/// reference is local and legal; after inheritance into the implementor's
/// module it stops resolving ("Intrinsic 'pkg__shape__make_stamp' not found").
///
/// IGNORED because this is NOT a default-inheritance defect: an EXPLICIT
/// method in a module impl making the same unqualified foreign call fails
/// identically (verified manually). Fixing it means teaching cross-module
/// callee resolution to qualify foreign unqualified names for ALL bodies —
/// cross-cutting work owned by the expr resolver/seeker, out of scope for
/// trait-default inheritance. Revisit if that resolver learns foreign-name
/// qualification; then un-ignore and expect green.
#[test]
#[ignore = "pre-existing cross-module limitation: unqualified foreign fn refs don't resolve \
           even in EXPLICIT module-impl methods (same E003 without any default involved). \
           Fix belongs to cross-module callee resolution, not trait-default expansion."]
fn inherited_default_with_unqualified_home_module_refs() {
    let slug = "unqualified_home_refs";
    let _shape_ns = write_module(
        slug,
        "shape",
        r#"
            package target.cross_module_trait_defaults_it.unqualified_home_refs.shape

            pub struct Stamp { pub code: i64 }

            pub fn make_stamp(c: i64) -> Stamp {
                return Stamp { code: c };
            }

            trait StampedU {
                fn stamped(&self) -> Stamp {
                    return make_stamp(6666666);
                }
                fn raw(&self) -> i64;
            }
        "#,
    );
    let impls_ns = write_module(
        slug,
        "boxes",
        r#"
            package target.cross_module_trait_defaults_it.unqualified_home_refs.boxes

            use target.cross_module_trait_defaults_it.unqualified_home_refs.shape;

            struct BoxU { n: i64 }

            impl StampedU for BoxU {
                fn raw(&self) -> i64 {
                    return self.n;
                }
            }

            pub fn make_boxu(n: i64) -> BoxU {
                return BoxU { n: n };
            }
        "#,
    );
    let code = format!(r#"
        package main

        use {impls_ns};

        pub fn main() -> i32 {{
            let b = boxes.make_boxu(9);
            let s = b.stamped();
            return 0;
        }}
    "#);
    let result = compile(&code, false, None, true);
    cleanup(slug);
    assert!(result.is_ok(), "unqualified home-module ref in inherited default failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(mlir.contains("BoxU__stamped"), "inherited default must exist");
    assert!(mlir.contains("6666666"), "home-referencing default body must be emitted");
}
