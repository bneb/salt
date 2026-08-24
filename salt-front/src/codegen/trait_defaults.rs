//! Trait default-method inheritance for codegen.
//!
//! `grammar::SaltTrait` keeps parsed default method bodies
//! (`default_methods`), and impl blocks may legally omit them. Every
//! downstream codegen stage — signature registration, monomorphization
//! seeding, method resolution, call-graph analysis — walks AST impl
//! blocks directly. Instead of teaching each stage about inheritance,
//! this pass rewrites each trait impl exactly once before registration:
//! omitted defaults are appended to the impl's method list so all
//! consumers see a complete, override-resolved method set. An explicit
//! impl method always wins over the default it overrides.

use std::collections::{HashMap, HashSet};

use crate::grammar::{Item, SaltFile, SaltFn, SaltImpl};
use crate::registry::Registry;

use super::module_loader::ModuleLoader;

/// Expand inherited trait defaults into the entry file and every loaded
/// module, then re-sync the registry's impl snapshots. Defaults are
/// collected first from all sources so a trait may be defined in a
/// different file than its implementors.
///
/// The registry copy matters as much as the ASTs: `load_module` snapshots
/// each module's impls into `ModuleInfo::impls` BEFORE this pass runs, so
/// without the re-sync `init_registry_impls` would register stale method
/// sets missing inherited defaults. Keeping both rewrites here gives
/// callers one choke point that cannot be half-applied.
pub fn expand_trait_defaults(
    file: &mut SaltFile,
    loader: &mut ModuleLoader,
    registry: &mut Registry,
) {
    // Sorted namespaces keep collection deterministic when identically
    // named traits exist in several modules; the entry file is consulted
    // last and therefore wins such (pathological) name collisions.
    let mut namespaces: Vec<String> = loader.loaded_files.keys().cloned().collect();
    namespaces.sort();

    let mut defaults: HashMap<String, Vec<SaltFn>> = HashMap::new();
    for ns in &namespaces {
        if let Some(ast) = loader.loaded_files.get(ns) {
            collect_file_defaults(ast, &mut defaults);
        }
    }
    collect_file_defaults(file, &mut defaults);

    for ns in &namespaces {
        if let Some(ast) = loader.loaded_files.get_mut(ns) {
            expand_file(ast, &defaults);
        }
    }
    expand_file(file, &defaults);

    // Module impls were snapshotted into ModuleInfo::impls at load time,
    // before expansion; re-copy them so registry-driven registration sees
    // the completed method sets.
    loader.refresh_impl_snapshots(registry);
}

/// Index trait name -> default methods declared in one file.
fn collect_file_defaults(file: &SaltFile, defaults: &mut HashMap<String, Vec<SaltFn>>) {
    for item in &file.items {
        if let Item::Trait(t) = item {
            if !t.default_methods.is_empty() {
                defaults.insert(t.name.to_string(), t.default_methods.clone());
            }
        }
    }
}

/// Append omitted defaults to every trait impl in one file.
fn expand_file(file: &mut SaltFile, defaults: &HashMap<String, Vec<SaltFn>>) {
    for item in &mut file.items {
        if let Item::Impl(SaltImpl::Trait { trait_name, methods, .. }) = item {
            inherit_into(trait_name, methods, defaults);
        }
    }
}

/// Append clones of the trait's defaults that the impl does not provide,
/// matched by method name so overrides are never clobbered.
fn inherit_into(
    trait_name: &syn::Ident,
    methods: &mut Vec<SaltFn>,
    defaults: &HashMap<String, Vec<SaltFn>>,
) {
    let Some(defaults) = defaults.get(&trait_name.to_string()) else { return };
    let provided: HashSet<String> = methods.iter().map(|m| m.name.to_string()).collect();
    for d in defaults {
        if !provided.contains(&d.name.to_string()) {
            methods.push(d.clone());
        }
    }
}

#[cfg(test)]
mod cross_module_tests {
    //! Loader-level lock on the registry re-sync: a module impl that omits a
    //! default must surface the inherited method in BOTH the loaded AST and
    //! the `ModuleInfo::impls` snapshot after ONE expansion call, since
    //! `init_registry_impls` registers from the snapshot while seeding and
    //! emission walk the AST.

    use super::*;
    use crate::registry::Registry;

    const MODULE_SRC: &str = r#"
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

    fn module_namespace() -> String {
        "target.trait_defaults_unit_it.snapshot.widgets".to_string()
    }

    /// Loads MODULE_SRC through the real ModuleLoader path (cwd root) and
    /// returns (loader, registry) before expansion.
    fn load_widget_module() -> (ModuleLoader, Registry) {
        let dir = std::env::current_dir()
            .expect("cwd")
            .join("target/trait_defaults_unit_it/snapshot");
        std::fs::create_dir_all(&dir).expect("create unit-test module dir");
        std::fs::write(dir.join("widgets.salt"), MODULE_SRC).expect("write module");
        let mut loader = ModuleLoader::new(vec![std::env::current_dir().expect("cwd")]);
        let mut registry = Registry::new();
        loader.load_module(&module_namespace(), &mut registry)
            .expect("module must load");
        (loader, registry)
    }

    /// Method names across every trait impl in a snapshot or AST slice.
    fn trait_impl_method_names(impl_items: &[crate::grammar::SaltImpl]) -> Vec<String> {
        let mut names = Vec::new();
        for imp in impl_items {
            if let crate::grammar::SaltImpl::Trait { methods, .. } = imp {
                for m in methods {
                    names.push(m.name.to_string());
                }
            }
        }
        names
    }

    fn snapshot_impls(info: &crate::registry::ModuleInfo) -> Vec<crate::grammar::SaltImpl> {
        info.impls.iter().map(|(i, _)| i.clone()).collect()
    }

    fn file_trait_impls(file: &SaltFile) -> Vec<crate::grammar::SaltImpl> {
        file.items.iter().filter_map(|i| match i {
            Item::Impl(imp @ crate::grammar::SaltImpl::Trait { .. }) => Some(imp.clone()),
            _ => None,
        }).collect()
    }

    #[test]
    fn expansion_reaches_loaded_ast_and_registry_snapshot() {
        let (mut loader, mut registry) = load_widget_module();
        let ns = module_namespace();

        // Pre-condition: the load-time snapshot is stale (no inherited method).
        let pre = trait_impl_method_names(&snapshot_impls(&registry.modules[&ns]));
        assert!(!pre.contains(&"describe".to_string()),
                "snapshot must start without the default, got {:?}", pre);

        let mut entry = syn::parse_str::<SaltFile>("package main\npub fn main() -> i32 { return 0; }").unwrap();
        expand_trait_defaults(&mut entry, &mut loader, &mut registry);

        let ast_methods = trait_impl_method_names(&file_trait_impls(&loader.loaded_files[&ns]));
        assert!(ast_methods.contains(&"describe".to_string()),
                "loaded AST must gain the default, got {:?}", ast_methods);

        let post = trait_impl_method_names(&snapshot_impls(&registry.modules[&ns]));
        assert!(post.contains(&"describe".to_string()),
                "registry snapshot must be resynced, got {:?}", post);
        // Required methods survive the rewrite in both copies.
        assert!(post.contains(&"tag".to_string()), "required method must survive");

        let dir = std::env::current_dir().expect("cwd").join("target/trait_defaults_unit_it");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> SaltFile {
        syn::parse_str::<SaltFile>(src).unwrap()
    }

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
