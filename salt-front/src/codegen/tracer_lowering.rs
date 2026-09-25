//! `TypeTracer` implementation for `LoweringContext`.
//!
//! Extracted from `tracer.rs` (which keeps the trait plus the default
//! `CodegenContext` implementation) so both files respect the 500-line
//! budget. Binary/Unary/Paren arms delegate to `tracer_binop`, whose rules
//! deliberately mirror what the EMITTER accepts and produces, so
//! constructor-argument inference can never invent a type codegen rejects.

use std::collections::BTreeMap;

use syn::Expr;

use crate::codegen::context::LoweringContext;
use crate::codegen::generic_resolver::generic_param_name;
use crate::codegen::tracer::TypeTracer;
use crate::types::Type;

impl<'a, 'ctx> TypeTracer for LoweringContext<'a, 'ctx> {
    fn trace_expr_type(&self, expr: &Expr, locals: &BTreeMap<String, Type>) -> Result<Type, String> {
        match expr {
            Expr::Path(path) if path.path.segments.len() == 1 => trace_path(self, path, locals),
            Expr::Field(f) => trace_field(self, f, locals),
            Expr::MethodCall(m) => trace_method_call(self, m, locals),
            Expr::Call(c) => trace_call(self, c, locals),
            Expr::Lit(lit) => trace_lit(lit),
            Expr::Reference(r) => trace_reference(self, r, locals),
            Expr::Struct(s) => trace_struct_literal(self, s),
            Expr::Cast(c) => trace_cast(c),
            Expr::Tuple(t) => trace_tuple(self, t, locals),
            Expr::Binary(b) => super::tracer_binop::trace_binary(self, b, locals),
            Expr::Unary(u) => super::tracer_binop::trace_unary(self, u, locals),
            Expr::Paren(p_expr) => self.trace_expr_type(&p_expr.expr, locals),
            _ => Err(format!("Type tracing not implemented for expression type: {:?}", expr)),
        }
    }

    fn resolve_field_type(&self, receiver_ty: &Type, field_name: &str) -> Result<Type, String> {
        field_type(self, receiver_ty, field_name)
    }

    fn resolve_method_info(
        &self,
        receiver_ty: &Type,
        method_name: &str,
    ) -> Result<(crate::grammar::SaltFn, Option<Type>), String> {
        let (func, trait_ty, _) = self.resolve_method(receiver_ty, method_name)?;
        Ok((func, trait_ty))
    }

    fn substitute_generics(&self, ty: &Type, type_map: &BTreeMap<String, Type>) -> Type {
        ty.substitute(type_map)
    }

    fn canonicalize_type(&self, ty: &Type) -> Type {
        canonicalize(ty, self)
    }
}

/// Resolution order: local variables, then FQN/global lookup, then unit globals.
fn trace_path(ctx: &LoweringContext, path: &syn::ExprPath, locals: &BTreeMap<String, Type>) -> Result<Type, String> {
    let name = path.path.segments[0].ident.to_string();
    if let Some(ty) = locals.get(&name) {
        return Ok(ty.clone());
    }
    if let Ok(key) = ctx.resolve_path_to_fqn(&path.path) {
        if let Some(ty) = ctx.lookup_global_type(&key) {
            return Ok(ty);
        }
        // Function items live in discovery.globals under their MANGLED
        // name as Type::Fn. A bare fn name used as a value IS a
        // signature-typed item (emission coerces it to a pointer where a
        // fn-pointer slot demands it); tracing it keeps payload
        // conformance able to reject fn items in scalar slots instead of
        // silently storing entry addresses.
        if let Some(ty @ Type::Fn(..)) = ctx.discovery.globals.get(&key.mangle()) {
            return Ok(ty.clone());
        }
    }
    if let Some(ty) = ctx.discovery.globals.get(&name) {
        return Ok(ty.clone());
    }
    Err(format!("KeuOS Tracer: Unknown local or path: {}", name))
}

fn trace_field(ctx: &LoweringContext, f: &syn::ExprField, locals: &BTreeMap<String, Type>) -> Result<Type, String> {
    let receiver_ty = ctx.trace_expr_type(&f.base, locals)?;
    match &f.member {
        syn::Member::Named(ident) => ctx.resolve_field_type(&receiver_ty, &ident.to_string()),
        syn::Member::Unnamed(_) => Err("KeuOS Tracer: Tuple indexing not implemented in tracer.".to_string()),
    }
}

fn trace_method_call(ctx: &LoweringContext, m: &syn::ExprMethodCall, locals: &BTreeMap<String, Type>) -> Result<Type, String> {
    let receiver_ty = ctx.trace_expr_type(&m.receiver, locals)?;
    let (func, trait_ty) = ctx.resolve_method_info(&receiver_ty, &m.method.to_string())?;
    let mut type_map = BTreeMap::new();
    type_map.insert("Self".to_string(), trait_ty.unwrap_or_else(|| receiver_ty.clone()));
    insert_turbofish_bindings(&mut type_map, &func, &m.turbofish)?;
    // Handle Turbofish and Generic Substitution, then substitute into the return.
    let ret_ty_node = func.ret_type.as_ref().and_then(Type::from_syn).unwrap_or(Type::Unit);
    Ok(ctx.substitute_generics(&ret_ty_node, &type_map))
}

/// Bind concrete types for every generic parameter covered by a turbofish.
fn insert_turbofish_bindings(
    type_map: &mut BTreeMap<String, Type>,
    func: &crate::grammar::SaltFn,
    turbofish: &Option<syn::AngleBracketedGenericArguments>,
) -> Result<(), String> {
    let Some(tf) = turbofish else { return Ok(()) };
    let Some(g) = &func.generics else { return Ok(()) };
    for (i, param) in g.params.iter().enumerate() {
        if let Some(syn::GenericArgument::Type(ty)) = tf.args.iter().nth(i) {
            // Use the Ptr-aware from_std bridge
            let syn_ty = crate::grammar::SynType::from_std(ty.clone()).map_err(|e| e.to_string())?;
            let concrete = Type::from_syn(&syn_ty).unwrap_or(Type::Unit);
            type_map.insert(generic_param_name(param), concrete);
        }
    }
    Ok(())
}

fn trace_call(ctx: &LoweringContext, c: &syn::ExprCall, locals: &BTreeMap<String, Type>) -> Result<Type, String> {
    let Expr::Path(p) = &*c.func else { return Ok(Type::Unit) };
    let path_string = p.path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>().join("::");
    if path_string.ends_with("size_of") { return Ok(Type::I64); }
    // Ptr Offset maintains pointer identity
    if path_string.contains("ptr_offset") {
        if let Some(first) = c.args.first() {
            return ctx.trace_expr_type(first, locals);
        }
    }
    let key = ctx.resolve_path_to_fqn(&p.path)?;
    // Last segment's turbofish args bind the fn's own generic params
    // positionally; without this, `Pair::<i64>::swapped_with::<f32>(7)`
    // traces as raw placeholder `U`/`T` and later casts reject.
    // Const/type args from ALL segments, in order: struct-level params
    // (first segment, e.g. Cache::<64>) precede method-level ones. Leftover
    // return-type placeholders bind positionally against this list.
    let fn_turbofish: Vec<Type> = p.path.segments.iter().flat_map(|seg| {
        if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
            args.args.iter().filter_map(|g| match g {
                syn::GenericArgument::Type(t) =>
                    crate::grammar::SynType::from_std(t.clone()).ok()
                        .and_then(|st| Type::from_syn(&st)),
                                syn::GenericArgument::Const(const_expr) =>
                                    crate::codegen::types::generic_arg::GenericArg
                                        ::from_const_expr(const_expr, ctx.evaluator)
                                        .ok()
                                        .map(|g| g.to_legacy_type()),
                _ => None,
            }).collect::<Vec<_>>()
        } else { Vec::new() }
    }).collect();
    // Signature lookups yield the whole Type::Fn; a call's traced type is
    // its RETURN type. (Before payload conformance consumed traced types
    // strictly, this path leaked full signatures and callers deferred.)
    if let Some((_, sig)) = ctx.resolve_global_signature(&key.mangle()) {
        let ret = crate::codegen::tracer::call_return_type(sig);
        return Ok(crate::codegen::types::substitution::bind_call_return_placeholders(&ret, &fn_turbofish));
    }
    // Free fns defined in this unit live in discovery.globals as Type::Fn;
    // a call yields their RETURN type.
    if let Some(Type::Fn(_, boxed_ret)) = ctx.discovery.globals.get(&key.mangle()) {
        let ret = (**boxed_ret).clone();
        return Ok(crate::codegen::types::substitution::bind_call_return_placeholders(&ret, &fn_turbofish));
    }
    Ok(Type::Unit)
}

fn trace_lit(lit: &syn::ExprLit) -> Result<Type, String> {
    match &lit.lit {
        // String literals are StringView by default
        syn::Lit::Str(_) => Ok(Type::Struct("std__core__str__StringView".to_string())),
        syn::Lit::Int(_) => Ok(Type::I64),
        syn::Lit::Bool(_) => Ok(Type::Bool),
        syn::Lit::Float(_) => Ok(Type::F32), // Salt defaults to f32 for benchmarks
        _ => Ok(Type::Unit),
    }
}

fn trace_reference(ctx: &LoweringContext, r: &syn::ExprReference, locals: &BTreeMap<String, Type>) -> Result<Type, String> {
    let inner = ctx.trace_expr_type(&r.expr, locals)?;
    Ok(Type::Reference(Box::new(inner), r.mutability.is_some()))
}

// Struct literal: Node { val: 42 } => Type::Struct("main__Node")
fn trace_struct_literal(ctx: &LoweringContext, s: &syn::ExprStruct) -> Result<Type, String> {
    let raw_name = s.path.segments.iter()
        .map(|seg| seg.ident.to_string())
        .collect::<Vec<_>>()
        .join("__");
    Ok(Type::Struct(resolve_struct_fqn(ctx, &raw_name)))
}

/// Canonical Resolution: construct an FQN candidate deterministically from the
/// current package, then verify against the Symbol Table (struct_templates).
fn resolve_struct_fqn(ctx: &LoweringContext, raw_name: &str) -> String {
    let candidate = match ctx.current_package.as_ref() {
        Some(pkg) => {
            let prefix = pkg.name.iter().map(|id| id.to_string()).collect::<Vec<_>>().join("__");
            if prefix.is_empty() { raw_name.to_string() } else { format!("{}__{}", prefix, raw_name) }
        }
        None => raw_name.to_string(),
    };
    if ctx.struct_templates().contains_key(&candidate) {
        candidate
    } else if ctx.struct_templates().contains_key(raw_name) {
        // Already-qualified or no-package struct
        raw_name.to_string()
    } else {
        // Fallback: external dependency or forward ref
        candidate
    }
}

fn trace_cast(c: &syn::ExprCast) -> Result<Type, String> {
    let syn_ty = crate::grammar::SynType::from_std((*c.ty).clone()).map_err(|e| e.to_string())?;
    Type::from_syn(&syn_ty).ok_or_else(|| "Cannot trace cast target type".to_string())
}

/// Tuple literal: trace every element so unannotated tuple-payload enum
/// constructors like Result::Ok((1, 2)) can infer their payload generics.
fn trace_tuple(ctx: &LoweringContext, t: &syn::ExprTuple, locals: &BTreeMap<String, Type>) -> Result<Type, String> {
    let elems = t.elems.iter()
        .map(|e| ctx.trace_expr_type(e, locals))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Type::Tuple(elems))
}

/// Field access: dereference through References/Pointers to the base struct,
/// then resolve the field via templates first and the registry second.
fn field_type(ctx: &LoweringContext, receiver_ty: &Type, field_name: &str) -> Result<Type, String> {
    let mut current = receiver_ty;
    while let Some(inner) = current.get_ptr_element() {
        current = inner;
    }
    let (base_name, concrete_args) = match current {
        Type::Struct(n) => (n.clone(), vec![]),
        Type::Concrete(n, args) => (n.clone(), args.clone()),
        _ => return Err(format!("Cannot access field '{}' on non-struct type {:?}", field_name, current)),
    };
    if let Some(ty) = field_type_via_template(ctx, &base_name, &concrete_args, field_name) {
        return Ok(ty);
    }
    if let Some(ty) = field_type_via_registry(ctx, current, field_name) {
        return Ok(ty);
    }
    Err(format!("Field '{}' not found in struct {}", field_name, base_name))
}

/// Template lookup; generic parameters are substituted from concrete args.
fn field_type_via_template(
    ctx: &LoweringContext,
    base_name: &str,
    concrete_args: &[Type],
    field_name: &str,
) -> Option<Type> {
    let struct_def = ctx.struct_templates().get(base_name)?;
    let f = struct_def.fields.iter().find(|f| f.name == field_name)?;
    let raw_field_ty = Type::from_syn(&f.ty).unwrap_or(Type::Unit);
    // Canonicalize raw struct names from AST to FQNs
    let field_ty = ctx.canonicalize_type(&raw_field_ty);
    let mut map = BTreeMap::new();
    if let Some(g) = &struct_def.generics {
        for (i, param) in g.params.iter().enumerate() {
            if let Some(arg) = concrete_args.get(i) {
                map.insert(generic_param_name(param), arg.clone());
            }
        }
    }
    Some(ctx.substitute_generics(&field_ty, &map))
}

/// Fallback to structural lookup in the registry.
fn field_type_via_registry(ctx: &LoweringContext, current: &Type, field_name: &str) -> Option<Type> {
    let info = ctx.lookup_struct_by_type(current)?;
    info.fields.get(field_name).map(|(_, ty)| ty.clone())
}

/// Recursively canonicalize raw struct names within a Type tree.
/// E.g., Concrete("Ptr", [Struct("Node")]) => Concrete("Ptr", [Struct("main__Node")])
fn canonicalize(ty: &Type, ctx: &LoweringContext) -> Type {
    match ty {
        Type::Struct(raw_name) => canonicalize_struct_name(ty, raw_name, ctx),
        Type::Concrete(name, args) => Type::Concrete(name.clone(), canon_all(args, ctx)),
        Type::Reference(inner, mutable) => Type::Reference(Box::new(canonicalize(inner, ctx)), *mutable),
        Type::Pointer { element, provenance, is_mutable } => Type::Pointer {
            element: Box::new(canonicalize(element, ctx)),
            provenance: provenance.clone(),
            is_mutable: *is_mutable,
        },
        Type::Tuple(elems) => Type::Tuple(canon_all(elems, ctx)),
        Type::Array(inner, size, packed) => Type::Array(Box::new(canonicalize(inner, ctx)), *size, *packed),
        // Primitives, Unit, etc. — no struct names to canonicalize
        _ => ty.clone(),
    }
}

fn canon_all(args: &[Type], ctx: &LoweringContext) -> Vec<Type> {
    args.iter().map(|a| canonicalize(a, ctx)).collect()
}

fn canonicalize_struct_name(original: &Type, raw_name: &str, ctx: &LoweringContext) -> Type {
    // Skip already-qualified names and single-uppercase generic placeholders.
    if raw_name.contains("__") || is_generic_placeholder(raw_name) {
        return original.clone();
    }
    let candidate = match ctx.current_package.as_ref() {
        Some(pkg) => {
            let prefix = pkg.name.iter().map(|id| id.to_string()).collect::<Vec<_>>().join("__");
            if prefix.is_empty() { raw_name.to_string() } else { format!("{}__{}", prefix, raw_name) }
        }
        None => return original.clone(),
    };
    if ctx.struct_templates().contains_key(&candidate) {
        Type::Struct(candidate)
    } else {
        original.clone() // External or generic param — keep as-is
    }
}

fn is_generic_placeholder(name: &str) -> bool {
    name.len() == 1 && name.chars().all(|c| c.is_uppercase())
}
