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
//!
//! Defaults are keyed by NAMESPACE-QUALIFIED trait identity
//! (`<package path>.<trait name>`), never by the bare name alone: two
//! modules may legally define same-named traits, and bare-name keying
//! made their implementors cross-inherit whichever body was collected
//! last. Each impl block is matched against defaults through the eyes
//! of the FILE CONTAINING IT — local trait definitions first, then the
//! file's own imports, then a global fallback that preserves the
//! historical entry-file-wins collision policy.
//!
//! Tests: `codegen::tests_trait_defaults` (unit + loader level) and
//! `tests/trait_defaults_namespace_collision_test.rs` (end-to-end).

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::grammar::{ImportDecl, Item, SaltFile, SaltFn, SaltImpl};
use crate::registry::Registry;

use super::module_loader::ModuleLoader;

/// Namespace-qualified trait identity: `<package path>.<trait name>`.
type QualifiedId = String;

/// An override of a default that carried contract clauses. Carries the
/// clause Exprs plus the method's parameter names so stage-2 Z3
/// refinement can judge semantics rather than clause counts.
#[derive(Debug)]
pub struct OverrideObligation {
    pub trait_name: String,
    pub method: String,
    /// Receiver + argument names of the method signature, in order.
    pub params: Vec<String>,
    pub default_requires: Vec<syn::Expr>,
    pub ovr_requires: Vec<syn::Expr>,
    pub default_ensures: Vec<syn::Expr>,
    pub ovr_ensures: Vec<syn::Expr>,
}

/// Indexes default method tables for every loaded source.
///
/// `by_bare_name` mirrors `by_qualified` for files that refer to a trait
/// without a qualifying import; it exists only so the fallback resolver
/// can enumerate candidates deterministically.
#[derive(Default)]
struct DefaultTable {
    /// Qualified id -> default methods declared by that exact trait.
    by_qualified: HashMap<QualifiedId, Vec<SaltFn>>,
    /// Bare trait name -> qualified ids declaring it (modules in sorted
    /// collection order, entry file appended last).
    by_bare_name: BTreeMap<String, Vec<QualifiedId>>,
}

impl DefaultTable {
    /// Indexes one file's trait defaults under `ns`, the file's package
    /// path. Re-declaration inside one file keeps last-wins semantics,
    /// matching the previous single-table behavior.
    fn collect(&mut self, file: &SaltFile, ns: &str) {
        for item in &file.items {
            let Item::Trait(t) = item else { continue };
            if t.default_methods.is_empty() {
                continue;
            }
            let name = t.name.to_string();
            let qualified = qualified_id(ns, &name);
            if self.by_qualified.insert(qualified.clone(), t.default_methods.clone()).is_none() {
                self.by_bare_name.entry(name).or_default().push(qualified);
            }
        }
    }

    /// Resolves every known bare trait name from ONE file's perspective,
    /// yielding bare name -> the defaults its impl blocks inherit.
    fn resolution_for<'a>(
        &'a self,
        file: &SaltFile,
        ns: &str,
        entry_ns: &str,
    ) -> HashMap<String, &'a Vec<SaltFn>> {
        let mut resolved = HashMap::new();
        for bare in self.by_bare_name.keys() {
            if let Some(defaults) = self.resolve_one(file, ns, entry_ns, bare) {
                resolved.insert(bare.clone(), defaults);
            }
        }
        resolved
    }

    /// Binding precedence for one bare name: a trait defined in this very
    /// file, then a trait reachable through this file's own imports, then
    /// the global collision fallback.
    fn resolve_one<'a>(
        &'a self,
        file: &SaltFile,
        ns: &str,
        entry_ns: &str,
        bare: &str,
    ) -> Option<&'a Vec<SaltFn>> {
        if file_defines_trait(file, bare) {
            return self.by_qualified.get(&qualified_id(ns, bare));
        }
        if let Some(via_import) = self.resolve_via_imports(file, bare) {
            return Some(via_import);
        }
        self.resolve_globally(ns, entry_ns, bare)
    }

    /// Follows the file's own `use` declarations. Every prefix of an
    /// import path is a candidate namespace, longest first, so both
    /// `use p.m.Trait` (item import) and `use p.m` (module import) bind
    /// correctly and an item import outranks its parent package.
    fn resolve_via_imports<'a>(&'a self, file: &SaltFile, bare: &str) -> Option<&'a Vec<SaltFn>> {
        for imp in &file.imports {
            for candidate in import_candidate_ids(imp, bare) {
                if let Some(defaults) = self.by_qualified.get(&candidate) {
                    return Some(defaults);
                }
            }
        }
        None
    }

    /// Collision fallback for files that neither define nor import the
    /// trait, preserving the historical entry-file-wins policy: the entry
    /// file's definition beats every module's, a unique definition wins
    /// outright, and a genuine tie resolves to the lexicographically
    /// first module id so behavior never depends on map iteration order.
    fn resolve_globally(&self, ns: &str, entry_ns: &str, bare: &str) -> Option<&Vec<SaltFn>> {
        let candidates = self.by_bare_name.get(bare)?;
        let entry_id = qualified_id(entry_ns, bare);
        if ns != entry_ns && candidates.contains(&entry_id) {
            return self.by_qualified.get(&entry_id);
        }
        // The entry branch above already returned when applicable, so the
        // filtered candidate set cannot be empty here.
        let winner = candidates.iter()
            .filter(|id| *id != &entry_id || ns == entry_ns)
            .min()?;
        self.by_qualified.get(winner)
    }
}

/// Builds `<ns>.<trait>`; the dot cannot appear in identifiers, so the
/// mapping between qualified ids and (package, trait) pairs is injective.
fn qualified_id(ns: &str, trait_name: &str) -> String {
    format!("{}.{}", ns, trait_name)
}

/// The entry file's namespace identity: its `package` declaration, or
/// `"main"` when the file omits one (the conventional root package).
fn entry_namespace(file: &SaltFile) -> String {
    file.package.as_ref()
        .map(|p| p.name.iter().map(|id| id.to_string()).collect::<Vec<_>>().join("."))
        .unwrap_or_else(|| "main".to_string())
}

fn file_defines_trait(file: &SaltFile, bare: &str) -> bool {
    file.items.iter().any(|item| matches!(
        item,
        Item::Trait(t) if t.name == bare
    ))
}

/// Qualified ids an import makes visible for `bare`, most specific
/// first. A renamed import (`use p.m.Real as bare`) contributes the
/// real item's id; otherwise every path prefix is probed.
fn import_candidate_ids(imp: &ImportDecl, bare: &str) -> Vec<QualifiedId> {
    let segments: Vec<String> = imp.name.iter().map(|id| id.to_string()).collect();
    let mut ids = Vec::new();
    for end in (1..=segments.len()).rev() {
        ids.push(qualified_id(&segments[..end].join("."), bare));
    }
    if imp.alias.as_ref().is_some_and(|alias| alias == bare) {
        if let Some(real) = segments.last() {
            let parent = segments[..segments.len() - 1].join(".");
            ids.push(qualified_id(&parent, real));
        }
    }
    ids
}

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
) -> Vec<OverrideObligation> {
    // Sorted namespaces keep collection deterministic; the entry file's
    // defaults join the table last under its own package identity.
    let mut namespaces: Vec<String> = loader.loaded_files.keys().cloned().collect();
    namespaces.sort();
    let entry_ns = entry_namespace(file);

    let mut table = DefaultTable::default();
    for ns in &namespaces {
        if let Some(ast) = loader.loaded_files.get(ns) {
            table.collect(ast, ns);
        }
    }
    table.collect(file, &entry_ns);

    // Each file expands against ITS OWN resolution of trait names, so
    // same-named traits in different modules never cross-inherit.
    let mut obligations = Vec::new();
    for ns in &namespaces {
        if let Some(ast) = loader.loaded_files.get_mut(ns) {
            obligations.extend(expand_file(ast, &table.resolution_for(ast, ns, &entry_ns)));
        }
    }
    obligations.extend(expand_file(file, &table.resolution_for(file, &entry_ns, &entry_ns)));

    // Module impls were snapshotted into ModuleInfo::impls at load time,
    // before expansion; re-copy them so registry-driven registration sees
    // the completed method sets.
    loader.refresh_impl_snapshots(registry);
    obligations
}

/// Append omitted defaults to every trait impl in one file.
fn expand_file(
    file: &mut SaltFile,
    resolved: &HashMap<String, &Vec<SaltFn>>,
) -> Vec<OverrideObligation> {
    let mut obligations = Vec::new();
    for item in &mut file.items {
        if let Item::Impl(SaltImpl::Trait { trait_name, methods, .. }) = item {
            obligations.extend(inherit_into(trait_name, methods, resolved));
        }
    }
    obligations
}

/// Append clones of the trait's defaults that the impl does not provide,
/// matched by method name so overrides are never clobbered.
fn inherit_into(
    trait_name: &syn::Ident,
    methods: &mut Vec<SaltFn>,
    resolved: &HashMap<String, &Vec<SaltFn>>,
) -> Vec<OverrideObligation> {
    let Some(defaults) = resolved.get(&trait_name.to_string()) else { return Vec::new() };
    let provided: HashSet<String> = methods.iter().map(|m| m.name.to_string()).collect();
    let mut obligations = Vec::new();
    for default_fn in defaults.iter() {
        let override_fn = methods.iter().find(|m| m.name == default_fn.name);
        // Contract inheritance: any override of a default that carries
        // clauses becomes an obligation. Stage-2 Z3 refinement decides
        // whether the override strengthens or weakens each kind.
        if let Some(ovr) = override_fn {
            if !default_fn.requires.is_empty() || !default_fn.ensures.is_empty() {
                let params: Vec<String> = default_fn.args.iter()
                    .map(|a| a.name.to_string()).collect();
                obligations.push(OverrideObligation {
                    trait_name: trait_name.to_string(),
                    method: default_fn.name.to_string(),
                    params,
                    default_requires: default_fn.requires.clone(),
                    ovr_requires: ovr.requires.clone(),
                    default_ensures: default_fn.ensures.clone(),
                    ovr_ensures: ovr.ensures.clone(),
                });
            }
        }
        if !provided.contains(&default_fn.name.to_string()) {
            methods.push(default_fn.clone());
        }
    }
    obligations
}
