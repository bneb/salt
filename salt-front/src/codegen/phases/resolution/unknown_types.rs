//! Rejects type names in the entry file that nothing declares.
//!
//! Codegen lowers a type name it cannot resolve to a struct alias that no
//! definition backs: `Box<i32>` without `use std.core.boxed.Box;` became
//! `!struct_Box_i32`, and salt-opt failed with "undefined symbol alias id".
//! A bare name is known when it is a generic parameter in scope, a type the
//! compiler lowers itself, or a struct, enum, trait or concept declared by
//! the entry file or by any loaded module (the prelude, the imports and
//! theirs: the modules codegen's own lookup searches). An import alias is
//! known when the item it renames is. Checked positions: fn and extern fn
//! signatures, struct fields, enum payloads, impl targets and methods,
//! trait methods, concepts, globals and consts; function bodies are not.

use std::collections::{HashMap, HashSet};
use syn::punctuated::Punctuated;
use syn::token::Comma;
use crate::grammar::{Arg, GenericParam, Generics, Item, SaltFile, SaltFn, SaltImpl, SaltTrait, SynPath, SynType};

/// Names codegen lowers without a declaration in any module.
const BUILTIN_TYPES: &[&str] = &[
    "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "usize", "f32", "f64", "bool",
    "Self", "LlvmPtr", "Tensor", "Owned", "Window", "Atomic", "Simd",
    "Vector4f32", "Vector8f32", "Vector4f64", "Vector16f32",
];

/// Fails with one line per unknown type name, naming where it is first used.
pub fn check<'a>(file: &'a SaltFile, loaded: impl IntoIterator<Item = &'a SaltFile>) -> Result<(), String> {
    let mut checker = Checker::new(file, loaded);
    for item in &file.items {
        checker.item(item);
    }
    if checker.unknown.is_empty() {
        return Ok(());
    }
    let lines: Vec<String> = checker.unknown.iter()
        .map(|(name, site)| format!(
            "unknown type `{}` in {}: not declared in this file, in any module it imports, or in the prelude",
            name, site
        ))
        .collect();
    Err(lines.join("\n"))
}

struct Checker {
    declared: HashSet<String>,
    renamed: HashMap<String, String>,
    generics: Vec<String>,
    site: String,
    unknown: Vec<(String, String)>,
}

impl Checker {
    fn new<'a>(file: &'a SaltFile, loaded: impl IntoIterator<Item = &'a SaltFile>) -> Self {
        let files = loaded.into_iter().chain(std::iter::once(file));
        let declared = files.flat_map(|f| f.items.iter().filter_map(declared_name)).collect();
        let renamed = file.imports.iter()
            .filter_map(|imp| Some((imp.alias.as_ref()?.to_string(), imp.name.last()?.to_string())))
            .collect();
        Checker { declared, renamed, generics: Vec::new(), site: String::new(), unknown: Vec::new() }
    }

    fn item(&mut self, item: &Item) {
        match item {
            Item::Fn(f) => self.function(f),
            Item::ExternFn(f) => {
                self.site = format!("extern fn `{}`", f.name);
                self.signature(&f.args, &f.ret_type);
            }
            Item::Struct(s) => {
                self.site = format!("struct `{}`", s.name);
                self.scoped(&s.generics, |c| s.fields.iter().for_each(|field| c.ty(&field.ty)));
            }
            Item::Enum(e) => {
                self.site = format!("enum `{}`", e.name);
                self.scoped(&e.generics, |c| e.variants.iter().flat_map(|v| &v.tys).for_each(|t| c.ty(t)));
            }
            Item::Impl(imp) => self.impl_block(imp),
            Item::Trait(t) => self.trait_def(t),
            Item::Concept(k) => {
                self.site = format!("concept `{}`", k.name);
                self.scoped(&k.generics, |c| c.ty(&k.param_ty));
            }
            Item::Global(g) => self.typed_item("global", &g.name, &g.ty),
            Item::Const(k) => self.typed_item("const", &k.name, &k.ty),
        }
    }

    fn function(&mut self, f: &SaltFn) {
        self.site = format!("fn `{}`", f.name);
        self.scoped(&f.generics, |c| c.signature(&f.args, &f.ret_type));
    }

    fn signature(&mut self, args: &Punctuated<Arg, Comma>, ret: &Option<SynType>) {
        for ty in args.iter().filter_map(|arg| arg.ty.as_ref()).chain(ret) {
            self.ty(ty);
        }
    }

    fn typed_item(&mut self, kind: &str, name: &syn::Ident, ty: &SynType) {
        self.site = format!("{} `{}`", kind, name);
        self.ty(ty);
    }

    fn impl_block(&mut self, imp: &SaltImpl) {
        let (target, methods, generics) = match imp {
            SaltImpl::Concept { target_ty, .. } => (target_ty, &[][..], &None),
            SaltImpl::Methods { target_ty, methods, generics }
            | SaltImpl::Trait { target_ty, methods, generics, .. } => (target_ty, methods.as_slice(), generics),
        };
        self.scoped(generics, |c| {
            c.site = "impl block".to_string();
            c.ty(target);
            methods.iter().for_each(|m| c.function(m));
        });
    }

    fn trait_def(&mut self, t: &SaltTrait) {
        self.scoped(&t.generics, |c| {
            for m in &t.methods {
                c.site = format!("trait `{}`", t.name);
                c.scoped(&m.generics, |c| c.signature(&m.args, &m.ret_type));
            }
            t.default_methods.iter().for_each(|m| c.function(m));
        });
    }

    /// Runs `visit` with `generics` in scope, then drops them.
    fn scoped(&mut self, generics: &Option<Generics>, visit: impl FnOnce(&mut Self)) {
        let depth = self.generics.len();
        let params = generics.iter().flat_map(|g| g.params.iter());
        self.generics.extend(params.map(|param| match param {
            GenericParam::Type { name, .. } | GenericParam::Const { name, .. } => name.to_string(),
        }));
        visit(self);
        self.generics.truncate(depth);
    }

    fn ty(&mut self, ty: &SynType) {
        match ty {
            // `&str` lowers to a pointer, so `str` never reaches the MLIR.
            SynType::Reference(inner, _) if is_bare(inner, "str") => {}
            SynType::Pointer(inner) | SynType::Reference(inner, _) | SynType::Array(inner, _) => self.ty(inner),
            SynType::ShapedTensor { element, .. } => self.ty(element),
            SynType::Tuple(tuple) => tuple.elems.iter().for_each(|e| self.ty(e)),
            SynType::FnPtr(args, ret) => args.iter().chain(ret.as_deref()).for_each(|t| self.ty(t)),
            SynType::Path(path) => self.path(path),
            SynType::Other(_) => {}
        }
    }

    /// A qualified name (`boxed.Box`) is known when some loaded module
    /// declares its last segment; the module path is codegen's to resolve.
    fn path(&mut self, path: &SynPath) {
        let Some(last) = path.segments.last() else { return };
        let known = match path.segments.as_slice() {
            [_] => self.bare_name_known(&last.ident.to_string()),
            _ => self.declared.contains(&last.ident.to_string()),
        };
        if !known {
            let written: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
            self.record(written.join("."));
        }
        for segment in &path.segments {
            // Window<T, Region>: the second argument names a memory region.
            let types = if segment.ident == "Window" { 1 } else { segment.args.len() };
            segment.args.iter().take(types).for_each(|arg| self.ty(arg));
        }
    }

    fn bare_name_known(&self, name: &str) -> bool {
        let declared_as = self.renamed.get(name).map_or(name, String::as_str);
        self.generics.iter().any(|g| g == name)
            || is_implicit_generic(name)
            || BUILTIN_TYPES.contains(&name)
            || self.declared.contains(declared_as)
    }

    fn record(&mut self, written: String) {
        if !self.unknown.iter().any(|(seen, _)| *seen == written) {
            self.unknown.push((written, self.site.clone()));
        }
    }
}

/// `Type::from_syn` reads an undeclared single capital letter as a generic
/// parameter, so `impl Pool<T>` needs no `impl<T>`.
fn is_implicit_generic(name: &str) -> bool {
    name.len() == 1 && name.chars().all(|c| c.is_ascii_uppercase())
}

fn is_bare(ty: &SynType, name: &str) -> bool {
    matches!(ty, SynType::Path(p) if matches!(p.segments.as_slice(), [s] if s.ident == name && s.args.is_empty()))
}

fn declared_name(item: &Item) -> Option<String> {
    let name = match item {
        Item::Struct(s) => &s.name,
        Item::Enum(e) => &e.name,
        Item::Trait(t) => &t.name,
        Item::Concept(c) => &c.name,
        _ => return None,
    };
    Some(name.to_string())
}
