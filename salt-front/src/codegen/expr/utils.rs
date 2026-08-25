use crate::types::Type;
use crate::codegen::context::LoweringContext;
use crate::common::mangling::Mangler;

pub fn is_aggregate(ty: &Type) -> bool {
    matches!(ty, Type::Array(..) | Type::Struct(_) | Type::Window(_, _) | Type::Tuple(_) | Type::Concrete(_, _))
}

pub fn get_path_from_expr(expr: &syn::Expr) -> Option<Vec<String>> {
    let res: Option<Vec<String>> = match expr {
        syn::Expr::Path(p) => Some(p.path.segments.iter().map(|s| s.ident.to_string()).collect()),
        syn::Expr::Field(f) => {
            let mut segments = get_path_from_expr(&f.base)?;
            if let syn::Member::Named(id) = &f.member {
                segments.push(id.to_string());
                Some(segments)
            } else { None }
        }
        syn::Expr::Index(_) => {
            // Array indexing breaks the namespace chain.
            // `TABLE[idx].low` is a field access on a computed value, NOT a module path `TABLE.low`.
            // Return None to force through the proper LValue/field access path.
            None
        }
        _ => None
    };

    if let Some(ref s) = res {
        // 'self' is an instance value, never a static package.
        // Prevent "self.field" from being resolved as static path "self::field".
        if s.first().map(|x| x.as_str()) == Some("self") {
            return None;
        }
    }
    res
}

/// Extract turbofish type arguments from path expressions.
/// For `HashMap::<i64, i64>::with_capacity`, this extracts [i64, i64] from the path segment.
pub fn get_path_turbofish_args(expr: &syn::Expr) -> Vec<syn::Type> {
    match expr {
        syn::Expr::Path(p) => {
            // Extract turbofish from the last segment with generic arguments
            for segment in p.path.segments.iter().rev() {
                if let syn::PathArguments::AngleBracketed(ref args) = segment.arguments {
                    let mut types = Vec::new();
                    for arg in &args.args {
                        if let syn::GenericArgument::Type(ty) = arg {
                            types.push(ty.clone());
                        }
                    }
                    if !types.is_empty() {
                        return types;
                    }
                }
            }
            vec![]
        }
        _ => vec![]
    }
}


fn check_explicit_and_implicit_alias(
    registry: Option<&crate::registry::Registry>,
    imp: &crate::grammar::ImportDecl,
    first: &str,
    segments: &[String],
) -> Option<(String, String)> {
    if let Some(alias) = &imp.alias {
        if alias == first {
            if segments.len() > 1 {
                let item_name = &segments[1];
                if let Some(reg) = registry {
                    let import_path = imp.name.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(".");
                    if let Some(mod_info) = reg.modules.get(&import_path) {
                        if let Some(export) = mod_info.exports.get(item_name) {
                            if matches!(export.kind, crate::registry::SymbolKind::Intrinsic) {
                                return Some((item_name.clone(), String::new()));
                            }
                        }
                    }
                }
            }
            let base_pkg = Mangler::mangle(&imp.name.iter().map(|id| id.to_string()).collect::<Vec<_>>());
            let item = Mangler::mangle(&segments[1..]);
            return Some((base_pkg, item));
        }
    } 

    if let Some(last) = imp.name.last() {
        if *last == *first {
            if segments.len() > 1 {
                let item_name = &segments[1];
                if let Some(reg) = registry {
                    let import_path = imp.name.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(".");
                    if let Some(mod_info) = reg.modules.get(&import_path) {
                        if let Some(export) = mod_info.exports.get(item_name) {
                            if matches!(export.kind, crate::registry::SymbolKind::Intrinsic) {
                                return Some((item_name.clone(), String::new()));
                            }
                        }
                    }
                }
            }
            let base_pkg = Mangler::mangle(&imp.name.iter().map(|id| id.to_string()).collect::<Vec<_>>());
            let item = if segments.len() > 1 { Mangler::mangle(&segments[1..]) } else { String::new() };
            return Some((base_pkg, item));
        }
    }

    if let Some(group) = &imp.group {
         if group.iter().any(|id| *id == *first) {
            let base = Mangler::mangle(&imp.name.iter().map(|id| id.to_string()).collect::<Vec<_>>());
            let item = Mangler::mangle(segments);
            return Some((base, item));
         }
    }
    None
}

fn check_wildcard_resolution(
    registry: Option<&crate::registry::Registry>,
    imports: &[crate::grammar::ImportDecl],
    segments: &[String],
) -> Option<(String, String)> {
    if let Some(reg) = registry {
        for imp in imports.iter() {
            let is_wildcard = imp.alias.is_none() && imp.group.is_none() && !imp.name.is_empty();
            if is_wildcard {
                let import_path = imp.name.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(".");
                
                if let Some(mod_info) = reg.modules.get(&import_path) {
                    if !segments.is_empty() {
                        let type_name = &segments[0];
                        
                        if let Some(export) = mod_info.exports.get(type_name) {
                            match export.kind {
                                crate::registry::SymbolKind::LeafType => {
                                    let pkg_prefix = mod_info.package.replace(".", "__");
                                    let item = Mangler::mangle(segments);
                                    return Some((pkg_prefix, item));
                                }
                                crate::registry::SymbolKind::Function => {
                                    let pkg_prefix = mod_info.package.replace(".", "__");
                                    if segments.len() == 1 {
                                        return Some((pkg_prefix, type_name.clone()));
                                    } else {
                                        let item = Mangler::mangle(segments);
                                        return Some((pkg_prefix, item));
                                    }
                                }
                                crate::registry::SymbolKind::Intrinsic => {
                                    return None;
                                }
                                crate::registry::SymbolKind::Namespace => {}
                                crate::registry::SymbolKind::Alias => {}
                            }
                        }
                        
                        if mod_info.struct_templates.contains_key(type_name) 
                            || mod_info.structs.contains_key(type_name)
                            || mod_info.enum_templates.contains_key(type_name) {
                            let pkg_prefix = mod_info.package.replace(".", "__");
                            let item = Mangler::mangle(segments);
                            return Some((pkg_prefix, item));
                        }
                        
                        if segments.len() == 1 && mod_info.functions.contains_key(type_name) {
                            let pkg_prefix = mod_info.package.replace(".", "__");
                            return Some((pkg_prefix, type_name.clone()));
                        }
                    }
                }
            }
        }
    }
    None
}

/// True when `leaf` is spelled as an integer literal: optional sign followed
/// by one or more ASCII digits ("-7", "+5", "007", "9223372036854775807").
/// Such leaves are const-generic VALUES surfacing as `Type::Struct(v.to_string())`,
/// never symbols; package qualification must refuse them instead of minting a
/// `pkg__-7` identity (T-a: turbofish caller/callee identity split). The
/// keyword spellings "true"/"false" are refused for the same reason ahead of
/// the T-c value-identity work: they are reserved words, so no user type can
/// bear those names, and the Mangler does not escape.
fn is_value_spelled_leaf(leaf: &str) -> bool {
    crate::codegen::types::generic_arg::is_value_spelled_leaf(leaf)
}

pub fn resolve_package_prefix(
    registry: Option<&crate::registry::Registry>,
    imports: &[crate::grammar::ImportDecl],
    external_decls: &std::collections::HashSet<String>,
    current_package: Option<&crate::grammar::PackageDecl>,
    segments: &[String],
) -> Option<(String, String)> {
    if segments.is_empty() { return None; }
    // T-a: value-spelled leaves (const-generic arguments) must never qualify
    // into `pkg__-7`-style identities.
    if is_value_spelled_leaf(&segments[segments.len() - 1]) { return None; }

    if segments[0] == "std" {
        for i in (1..=segments.len()).rev() {
            let namespace = Mangler::mangle(&segments[0..i]);
            if let Some(reg) = registry {
                let mod_path = segments[0..i].join(".");
                if reg.modules.contains_key(&mod_path) {
                    return Some((namespace, Mangler::mangle(&segments[i..])));
                }
            }
        }
        return Some((Mangler::mangle(segments), String::new()));
    }

    let first = &segments[0];
    for imp in imports.iter() {
        if let Some(res) = check_explicit_and_implicit_alias(registry, imp, first, segments) {
            return Some(res);
        }
    }
    
    for i in (1..=segments.len()).rev() {
        let namespace = segments[0..i].join(".");
        for imp in imports.iter() {
            let full: String = imp.name.iter().map(|id: &syn::Ident| id.to_string()).collect::<Vec<_>>().join(".");
            if full == namespace {
                let full_pkg: String = Mangler::mangle(&imp.name.iter().map(|id: &syn::Ident| id.to_string()).collect::<Vec<_>>());
                let item = if i < segments.len() { Mangler::mangle(&segments[i..]) } else { String::new() };
                return Some((full_pkg, item));
            }
        }
    }

    if segments.len() == 1 {
        let name = &segments[0];
        let intrinsics = ["size_of", "align_of", "zeroed", "popcount", "ctpop"];
        if intrinsics.contains(&name.as_str()) || name.starts_with("intrin_") || 
           name.contains("ptr_offset") || name.contains("ptr_read") || name.contains("ptr_write") {
            return None;
        }
    }
    
    if segments.len() == 1 {
        let name = &segments[0];
        if external_decls.contains(name) {
            return None;
        }
    }

    if let Some(res) = check_wildcard_resolution(registry, imports, segments) {
        return Some(res);
    }
    
    let pkg_name = {
        let pkg = current_package?;
        Mangler::mangle(&pkg.name.iter().map(|id| id.to_string()).collect::<Vec<_>>())
    };
    
    if pkg_name.is_empty() {
        return None;
    }
    
    let item = Mangler::mangle(segments);
    Some((pkg_name, item))
}

/// Convenience wrapper: extracts parameters from CodegenContext for `resolve_package_prefix`.
/// Callers migrating to LoweringContext should call the pure function directly.
pub fn resolve_package_prefix_ctx(ctx: &LoweringContext, segments: &[String]) -> Option<(String, String)> {
    let pkg_guard = &*ctx.current_package;
    let imports_guard = ctx.imports();
    let ext_decls_guard = ctx.external_decls();
    resolve_package_prefix(ctx.config.registry, imports_guard, ext_decls_guard, pkg_guard.as_ref(), segments)
}

pub fn get_name_from_expr(expr: &syn::Expr) -> Option<String> {
    if let syn::Expr::Path(p) = expr {
        if p.path.segments.len() == 1 {
            return Some(p.path.segments[0].ident.to_string());
        }
    }
    None
}

pub use crate::codegen::expr::enum_ctor::{resolve_path_to_enum, EnumVariantResolution};

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    #[allow(unused_imports)]
    use crate::codegen::expr::enum_ctor::{ordered_complete_generics, unify_payload_pattern};
    #[allow(unused_imports)]
    use std::collections::BTreeMap as TestBTreeMap;
    #[allow(unused_imports)]
    type SynType = crate::grammar::SynType;
    
    /// Test that wildcard detection correctly identifies different import types
    #[test]
    fn test_wildcard_import_detection() {
        use syn::punctuated::Punctuated;
        use syn::token::Dot;
        use proc_macro2::Span;
        
        // Helper to create an ImportDecl
        fn make_import(path: &[&str], alias: Option<&str>, group: Option<Vec<&str>>) -> crate::grammar::ImportDecl {
            let mut name: Punctuated<syn::Ident, Dot> = Punctuated::new();
            for (i, segment) in path.iter().enumerate() {
                name.push_value(syn::Ident::new(segment, Span::call_site()));
                if i < path.len() - 1 {
                    name.push_punct(Default::default());
                }
            }
            crate::grammar::ImportDecl {
                name,
                alias: alias.map(|a| syn::Ident::new(a, Span::call_site())),
                group: group.map(|g| g.iter().map(|s| syn::Ident::new(s, Span::call_site())).collect()),
            }
        }
        
        // Case 1: Wildcard import - use std::string::* -> import std.string (no alias, no group)
        let wildcard = make_import(&["std", "string"], None, None);
        let is_wildcard = wildcard.alias.is_none() && wildcard.group.is_none() && !wildcard.name.is_empty();
        assert!(is_wildcard, "Pure module import should be detected as potential wildcard");
        
        // Case 2: Single item import - use std::string::String -> import std.string.String (no alias, no group)
        // NOTE: This also has no alias/group, but the last segment is an item, not a module!
        let single_item = make_import(&["std", "string", "String"], None, None);
        let is_single_item_detected_as_wildcard = single_item.alias.is_none() && single_item.group.is_none() && !single_item.name.is_empty();
        // This WILL be detected as wildcard by basic check - disambiguation happens via Registry lookup
        assert!(is_single_item_detected_as_wildcard, "Single item import also passes basic wildcard check");
        
        // Case 3: Aliased import - use std::string::String as Str
        let aliased = make_import(&["std", "string", "String"], Some("Str"), None);
        let is_aliased_wildcard = aliased.alias.is_none() && aliased.group.is_none();
        assert!(!is_aliased_wildcard, "Aliased import should NOT be wildcard");
        
        // Case 4: Group import - use std::string::{String, from_literal}
        let grouped = make_import(&["std", "string"], None, Some(vec!["String", "from_literal"]));
        let is_grouped_wildcard = grouped.alias.is_none() && grouped.group.is_none();
        assert!(!is_grouped_wildcard, "Group import should NOT be wildcard");
        
        // Case 5: Empty import path (edge case)
        let empty = make_import(&[], None, None);
        let is_empty_wildcard = empty.alias.is_none() && empty.group.is_none() && !empty.name.is_empty();
        assert!(!is_empty_wildcard, "Empty import should NOT be wildcard");
    }
    
    /// Test that import path construction works correctly  
    #[test]
    fn test_import_path_construction() {
        use syn::punctuated::Punctuated;
        use syn::token::Dot;
        use proc_macro2::Span;
        
        let mut name: Punctuated<syn::Ident, Dot> = Punctuated::new();
        name.push_value(syn::Ident::new("std", Span::call_site()));
        name.push_punct(Default::default());
        name.push_value(syn::Ident::new("string", Span::call_site()));
        
        let import_path = name.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(".");
        assert_eq!(import_path, "std.string", "Import path should be dot-separated");
    }
    
    /// Test that mangled import names are NOT treated as module paths
    #[test]
    fn test_mangled_names_not_modules() {
        use syn::punctuated::Punctuated;
        use syn::token::Dot;
        use proc_macro2::Span;
        
        // A mangled import like "std__collections__vec__Vec" is a single segment
        let mut name: Punctuated<syn::Ident, Dot> = Punctuated::new();
        name.push_value(syn::Ident::new("std__collections__vec__Vec", Span::call_site()));
        
        let import_path = name.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(".");
        assert_eq!(import_path, "std__collections__vec__Vec", "Mangled name is single segment");
        
        // This should NOT match any module in registry (modules are "std.collections.vec")
        // The wildcard fix should only activate if registry.modules.get(&import_path) returns Some
    }

    #[test]
    fn test_unify_payload_binds_generic_from_arg_type() {
        let payload: SynType = syn::parse_str("T").expect("payload type must parse");
        let declared = vec!["T".to_string()];
        let mut map = TestBTreeMap::new();
        unify_payload_pattern(&declared, &payload, &Type::I64, &mut map);
        assert_eq!(map.get("T"), Some(&Type::I64),
            "Payload pattern T must bind to the traced argument type I64");
    }

    #[test]
    fn test_unify_payload_concrete_pattern_binds_nothing() {
        let payload: SynType = syn::parse_str("i32").expect("payload type must parse");
        let declared: Vec<String> = vec![];
        let mut map = TestBTreeMap::new();
        unify_payload_pattern(&declared, &payload, &Type::I64, &mut map);
        assert!(map.is_empty(),
            "Concrete payload patterns must not inject generic bindings");
    }

    #[test]
    fn test_ordered_complete_generics_requires_full_coverage() {
        let declared = vec!["A".to_string(), "B".to_string()];
        let mut partial = TestBTreeMap::new();
        partial.insert("A".to_string(), Type::I64);
        assert!(ordered_complete_generics(&declared, &partial).is_empty(),
            "Partially bound templates must yield no generics (no guessing)");
        partial.insert("B".to_string(), Type::Bool);
        assert_eq!(ordered_complete_generics(&declared, &partial), vec![Type::I64, Type::Bool],
            "Fully bound templates must be projected in declaration order");
    }

    #[test]
    fn test_value_spelled_leaf_classification() {
        for lit in ["-7", "+5", "007", "0", "9223372036854775807"] {
            assert!(super::is_value_spelled_leaf(lit), "`{lit}` is a value");
        }
        for sym in ["S_-7", "S_7", "main__S", "_7", "3x", "-", "+", "--4", "1_000", "0x10"] {
            assert!(!super::is_value_spelled_leaf(sym), "`{sym}` stays symbol-eligible");
        }
        // T-c rail: reserved keyword spellings are refused ahead of the
        // value-identity work so no pkg__true ghost can ever be minted.
        assert!(super::is_value_spelled_leaf("true"));
        assert!(super::is_value_spelled_leaf("false"));
    }
}
