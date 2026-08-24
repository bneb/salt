//! Tests for `codegen::trait_defaults` — namespace-qualified inheritance.
//!
//! Layers, from narrowest to widest:
//! - `single_file`: expansion inside one file (override wins, untouched
//!   impls, trait-less safety).
//! - `cross_module`: the registry re-sync lock — an omitted default must
//!   reach BOTH the loaded module AST and the `ModuleInfo::impls`
//!   snapshot after ONE expansion call, because `init_registry_impls`
//!   registers from the snapshot while seeding and emission walk ASTs.
//! - `collision`: THE REGRESSION LOCK. Two modules declaring same-named
//!   traits (`Config`) must never cross-inherit: keying defaults by bare
//!   name let the lexicographically last module's body leak into every
//!   other module's implementors.
//! - `entry_wins`: the preserved fallback policy — an impl that neither
//!   defines nor imports its trait keeps binding to the ENTRY file's
//!   definition when one exists.
//! - `import_binding`: item-level `use p.m.Trait` binds an impl to the
//!   imported trait through real ModuleLoader loading.
//!
//! Loader-level fixtures are written under gitignored `target/`, which is
//! a compiler search root via cwd (cargo test runs with cwd = manifest
//! dir), exactly like production module loading.

use crate::codegen::module_loader::ModuleLoader;
use crate::codegen::trait_defaults::{expand_trait_defaults, OverrideObligation};
use crate::grammar::{Item, SaltFile, SaltImpl};
use crate::registry::Registry;

const UNIT_ROOT: &str = "target/trait_defaults_unit_it";

/// Writes one module file reachable as namespace
/// `<UNIT_ROOT dotted>.<root_suffix>.<slug>` and returns that namespace.
fn write_module(root_suffix: &str, slug: &str, src: &str) -> String {
    let dir = std::env::current_dir()
        .expect("cwd available")
        .join(UNIT_ROOT)
        .join(root_suffix);
    std::fs::create_dir_all(&dir).expect("create module dir");
    std::fs::write(dir.join(format!("{}.salt", slug)), src).expect("write module");
    format!("{}.{}.{}", UNIT_ROOT.replace('/', "."), root_suffix, slug)
}

fn remove_test_root(root_suffix: &str) {
    let dir = std::env::current_dir().expect("cwd").join(UNIT_ROOT).join(root_suffix);
    let _ = std::fs::remove_dir_all(dir);
}

fn parse(src: &str) -> SaltFile {
    syn::parse_str::<SaltFile>(src).unwrap()
}

fn empty_main() -> SaltFile {
    parse("package main\npub fn main() -> i32 { return 0; }")
}

/// Loads modules by namespace through the real ModuleLoader path.
fn load_namespaces(namespaces: &[String]) -> (ModuleLoader, Registry) {
    let mut loader = ModuleLoader::new(vec![std::env::current_dir().expect("cwd")]);
    let mut registry = Registry::new();
    for ns in namespaces {
        loader.load_module(ns, &mut registry).expect("module must load");
    }
    (loader, registry)
}

/// Debug-formats every inherited body of `method_name` across one file's
/// trait impls; sentinel digits identify which module's default was
/// inherited.
fn inherited_bodies(file: &SaltFile, method_name: &str) -> Vec<String> {
    let mut bodies = Vec::new();
    for item in &file.items {
        if let Item::Impl(SaltImpl::Trait { methods, .. }) = item {
            for m in methods {
                if m.name == method_name {
                    bodies.push(format!("{:?}", m.body));
                }
            }
        }
    }
    bodies
}

fn assert_single_body(bodies: &[String], present: &str, absent: &str) {
    assert_eq!(bodies.len(), 1, "exactly one value body expected");
    assert!(bodies[0].contains(present), "expected {} in {:?}", present, bodies[0]);
    if !absent.is_empty() {
        assert!(!bodies[0].contains(absent), "must not contain {}", absent);
    }
}

#[cfg(test)]
mod single_file {
    use super::*;

    const TRAIT_AND_IMPLS: &str = r#"
        package main

        struct Counter { n: i64 }
        struct Step { n: i64 }

        trait Resettable {
            fn reset(&self);
            fn reset_to_zero(&self) {
                self.reset();
            }
        }

        impl Resettable for Counter {
            fn reset(&self) { }
        }

        impl Resettable for Step {
            fn reset(&self) { }
            fn reset_to_zero(&self) { }
        }
    "#;

    #[test]
    fn omitted_default_is_injected_into_impl_methods() {
        let mut file = parse(TRAIT_AND_IMPLS);
        expand_trait_defaults(&mut file, &mut ModuleLoader::new(vec![]), &mut Registry::new());
        let counter = file.items.iter().find_map(|i| match i {
            Item::Impl(SaltImpl::Trait { target_ty, methods, .. }) => {
                let name = format!("{:?}", target_ty);
                name.contains("Counter").then_some(methods.len())
            }
            _ => None,
        });
        assert_eq!(counter, Some(2), "omitted default must be appended");
    }

    #[test]
    fn explicit_override_wins_over_default() {
        let mut file = parse(TRAIT_AND_IMPLS);
        expand_trait_defaults(&mut file, &mut ModuleLoader::new(vec![]), &mut Registry::new());
        let step = file.items.iter().find_map(|i| match i {
            Item::Impl(SaltImpl::Trait { target_ty, methods, .. }) => {
                let name = format!("{:?}", target_ty);
                name.contains("Step").then_some(methods.clone())
            }
            _ => None,
        });
        let methods = step.expect("Step impl must exist");
        assert_eq!(methods.len(), 2, "override must not duplicate the default");
        let reset_to_zero = methods.iter().find(|m| m.name == "reset_to_zero").unwrap();
        assert_eq!(reset_to_zero.body.stmts.len(), 0, "impl body must win over default body");
    }

    #[test]
    fn impl_without_matching_trait_is_untouched() {
        let src = r#"
            package main
            struct Cat { }
            trait Pet { fn speak(&self); }
            trait Feed { fn feed(&self); }
            impl Pet for Cat { fn speak(&self) { } }
        "#;
        let mut file = parse(src);
        expand_trait_defaults(&mut file, &mut ModuleLoader::new(vec![]), &mut Registry::new());
        let count = file.items.iter().filter_map(|i| match i {
            Item::Impl(SaltImpl::Trait { methods, .. }) => Some(methods.len()),
            _ => None,
        }).next();
        assert_eq!(count, Some(1));
    }

    #[test]
    fn defaults_from_trait_without_impls_are_ignored_safely() {
        let src = r#"
            package main
            trait Lone { fn helper(&self) -> i64 { return 7; } }
        "#;
        let mut file = parse(src);
        expand_trait_defaults(&mut file, &mut ModuleLoader::new(vec![]), &mut Registry::new());
        assert_eq!(file.items.len(), 1, "no impl present, nothing rewritten");
    }
}

#[cfg(test)]
mod cross_module {
    use super::*;

    const WIDGETS_SRC: &str = r#"
        package target.trait_defaults_unit_it.snapshot.widgets

        struct Widget { id: i64 }

        trait Describe {
            fn tag(&self) -> i64;
            fn describe(&self) -> i64 {
                return 7777777;
            }
        }

        impl Describe for Widget {
            fn tag(&self) -> i64 {
                return self.id;
            }
        }
    "#;

    /// Method names across every trait impl in a snapshot or AST slice.
    fn trait_impl_method_names(impl_items: &[SaltImpl]) -> Vec<String> {
        impl_items.iter().filter_map(|imp| match imp {
            SaltImpl::Trait { methods, .. } => {
                Some(methods.iter().map(|m| m.name.to_string()).collect::<Vec<_>>())
            }
            _ => None,
        }).flatten().collect()
    }

    fn snapshot_impls(info: &crate::registry::ModuleInfo) -> Vec<SaltImpl> {
        info.impls.iter().map(|(i, _)| i.clone()).collect()
    }

    fn file_trait_impls(file: &SaltFile) -> Vec<SaltImpl> {
        file.items.iter().filter_map(|i| match i {
            Item::Impl(imp @ SaltImpl::Trait { .. }) => Some(imp.clone()),
            _ => None,
        }).collect()
    }

    #[test]
    fn expansion_reaches_loaded_ast_and_registry_snapshot() {
        let ns = write_module("snapshot", "widgets", WIDGETS_SRC);
        let (mut loader, mut registry) = load_namespaces(std::slice::from_ref(&ns));

        // Pre-condition: the load-time snapshot is stale (no inherited method).
        let pre = trait_impl_method_names(&snapshot_impls(&registry.modules[&ns]));
        assert!(!pre.contains(&"describe".to_string()),
                "snapshot must start without the default, got {:?}", pre);

        let mut entry = empty_main();
        expand_trait_defaults(&mut entry, &mut loader, &mut registry);

        let ast_methods = trait_impl_method_names(&file_trait_impls(&loader.loaded_files[&ns]));
        assert!(ast_methods.contains(&"describe".to_string()),
                "loaded AST must gain the default, got {:?}", ast_methods);

        let post = trait_impl_method_names(&snapshot_impls(&registry.modules[&ns]));
        assert!(post.contains(&"describe".to_string()),
                "registry snapshot must be resynced, got {:?}", post);
        // Required methods survive the rewrite in both copies.
        assert!(post.contains(&"tag".to_string()), "required method must survive");

        remove_test_root("snapshot");
    }
}

#[cfg(test)]
mod collision {
    //! Same-named traits in two modules: each impl inherits ITS OWN
    //! module's default body. Before qualified keying, beta (sorted
    //! after alpha) clobbered alpha's table entry and BOTH inherited
    //! beta's constant 222222.

    use super::*;

    const ALPHA_SRC: &str = r#"
        package target.trait_defaults_unit_it.xname.alpha

        struct AlphaCfg { v: i64 }

        trait Config {
            fn kind(&self) -> i64;
            fn value(&self) -> i64 {
                return 111111;
            }
        }

        impl Config for AlphaCfg {
            fn kind(&self) -> i64 { return 1; }
        }
    "#;

    const BETA_SRC: &str = r#"
        package target.trait_defaults_unit_it.xname.beta

        struct BetaCfg { v: i64 }

        trait Config {
            fn kind(&self) -> i64;
            fn value(&self) -> i64 {
                return 222222;
            }
        }

        impl Config for BetaCfg {
            fn kind(&self) -> i64 { return 2; }
        }
    "#;

    #[test]
    fn same_named_traits_keep_their_own_defaults() {
        let alpha_ns = write_module("xname", "alpha", ALPHA_SRC);
        let beta_ns = write_module("xname", "beta", BETA_SRC);
        let both = [alpha_ns.clone(), beta_ns.clone()];
        let (mut loader, mut registry) = load_namespaces(&both);

        let mut entry = empty_main();
        expand_trait_defaults(&mut entry, &mut loader, &mut registry);

        let alpha = &loader.loaded_files[&alpha_ns];
        let beta = &loader.loaded_files[&beta_ns];
        assert_single_body(&inherited_bodies(alpha, "value"), "111111", "222222");
        assert_single_body(&inherited_bodies(beta, "value"), "222222", "111111");

        remove_test_root("xname");
    }

    #[test]
    fn entry_file_definition_wins_unresolved_fallback() {
        // Gamma implements `Config` without defining or importing it;
        // the entry file declares its OWN `Config` with a distinct
        // default, and the historical entry-file-wins policy keeps gamma
        // bound to the entry definition.
        let gamma_src = r#"
            package target.trait_defaults_unit_it.ewins.gamma

            struct GammaCfg { v: i64 }

            impl Config for GammaCfg {
                fn kind(&self) -> i64 { return 3; }
            }
        "#;
        let gamma_ns = write_module("ewins", "gamma", gamma_src);
        let (mut loader, mut registry) = load_namespaces(std::slice::from_ref(&gamma_ns));

        let mut entry = parse(r#"
            package main

            trait Config {
                fn kind(&self) -> i64;
                fn value(&self) -> i64 {
                    return 333333;
                }
            }

            pub fn main() -> i32 { return 0; }
        "#);
        expand_trait_defaults(&mut entry, &mut loader, &mut registry);

        let gamma = &loader.loaded_files[&gamma_ns];
        assert_single_body(&inherited_bodies(gamma, "value"), "333333", "");

        remove_test_root("ewins");
    }
}

#[cfg(test)]
mod import_binding {
    //! An impl block importing its trait by item path
    //! (`use p.m.Trait`) must inherit THAT trait's defaults through the
    //! real loader pipeline.

    use super::*;

    const GREET_SRC: &str = r#"
        package target.trait_defaults_unit_it.ibind.greet

        struct Pt { x: i64 }

        trait Greet {
            fn hi(&self) -> i64;
            fn hello(&self) -> i64 {
                return 8888888;
            }
        }
    "#;

    const IMPLS_SRC: &str = r#"
        package target.trait_defaults_unit_it.ibind.impls

        use target.trait_defaults_unit_it.ibind.greet.Greet;

        struct Dog { n: i64 }

        impl Greet for Dog {
            fn hi(&self) -> i64 { return self.n; }
        }
    "#;

    #[test]
    fn item_import_binds_impl_to_imported_trait() {
        let greet_ns = write_module("ibind", "greet", GREET_SRC);
        let impls_ns = write_module("ibind", "impls", IMPLS_SRC);
        // Loading the importer pulls in the trait module via its imports.
        let (mut loader, mut registry) = load_namespaces(std::slice::from_ref(&impls_ns));
        assert!(loader.loaded_files.contains_key(&greet_ns), "trait module must be pulled in");

        let mut entry = empty_main();
        expand_trait_defaults(&mut entry, &mut loader, &mut registry);

        let impls_ast = &loader.loaded_files[&impls_ns];
        let bodies = inherited_bodies(impls_ast, "hello");
        assert_eq!(bodies.len(), 1, "Dog must gain exactly one value body");
        assert!(bodies[0].contains("8888888"), "imported default body expected, got {:?}", bodies[0]);

        // The registry snapshot sees the inherited method too.
        let info = &registry.modules[&impls_ns];
        let names: Vec<String> = snapshot_method_names(info);
        assert!(names.contains(&"hello".to_string()), "snapshot missing hello, got {:?}", names);

        remove_test_root("ibind");
    }

    fn snapshot_method_names(info: &crate::registry::ModuleInfo) -> Vec<String> {
        let mut names = Vec::new();
        for (imp, _) in &info.impls {
            if let SaltImpl::Trait { methods, .. } = imp {
                names.extend(methods.iter().map(|m| m.name.to_string()));
            }
        }
        names
    }
}

#[cfg(test)]
mod contract_inheritance {
    //! Stage-1 contract inheritance: an override dropping requires or
    //! ensures clauses its default carried yields an OverrideObligation
    //! (drained as a hard [E009] by emit_mlir).

    use super::*;

    fn expand(src: &str) -> Vec<OverrideObligation> {
        let mut file = parse(src);
        expand_trait_defaults(&mut file, &mut ModuleLoader::new(vec![]), &mut Registry::new())
    }

    #[test]
    fn override_dropping_requires_yields_obligation() {
        let obligations = expand(r#"
            package main

            trait Guarded {
                fn wrapped(&self, k: i64) -> i64 requires(k > 0) { return k; }
            }

            struct Thing { v: i64 }

            impl Guarded for Thing {
                fn wrapped(&self, k: i64) -> i64 { return k; }
            }
        "#);
        assert_eq!(obligations.len(), 1, "dropped requires must obligate");
        assert_eq!(obligations[0].trait_name, "Guarded");
        assert_eq!(obligations[0].method, "wrapped");
        assert_eq!(obligations[0].dropped_ensures, 0);
    }

    #[test]
    fn conformant_override_yields_no_obligation() {
        let obligations = expand(r#"
            package main

            trait Guarded {
                fn wrapped(&self, k: i64) -> i64 requires(k > 0) { return k; }
            }

            struct Thing { v: i64 }

            impl Guarded for Thing {
                fn wrapped(&self, k: i64) -> i64 requires(k > 0) { return k * 2; }
            }
        "#);
        assert!(obligations.is_empty(), "strengthening override is legal");
    }
}
