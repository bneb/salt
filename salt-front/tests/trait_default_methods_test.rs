// Regression tests: INHERITED TRAIT DEFAULT METHODS.
//
// grammar::SaltTrait parses default method bodies into `default_methods`,
// but they used to be inert: lowering dropped their bodies and codegen
// never materialized them for implementors. An impl that omitted such a
// method failed at codegen with "Method '<name>' not found for type".
//
// Locked here:
//   (a) an impl that OMITS a defaulted method inherits it — calls compile
//       and the default body is monomorphized under the implementing type;
//   (b) an explicit override wins — the default body is NOT emitted;
//   (c) mixed omit/override across two impls of ONE trait;
//   (d) alloc-style shape (required alloc/dealloc + defaulted realloc
//       composing them), mirroring std/core/alloc.salt inline without
//       importing std.
//   (e) CROSS-MODULE inheritance (two physical files): a module-file impl
//       omitting a default from another file's trait must reach
//       init_registry_impls with the inherited method present — registry
//       impl snapshots are re-synced after expansion.
use saltc::compile;
use saltc::grammar::SaltFile;

const SENTINEL_DEFAULT: &str = "1111111";
const SENTINEL_OVERRIDE: &str = "2222222";

/// (a) Omitted default: call must compile and use the trait default body.
#[test]
fn default_used_when_impl_omits_method() {
    let code = format!(r#"
        package main

        struct Counter {{ n: i64 }}

        trait Resettable {{
            fn reset(&self);
            fn describe(&self) -> i64 {{
                return {};
            }}
        }}

        impl Resettable for Counter {{
            fn reset(&self) {{ }}
        }}

        pub fn main() -> i32 {{
            let c = Counter {{ n: 7 }};
            let d = c.describe();
            return 0;
        }}
    "#, SENTINEL_DEFAULT);
    let result = compile(&code, false, None, true);
    assert!(result.is_ok(), "call to inherited default failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(mlir.contains("Counter__describe"),
            "inherited default must be emitted as Counter__describe");
    assert!(mlir.contains(SENTINEL_DEFAULT),
            "default body sentinel must appear in emitted MLIR");
}

/// (b) Explicit override: the impl body replaces the default entirely.
#[test]
fn override_wins_over_default() {
    let code = format!(r#"
        package main

        struct Gauge {{ v: i64 }}

        trait Describe {{
            fn describe(&self) -> i64 {{
                return {};
            }}
        }}

        impl Describe for Gauge {{
            fn describe(&self) -> i64 {{
                return {};
            }}
        }}

        pub fn main() -> i32 {{
            let g = Gauge {{ v: 1 }};
            let d = g.describe();
            return 0;
        }}
    "#, SENTINEL_DEFAULT, SENTINEL_OVERRIDE);
    let result = compile(&code, false, None, true);
    assert!(result.is_ok(), "override call failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(mlir.contains("Gauge__describe"),
            "overriding method must be emitted as Gauge__describe");
    assert!(mlir.contains(SENTINEL_OVERRIDE), "override body must be emitted");
    assert!(!mlir.contains(SENTINEL_DEFAULT),
            "default body must NOT be emitted when overridden");
}

/// (c) One trait, two impls: TypeA omits `score` (inherits default),
///     TypeB overrides it. Both calls compile; each type gets its own
///     monomorphized copy under its own mangled name.
#[test]
fn mixed_omit_and_override_across_two_impls() {
    let code = format!(r#"
        package main

        struct WidgetA {{ id: i64 }}
        struct WidgetB {{ id: i64 }}

        trait Scored {{
            fn score(&self) -> i64 {{
                return {};
            }}
        }}

        impl Scored for WidgetA {{
        }}

        impl Scored for WidgetB {{
            fn score(&self) -> i64 {{
                return {};
            }}
        }}

        pub fn main() -> i32 {{
            let a = WidgetA {{ id: 1 }};
            let b = WidgetB {{ id: 2 }};
            let s1 = a.score();
            let s2 = b.score();
            return 0;
        }}
    "#, SENTINEL_DEFAULT, SENTINEL_OVERRIDE);
    let result = compile(&code, false, None, true);
    assert!(result.is_ok(), "mixed omit/override failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(mlir.contains("WidgetA__score"), "inherited copy must exist for WidgetA");
    assert!(mlir.contains("WidgetB__score"), "override copy must exist for WidgetB");
    assert!(mlir.contains(SENTINEL_DEFAULT), "WidgetA must run the default body");
    assert!(mlir.contains(SENTINEL_OVERRIDE), "WidgetB must run the override body");
}

/// (d) Alloc-style shape (mirrors std/core/alloc.salt's Allocator, minimal
///     form, no std import): required `alloc`/`dealloc` plus a DEFAULTED
///     `realloc` whose body composes the two required methods through
///     `self.` dispatch on the implementing type.
#[test]
fn allocator_style_trait_inherits_realloc_default() {
    let code = r#"
        package main

        struct Pool { high_water: i64 }

        trait Allocator {
            fn alloc(&self, n: i64) -> i64;
            fn dealloc(&self, p: i64);
            fn realloc(&self, p: i64, n: i64) -> i64 {
                let fresh = self.alloc(n);
                self.dealloc(p);
                return fresh;
            }
        }

        impl Allocator for Pool {
            fn alloc(&self, n: i64) -> i64 {
                return n;
            }
            fn dealloc(&self, p: i64) { }
        }

        pub fn main() -> i32 {
            let pool = Pool { high_water: 1024 };
            let p = pool.alloc(64);
            let q = pool.realloc(p, 128);
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Allocator-style realloc call failed: {:?}", result.err());
    let mlir = result.unwrap();
    assert!(mlir.contains("Pool__realloc"),
            "defaulted realloc must be emitted as Pool__realloc");
    assert!(mlir.contains("Pool__alloc"),
            "required alloc must be emitted (called by default realloc)");
}

// ===========================================================================
// Cross-module inheritance: trait in one FILE, impl in another. The loader
// snapshots impls into the registry while loading; trait-default expansion
// rewrites the ASTs afterwards, so snapshots must be re-synced before
// init_registry_impls registers method tables.
// ===========================================================================

use std::fs;
use std::path::{Path, PathBuf};

const PROVIDER_TEMPLATE: &str = r#"
package {NS}.provider

pub trait Service {
    fn start(&self);
    fn ensure_started(&self) {
        self.start();
    }
}
"#;

const CONSUMER_TEMPLATE: &str = r#"
package {NS}.consumer

use {NS}.provider.Service;

pub struct Gadget { n: i64 }

impl Service for Gadget {
    fn start(&self) { }
}

pub fn demo() -> i64 {
    let g = Gadget { n: 5 };
    g.ensure_started();
    return g.n;
}
"#;

/// Writes the two fixture modules under a test-unique directory (tests run
/// in parallel, so each gets its own namespace root) and returns that root.
fn write_fixtures(unique: &str) -> PathBuf {
    let ns_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(unique)
        .join("fixtures");
    fs::create_dir_all(&ns_dir).expect("create fixture dir");
    let ns = format!("{}.fixtures", unique);
    fs::write(ns_dir.join("provider.salt"), PROVIDER_TEMPLATE.replace("{NS}", &ns))
        .expect("write provider");
    fs::write(ns_dir.join("consumer.salt"), CONSUMER_TEMPLATE.replace("{NS}", &ns))
        .expect("write consumer");
    Path::new(env!("CARGO_MANIFEST_DIR")).join(unique)
}

fn cleanup_fixtures(dir: &PathBuf) {
    let _ = fs::remove_dir_all(dir);
}

/// Registry-path check: after expansion + resync, the consumer module's
/// impl snapshot contains BOTH the provided method and the inherited
/// default — this is exactly what init_registry_impls walks.
#[test]
fn cross_module_impl_snapshot_carries_inherited_default() {
    let dir = write_fixtures("td_fixtures_snap");

    let roots = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR"))];
    let mut loader = saltc::codegen::module_loader::ModuleLoader::new(roots);
    let mut registry = saltc::registry::Registry::new();
    loader
        .load_module("td_fixtures_snap.fixtures.consumer", &mut registry)
        .expect("consumer module (and provider dep) must load");

    // Pre-expansion snapshot check: the load-time copy knows only the
    // provided method — this is the stale state init_registry_impls used
    // to see.
    let pre = registry.modules.get("td_fixtures_snap.fixtures.consumer").unwrap();
    let pre_methods: Vec<String> = pre
        .impls
        .iter()
        .flat_map(|(i, _)| match i {
            saltc::grammar::SaltImpl::Trait { methods, .. } => {
                methods.iter().map(|m| m.name.to_string()).collect::<Vec<_>>()
            }
            _ => vec![],
        })
        .collect();
    assert_eq!(pre_methods, vec!["start".to_string()], "load-time snapshot must be pre-inheritance");

    let mut entry: SaltFile = syn::parse_str("package main").unwrap();
    saltc::codegen::trait_defaults::expand_trait_defaults(&mut entry, &mut loader, &mut registry);
    cleanup_fixtures(&dir);

    let info = registry
        .modules
        .get("td_fixtures_snap.fixtures.consumer")
        .expect("consumer module registered");
    let mut trait_methods = None;
    for (impl_item, _) in &info.impls {
        if let saltc::grammar::SaltImpl::Trait { methods, .. } = impl_item {
            trait_methods = Some(methods.iter().map(|m| m.name.to_string()).collect::<Vec<_>>());
        }
    }
    let mut methods = trait_methods.expect("trait impl snapshot present");
    methods.sort();
    assert_eq!(
        methods,
        vec!["ensure_started".to_string(), "start".to_string()],
        "registry impl snapshot must include the inherited default"
    );
}

/// End-to-end: entry imports the consumer module's function and runs it;
/// the function body invokes the INHERITED default on its local type, so
/// the whole pipeline — load, expand, resync, init_registry_impls,
/// monomorphization seeding, emission — must succeed.
#[test]
fn cross_file_default_compiles_through_registry_init_path() {
    let dir = write_fixtures("td_fixtures_e2e");
    let code = r#"
        package main

        use td_fixtures_e2e.fixtures.consumer.demo;

        pub fn main() -> i32 {
            let outcome = demo();
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    cleanup_fixtures(&dir);
    assert!(
        result.is_ok(),
        "cross-file default inheritance failed: {:?}",
        result.err()
    );
    let mlir = result.unwrap();
    assert!(
        mlir.contains("Gadget__ensure_started"),
        "inherited default must be emitted as Gadget__ensure_started"
    );
}

// ===========================================================================
// Live std omissions locked as guards: real std modules omit Writer defaults
// and rely on cross-impl inheritance through the registry path.
//
// (Console deliberately has NO guard here: its type lives behind the
// `mod.salt` filename, so user-code imports cannot name it — `mod` is a
// reserved word and concrete cross-package struct lookup does not resolve.
// A pre-existing reachability gap, tracked separately from defaults.)
// ===========================================================================

/// String overrides write_bytes/write_i32 but OMITS write_str and write_bool;
/// their default bodies dispatch onto the provided set.
#[test]
fn std_string_inherits_write_str_and_write_bool_defaults() {
    let code = r#"
        package main

        use std.string.String;

        pub fn main() -> i32 {
            let mut s = String::new();
            s.write_bool(true);
            s.write_str("x", 1);
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(
        result.is_ok(),
        "String omitted-default calls failed: {:?}",
        result.err()
    );
}
