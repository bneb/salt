//! Trait Registry - Signature-Aware Method Resolution for Salt
//!
//! This module solves the "method overload collision" problem where the legacy
//! method_registry keyed by `(TypeKey, String)` could only store one method
//! per (type, name) pair, ignoring parameter signatures.
//!
//! ## The Solution: MethodKey with Parameter Signature Hash
//!
//! The key is extended to include a hash of the parameter types, enabling:
//! - `append_formatted(&mut self, val: i64)` → key: (Handler, "append_formatted", hash([i64]))
//! - `append_formatted(&mut self, fmt: FormattedHex)` → key: (Handler, "append_formatted", hash([FormattedHex]))
//!
//! ## Design Principles
//!
//! 1. **Backward Compatible**: Legacy lookups still work via fallback
//! 2. **Monomorphization-Aware**: Works with the existing hydration/collector system
//! 3. **Minimal Diff**: Extends rather than replaces existing infrastructure
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use crate::types::{Type, TypeKey};
use crate::grammar::{SaltFn, ImportDecl};

/// A method key that includes parameter signature for overload resolution.
/// This enables multiple methods with the same name but different parameter types.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MethodKey {
    /// The type implementing the method (e.g., InterpolatedStringHandler)
    pub receiver_type: TypeKey,
    /// The method name (e.g., "append_formatted")
    pub method_name: String,
    /// Hash of parameter types for overload disambiguation
    /// A hash rather than the full signature is used for efficiency
    pub param_signature_hash: u64,
}
impl MethodKey {
    /// Create a new method key from receiver type, method name, and parameter types.
    pub fn new(receiver: TypeKey, name: String, param_types: &[Type]) -> Self {
        Self {
            receiver_type: receiver,
            method_name: name,
            param_signature_hash: Self::hash_params(param_types),
        }
    }
    
    /// Create a legacy-compatible key (for lookups without knowing parameter types).
    /// This sets param_signature_hash to 0, which is used for fallback resolution.
    pub fn legacy(receiver: TypeKey, name: String) -> Self {
        Self {
            receiver_type: receiver,
            method_name: name,
            param_signature_hash: 0,
        }
    }
    
    /// Hash parameter types for signature disambiguation.
    /// This produces a stable hash based on type structure.
    fn hash_params(params: &[Type]) -> u64 {
        if params.is_empty() {
            return 0;
        }
        let mut hasher = DefaultHasher::new();
        for param in params {
            // Hash the debug representation for stability
            // In production, we'd use a proper canonical type hash
            format!("{:?}", param).hash(&mut hasher);
        }
        hasher.finish()
    }
    
    /// Generate a mangled symbol name for this method.
    /// Format: `{receiver_mangled}__{method_name}[__{sig_suffix}]`
    pub fn mangle(&self) -> String {
        let base = format!("{}_{}", self.receiver_type.mangle(), self.method_name);
        if self.param_signature_hash == 0 {
            base
        } else {
            // Include signature hash suffix for overloaded methods
            format!("{}_{:x}", base, self.param_signature_hash & 0xFFFF)
        }
    }
}

/// A resolved method with its associated metadata.
#[derive(Debug, Clone)]
pub struct ResolvedMethod {
    /// The function AST node
    pub func: SaltFn,
    /// The Self type for this impl (e.g., InterpolatedStringHandler)
    pub self_ty: Option<Type>,
    /// Imports in scope for this method
    pub imports: Vec<ImportDecl>,
}

/// Trait definition parsed from Salt source.
/// This represents a `trait Foo { fn bar(...); }` declaration.
#[derive(Debug, Clone)]
pub struct TraitDef {
    /// Trait name (e.g., "Formattable")
    pub name: String,
    /// Generic type parameters (e.g., ["T"] for trait Foo<T>)
    pub generic_params: Vec<String>,
    /// Method signatures required by this trait
    pub method_signatures: Vec<TraitMethodSig>,
}
/// A method signature within a trait definition.
#[derive(Debug, Clone)]
pub struct TraitMethodSig {
    /// Method name
    pub name: String,
    /// Parameter types (excluding self)
    pub param_types: Vec<Type>,
    /// Return type
    pub return_type: Type,
    /// Whether the method takes &self, &mut self, or self
    pub receiver_kind: ReceiverKind,
}

/// How a method receives its self parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverKind {
    /// No self parameter (associated function)
    None,
    /// `&self` - immutable borrow
    Ref,
    /// `&mut self` - mutable borrow
    RefMut,
    /// `self` - takes ownership
    Owned,
}

/// Trait implementation for a concrete type.
#[derive(Debug, Clone)]
pub struct TraitImpl {
    /// Which trait is being implemented
    pub trait_name: String,
    /// The concrete type implementing the trait
    pub impl_type: TypeKey,
    /// Mapping from method name to implementation
    pub method_impls: HashMap<String, SaltFn>,
}

/// The unified registry for trait-aware method resolution.
/// 
/// This provides:
/// 1. **Trait Storage**: `traits` maps trait names to definitions
/// 2. **Impl Storage**: `impls` maps (trait, type) to implementations
/// 3. **Method Index**: `method_index` allows direct lookup by signature
/// 4. **Legacy Fallback**: `legacy_index` for backward compatibility
pub struct TraitRegistry {
    /// Trait definitions: trait_name -> TraitDef
    traits: HashMap<String, TraitDef>,
    /// Trait implementations: (trait_name, impl_type) -> TraitImpl
    impls: HashMap<(String, TypeKey), TraitImpl>,
    /// Primary method index with full signature awareness
    method_index: HashMap<MethodKey, ResolvedMethod>,
    /// Legacy fallback index: (TypeKey, method_name) -> Vec<MethodKey>
    /// Maps to all overloads for backward-compatible lookup
    legacy_index: HashMap<(TypeKey, String), Vec<MethodKey>>,
}

impl Default for TraitRegistry {
    fn default() -> Self {
        Self::new()
    }
}
impl TraitRegistry {
    /// Create a new empty trait registry.
    pub fn new() -> Self {
        Self {
            traits: HashMap::new(),
            impls: HashMap::new(),
            method_index: HashMap::new(),
            legacy_index: HashMap::new(),
        }
    }
    
    /// Register a trait definition.
    pub fn register_trait(&mut self, def: TraitDef) {
        self.traits.insert(def.name.clone(), def);
    }
    
    /// Register a trait definition from parsed grammar.
    pub fn register_trait_def(
        &mut self, 
        name: String, 
        generics: Option<crate::grammar::Generics>,
        method_names: Vec<String>,
    ) {
        let generic_params = generics.map_or(vec![], |g| {
            g.params.iter().filter_map(|p| {
                if let crate::grammar::GenericParam::Type { name, .. } = p {
                    Some(name.to_string())
                } else {
                    None
                }
            }).collect()
        });
        
        // For now, we store just the method names - full signature resolution happens later
        let method_signatures = method_names.into_iter().map(|name| {
            TraitMethodSig {
                name,
                param_types: vec![],  // Will be resolved on impl
                return_type: Type::Unit,  // Placeholder
                receiver_kind: ReceiverKind::Ref,  // Default
            }
        }).collect();
        
        self.register_trait(TraitDef { name, generic_params, method_signatures });
    }
    
    /// Get a trait definition by name.
    pub fn get_trait(&self, name: &str) -> Option<&TraitDef> {
        self.traits.get(name)
    }
    
    /// Register a trait implementation for a concrete type.
    pub fn register_impl(&mut self, impl_: TraitImpl) {
        self.impls.insert((impl_.trait_name.clone(), impl_.impl_type.clone()), impl_);
    }
    
    /// Register a method with full signature awareness.
    /// This is the primary registration path for Phase 2.
    pub fn register_method(&mut self, key: MethodKey, method: ResolvedMethod) {
        // Add to legacy index for fallback
        self.legacy_index
            .entry((key.receiver_type.clone(), key.method_name.clone()))
            .or_default()
            .push(key.clone());
        
        // Add to primary index
        self.method_index.insert(key, method);
    }
    
    /// Look up a method by its full key (receiver + name + param signature).
    /// This is the precise lookup used when parameter types are known.
    pub fn get_method(&self, key: &MethodKey) -> Option<&ResolvedMethod> {
        self.method_index.get(key)
    }
    
    /// Look up a method by receiver and name only (legacy compatibility).
    /// Returns the first matching method, or None if no matches.
    /// 
    /// For overloaded methods, use `get_method_overloads` to get all candidates.
    pub fn get_method_legacy(&self, receiver: &TypeKey, name: &str) -> Option<&ResolvedMethod> {
        // Try exact key lookup first
        if let Some(method) = self.legacy_index
            .get(&(receiver.clone(), name.to_string()))
            .and_then(|keys| keys.first())
            .and_then(|key| self.method_index.get(key))
        {
            return Some(method);
        }
        
        // Path-agnostic fallback: when lookup key has empty path but
        // registration used full path (e.g., trait method flattening), match by name only
        if receiver.path.is_empty() {
            for ((reg_key, method_name), method_keys) in &self.legacy_index {
                // Match if: method name matches AND receiver name matches (ignoring path)
                if method_name == name && reg_key.name == receiver.name {
                    if let Some(key) = method_keys.first() {
                        if let Some(method) = self.method_index.get(key) {
                            return Some(method);
                        }
                    }
                }
            }
        }
        
        None
    }
    
    /// Get all overloads for a method name on a given receiver type.
    /// Returns method keys that can be used for further resolution.
    pub fn get_method_overloads(&self, receiver: &TypeKey, name: &str) -> Vec<&MethodKey> {
        self.legacy_index
            .get(&(receiver.clone(), name.to_string()))
            .map(|keys| keys.iter().collect())
            .unwrap_or_default()
    }
    
    /// Resolve the best matching method for a call based on argument types.
    /// This implements overload resolution by finding the method whose
    /// parameter signature matches the provided argument types.
    pub fn resolve_overload(
        &self,
        receiver: &TypeKey,
        name: &str,
        arg_types: &[Type],
    ) -> Option<&ResolvedMethod> {
        // Compute the expected signature hash
        let expected_hash = MethodKey::hash_params(arg_types);
        
        // Look for exact match first
        let key = MethodKey {
            receiver_type: receiver.clone(),
            method_name: name.to_string(),
            param_signature_hash: expected_hash,
        };
        
        if let Some(method) = self.method_index.get(&key) {
            return Some(method);
        }
        
        // Fall back to legacy single-method lookup
        self.get_method_legacy(receiver, name)
    }
    
    /// Check if a type implements a specific trait.
    pub fn implements_trait(&self, ty: &TypeKey, trait_name: &str) -> bool {
        self.impls.contains_key(&(trait_name.to_string(), ty.clone()))
    }
    
    /// Get the trait implementation for a type, if it exists.
    pub fn get_trait_impl(&self, ty: &TypeKey, trait_name: &str) -> Option<&TraitImpl> {
        self.impls.get(&(trait_name.to_string(), ty.clone()))
    }
    
    /// Get the number of registered methods.
    pub fn method_count(&self) -> usize {
        self.method_index.len()
    }
    
    /// Get the number of registered traits.
    pub fn trait_count(&self) -> usize {
        self.traits.len()
    }
    
    // =========================================================================
    // Convenience methods for migration from legacy method_registry
    // =========================================================================
    
    /// Register a method using simple parameters (convenience wrapper).
    /// Extracts parameter types from the function definition.
    pub fn register_simple(
        &mut self,
        receiver: TypeKey,
        func: SaltFn,
        self_ty: Option<Type>,
        imports: Vec<ImportDecl>,
    ) {
        // Extract parameter types from function signature (skip 'self' parameter)
        let param_types: Vec<Type> = func.args.iter()
            .filter(|arg| arg.name != "self")
            .filter_map(|arg| arg.ty.as_ref().and_then(Type::from_syn))
            .collect();
        
        let key = MethodKey::new(receiver, func.name.to_string(), &param_types);
        let method = ResolvedMethod {
            func,
            self_ty,
            imports,
        };
        self.register_method(key, method);
    }
    
    /// Find all method names registered for a given type.
    /// Used by context.find_methods_for_template.
    pub fn find_methods_for_type(&self, template_name: &str) -> Vec<String> {
        let mut methods = Vec::new();
        for key in self.method_index.keys() {
            if key.receiver_type.name == template_name || key.receiver_type.mangle() == template_name {
                methods.push(key.method_name.clone());
            }
        }
        methods.sort();
        methods.dedup();
        methods
    }
    
    /// Check if a method exists for a given receiver type and name.
    pub fn contains_method(&self, receiver: &TypeKey, name: &str) -> bool {
        self.legacy_index.contains_key(&(receiver.clone(), name.to_string()))
    }
    
    /// Iterate over all method entries (for migration compatibility).
    pub fn iter_methods(&self) -> impl Iterator<Item = (&MethodKey, &ResolvedMethod)> {
        self.method_index.iter()
    }
    
    /// Iterate over all unique type keys (receivers) in the registry.
    /// Used for fallback matching when exact key lookup fails.
    pub fn iter_type_keys(&self) -> impl Iterator<Item = TypeKey> + '_ {
        self.legacy_index.keys().map(|(tk, _)| tk.clone())
    }
    
    /// Look up a method by (receiver, name) and return in legacy format.
    /// Returns (SaltFn, Option<Type>, Vec<ImportDecl>) for compatibility.
    pub fn get_legacy(&self, receiver: &TypeKey, name: &str) -> Option<(SaltFn, Option<Type>, Vec<ImportDecl>)> {
        self.get_method_legacy(receiver, name)
            .map(|m| (m.func.clone(), m.self_ty.clone(), m.imports.clone()))
    }
    
    /// Find a method by matching receiver type name/mangle and method name.
    /// Used for hydration where the exact TypeKey is unavailable.
    ///
    /// Qualified identity first: when `type_name` is a fully-qualified
    /// receiver mangle (`pkg__mod__Type`), only receivers whose mangled
    /// name equals it qualify, so two same-named types in different
    /// modules never cross-resolve. Bare-name matching remains as the
    /// fallback for callers that genuinely lack path context.
    pub fn find_method_by_name(
        &self,
        type_name: &str,
        method_name: &str,
        self_ty: &Type,
    ) -> Option<(SaltFn, Option<Type>, Vec<ImportDecl>)> {
        self.pick_method_by_identity(type_name, method_name)
            .map(|method| (method.func.clone(), Some(self_ty.clone()), method.imports.clone()))
    }

    /// Qualified identity resolution over receiver + method name.
    ///
    /// Every candidate matching either identity arm is collected and ranked
    /// so the choice stays deterministic where several modules register
    /// same-named types: an exact fully-qualified receiver match (mangled
    /// pkg__mod__Type) always outranks a bare-name match, and equal ranks
    /// are broken by receiver mangle. Callers without any path context
    /// keep working through the bare-name rank.
    fn pick_method_by_identity(&self, type_name: &str, method_name: &str) -> Option<&ResolvedMethod> {
        let mut ranked: Vec<(u8, String, &ResolvedMethod)> = self.method_index.iter()
            .filter_map(|(key, method)| {
                if key.method_name != method_name { return None; }
                if key.receiver_type.mangle() == type_name {
                    Some((0u8, key.receiver_type.mangle(), method))
                } else if key.receiver_type.name == type_name {
                    Some((1u8, key.receiver_type.mangle(), method))
                } else {
                    None
                }
            })
            .collect();
        ranked.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        ranked.first().map(|(_, _, method)| *method)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn mock_type_key(name: &str) -> TypeKey {
        TypeKey {
            path: vec!["std".to_string(), "string".to_string()],
            name: name.to_string(),
            specialization: None,
        }
    }
    
    fn mock_salt_fn(name: &str) -> SaltFn {
        // Create a minimal SaltFn for testing
        SaltFn {
            name: syn::Ident::new(name, proc_macro2::Span::call_site()),
            generics: None,
            args: syn::punctuated::Punctuated::new(),
            ret_type: None,
            body: crate::grammar::SaltBlock { stmts: vec![] },
            requires: vec![],
            ensures: vec![],
            attributes: vec![],
            is_pub: false,
        }
    }
    
    #[test]
    fn test_method_key_hash_different_params() {
        let receiver = mock_type_key("Handler");
        
        // Same name, different parameter types
        let key1 = MethodKey::new(receiver.clone(), "append".into(), &[Type::I64]);
        let key2 = MethodKey::new(receiver.clone(), "append".into(), &[Type::F64]);
        
        // Keys should be different due to different param hashes
        assert_ne!(key1.param_signature_hash, key2.param_signature_hash);
        assert_ne!(key1, key2);
    }
    
    #[test]
    fn test_method_key_hash_same_params() {
        let receiver = mock_type_key("Handler");
        
        // Same name, same parameter types
        let key1 = MethodKey::new(receiver.clone(), "append".into(), &[Type::I64]);
        let key2 = MethodKey::new(receiver.clone(), "append".into(), &[Type::I64]);
        
        // Keys should be identical
        assert_eq!(key1.param_signature_hash, key2.param_signature_hash);
        assert_eq!(key1, key2);
    }
    
    #[test]
    fn test_legacy_key_has_zero_hash() {
        let receiver = mock_type_key("Handler");
        let key = MethodKey::legacy(receiver, "append".into());
        assert_eq!(key.param_signature_hash, 0);
    }
    
    #[test]
    fn test_registry_overload_resolution() {
        let mut registry = TraitRegistry::new();
        let receiver = mock_type_key("Handler");
        
        // Register two overloads
        let key_i64 = MethodKey::new(receiver.clone(), "format".into(), &[Type::I64]);
        let key_f64 = MethodKey::new(receiver.clone(), "format".into(), &[Type::F64]);
        
        registry.register_method(key_i64.clone(), ResolvedMethod {
            func: mock_salt_fn("format_i64"),
            self_ty: Some(Type::Struct("Handler".into())),
            imports: vec![],
        });
        
        registry.register_method(key_f64.clone(), ResolvedMethod {
            func: mock_salt_fn("format_f64"),
            self_ty: Some(Type::Struct("Handler".into())),
            imports: vec![],
        });
        
        // Should have 2 overloads
        let overloads = registry.get_method_overloads(&receiver, "format");
        assert_eq!(overloads.len(), 2);
        
        // Resolution with i64 should find format_i64
        let resolved = registry.resolve_overload(&receiver, "format", &[Type::I64]);
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap().func.name.to_string(), "format_i64");
        
        // Resolution with f64 should find format_f64
        let resolved = registry.resolve_overload(&receiver, "format", &[Type::F64]);
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap().func.name.to_string(), "format_f64");
    }
    
    #[test]
    fn test_mangle_includes_signature() {
        let receiver = mock_type_key("Handler");
        
        let key_legacy = MethodKey::legacy(receiver.clone(), "append".into());
        let key_i64 = MethodKey::new(receiver.clone(), "append".into(), &[Type::I64]);
        
        let mangle_legacy = key_legacy.mangle();
        let mangle_typed = key_i64.mangle();
        
        // Typed key should have signature suffix (hex hash at the end)
        // Format: base_method_hash where hash is hex
        assert!(mangle_typed.len() > mangle_legacy.len(), 
            "Typed mangle '{}' should be longer than legacy '{}'", mangle_typed, mangle_legacy);
        
        // The typed mangle should start with the legacy mangle
        assert!(mangle_typed.starts_with(&mangle_legacy),
            "Typed mangle '{}' should start with legacy '{}'", mangle_typed, mangle_legacy);
    }
    
    // =========================================================================
    // TDD: Primitive Type Trait Method Lookup
    // =========================================================================
    // When impl Hash for i64 is registered with path ["std", "hash"], the lookup
    // for i64.hash() must succeed even when the lookup uses no path or different path.
    
    fn mock_primitive_key(name: &str) -> TypeKey {
        // Primitive registered with full path (e.g., from std.hash module)
        TypeKey {
            path: vec!["std".to_string(), "hash".to_string()],
            name: name.to_string(),
            specialization: None,
        }
    }
    
    fn mock_lookup_primitive_key(name: &str) -> TypeKey {
        // Lookup with empty path (e.g., from calling code that just knows "i64")
        TypeKey {
            path: vec![],
            name: name.to_string(),
            specialization: None,
        }
    }
    
    #[test]
    fn test_primitive_trait_method_lookup_with_path_mismatch() {
        let mut registry = TraitRegistry::new();
        
        // Register i64.hash() with full path (as it would be when loaded from std.hash module)
        let registration_key = mock_primitive_key("i64");
        let method_key = MethodKey::new(registration_key.clone(), "hash".into(), &[]);
        
        registry.register_method(method_key, ResolvedMethod {
            func: mock_salt_fn("hash"),
            self_ty: Some(Type::I64),
            imports: vec![],
        });
        
        // Lookup should succeed even with empty path
        let lookup_key = mock_lookup_primitive_key("i64");
        let result = registry.get_method_legacy(&lookup_key, "hash");
        
        assert!(result.is_some(), 
            "Primitive trait method lookup for 'hash' on i64 should succeed even with path mismatch");
        assert_eq!(result.unwrap().func.name.to_string(), "hash",
            "Should return the registered hash method");
    }
    
    #[test]
    fn test_primitive_trait_method_find_by_name() {
        let mut registry = TraitRegistry::new();
        
        // Register i64.hash() with full path
        let registration_key = mock_primitive_key("i64");
        let method_key = MethodKey::new(registration_key.clone(), "hash".into(), &[]);
        
        registry.register_method(method_key, ResolvedMethod {
            func: mock_salt_fn("hash"),
            self_ty: Some(Type::I64),
            imports: vec![],
        });
        
        // find_method_by_name should work with just "i64" as type_name
        let result = registry.find_method_by_name("i64", "hash", &Type::I64);
        
        assert!(result.is_some(), 
            "find_method_by_name for 'hash' on 'i64' should succeed");
    }
    
    // =========================================================================
    // Qualified Trait Identity: same-named types across modules
    // =========================================================================

    /// Register greet() under (path, name) and return the registry.
    fn registry_with_greet(path: &[&str], type_name: &str, body_name: &str) -> TraitRegistry {
        let mut registry = TraitRegistry::new();
        let key = TypeKey {
            path: path.iter().map(|s| s.to_string()).collect(),
            name: type_name.to_string(),
            specialization: None,
        };
        registry.register_method(
            MethodKey::new(key, "greet".into(), &[]),
            ResolvedMethod {
                func: mock_salt_fn(body_name),
                self_ty: Some(Type::Struct(type_name.to_string())),
                imports: vec![],
            },
        );
        registry
    }

    #[test]
    fn qualified_type_name_resolves_to_its_own_module_impl() {
        let registry = registry_with_greet(&["alpha"], "Config", "greet_alpha");

        let result = registry.find_method_by_name("alpha__Config", "greet", &Type::Unit);

        assert!(result.is_some(), "qualified lookup must find the alpha impl");
        assert_eq!(result.unwrap().0.name.to_string(), "greet_alpha",
            "qualified identity must resolve to the alpha module impl");
    }

    #[test]
    fn same_named_types_in_different_modules_never_cross_resolve() {
        let mut registry = registry_with_greet(&["alpha", "cfg"], "Config", "greet_alpha");
        let beta_key = TypeKey {
            path: vec!["beta".to_string(), "cfg".to_string()],
            name: "Config".to_string(),
            specialization: None,
        };
        registry.register_method(
            MethodKey::new(beta_key, "greet".into(), &[]),
            ResolvedMethod {
                func: mock_salt_fn("greet_beta"),
                self_ty: Some(Type::Struct("Config".to_string())),
                imports: vec![],
            },
        );

        let alpha = registry.find_method_by_name("alpha__cfg__Config", "greet", &Type::Unit)
            .expect("alpha lookup must succeed");
        let beta = registry.find_method_by_name("beta__cfg__Config", "greet", &Type::Unit)
            .expect("beta lookup must succeed");

        assert_eq!(alpha.0.name.to_string(), "greet_alpha",
            "fully-qualified alpha name must not resolve into beta's impl");
        assert_eq!(beta.0.name.to_string(), "greet_beta",
            "fully-qualified beta name must not resolve into alpha's impl");
    }

    #[test]
    fn bare_name_fallback_still_resolves_for_pathless_callers() {
        let registry = registry_with_greet(&["alpha"], "Config", "greet_alpha");

        let result = registry.find_method_by_name("Config", "greet", &Type::Unit);

        assert!(result.is_some(),
            "callers without path context keep the bare-name fallback");
    }
}
