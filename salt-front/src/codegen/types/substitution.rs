use crate::types::Type;
use std::collections::BTreeMap;
use crate::codegen::context::LoweringContext;

/// Returns true when a type_map entry maps `name` back to itself.
/// `Concrete(name)` counts too: a const param bound to its own bare-name
/// Concrete (`SIZE -> Concrete("SIZE")`, from turbofish parsed outside
/// generic-name context) makes substitution infinitely self-recursive
/// unless treated as self-reference here.
fn is_self_ref(n: &str, c: &Type) -> bool {
    matches!(c, Type::Struct(s) | Type::Generic(s) | Type::Concrete(s, _) if s == n)
}

/// True when `name` (or its pkg-stripped leaf: `main__SIZE` -> `SIZE`)
/// is a declared param in the active map. Placeholder spellings reach
/// substitution pre-prefixed from several layers; all must normalize.
fn maps_param(m: &std::collections::BTreeMap<String, Type>, name: &str) -> bool {
    if m.contains_key(name) { return true; }
    if let Some(idx) = name.rfind("__") {
        return m.contains_key(&name[idx + 2..]);
    }
    false
}

fn sub_through(m: &std::collections::BTreeMap<String, Type>, n: &str, ty: &Type) -> Type {
    // Resolve through prefixed spellings to the owning param leaf.
    let leaf = if m.contains_key(n) {
        n
    } else if let Some(idx) = n.rfind("__") {
        &n[idx + 2..]
    } else {
        n
    };
    let Some(c) = m.get(leaf) else { return ty.clone(); };
    if is_self_ref(leaf, c) { return Type::Generic(leaf.to_string()); }
    substitute_generics(m, c)
}

fn try_suffix(m: &std::collections::BTreeMap<String, Type>, n: &str) -> Option<Type> {
    let f = m.get(n);
    let s = if n.contains("__") { m.get(n.rsplit("__").next()?) } else { None };
    f.or(s).map(|c| substitute_generics(m, c))
}

/// Recursively substitute generic placeholders using current_type_map.
/// When HashMap<i64, i64> references Entry<K, V>, this function consults the
/// active type context to produce Entry<i64, i64>.
///
/// Totality: pathological type maps can make entries expand into types that
/// re-reference themselves (const params bound to template-shaped Concretes),
/// growing the result without bound. A generous depth cap keeps the function
/// total; real-world generic nesting stays far below it.
pub fn substitute_generics(type_map: &std::collections::BTreeMap<String, Type>, ty: &Type) -> Type {
    thread_local! {
        static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }
    let d = DEPTH.with(|c| c.get()) + 1;
    DEPTH.with(|c| c.set(d));
    let _guard = DepthReset;
    struct DepthReset;
    impl Drop for DepthReset {
        fn drop(&mut self) {
            DEPTH.with(|c| c.set(c.get() - 1));
        }
    }
    if d > 256 {
        return ty.clone();
    }
    match ty {
        Type::Struct(name) if maps_param(type_map, name) => sub_through(type_map, name, ty),
        Type::Generic(name) => sub_through(type_map, name, ty),
        Type::Concrete(name, args) => {
            if args.is_empty() {
                if let Some(result) = try_suffix(type_map, name) {
                    return result;
                }
            }
            let substituted_args: Vec<Type> = args.iter()
                .map(|a| substitute_generics(type_map, a))
                .collect();
            Type::Concrete(name.clone(), substituted_args)
        }
        Type::SelfType => {
            let Some(concrete) = type_map.get("Self") else { return ty.clone(); };
            substitute_generics(type_map, concrete)
        }
        Type::Pointer { element, provenance, is_mutable } => {
            Type::Pointer {
                element: Box::new(substitute_generics(type_map, element)),
                provenance: provenance.clone(),
                is_mutable: *is_mutable,
            }
        }
        Type::Reference(inner, mutability) => {
            Type::Reference(Box::new(substitute_generics(type_map, inner)), *mutability)
        }
        Type::Array(inner, len, packed) => {
            Type::Array(Box::new(substitute_generics(type_map, inner)), *len, *packed)
        }
        Type::Tuple(elems) => {
            Type::Tuple(elems.iter().map(|e| substitute_generics(type_map, e)).collect())
        }
        Type::Fn(args, ret) => {
            Type::Fn(
                args.iter().map(|a| substitute_generics(type_map, a)).collect(),
                Box::new(substitute_generics(type_map, ret)),
            )
        }
        _ => ty.clone()
    }
}

/// Convenience wrapper: extracts type_map from CodegenContext.
pub fn substitute_generics_ctx(ctx: &mut LoweringContext, ty: &Type) -> Type {
    let type_map = ctx.current_type_map();
    substitute_generics(type_map, ty)
}

/// Collects the generic placeholder names a call's return type still
/// carries, in order of first appearance. A placeholder is a `Generic`
/// or a zero-arg `Concrete` whose name is not a known mangled path
/// (i.e. an unsubstituted type/const param like `T` or `U`).
pub(crate) fn collect_placeholder_names(ty: &Type, out: &mut Vec<String>) {
    match ty {
        Type::Generic(n) => {
            if !out.contains(n) { out.push(n.clone()); }
        }
        Type::Struct(n) if !n.contains("__") && !is_primitive_name(n) => {
            if !out.contains(n) { out.push(n.clone()); }
        }
        Type::Concrete(n, args) if args.is_empty() && !n.contains("__") => {
            if !out.contains(n) { out.push(n.clone()); }
        }
        Type::Pointer { element, .. } | Type::Owned(element) | Type::Tensor(element, _) =>
            collect_placeholder_names(element, out),
        Type::Reference(inner, _) => collect_placeholder_names(inner, out),
        Type::Array(inner, _, _) => collect_placeholder_names(inner, out),
        Type::Fn(args, ret) => {
            for a in args { collect_placeholder_names(a, out); }
            collect_placeholder_names(ret, out);
        }
        Type::Tuple(elems) => {
            for e in elems { collect_placeholder_names(e, out); }
        }
        Type::Concrete(_, args) => {
            for a in args { collect_placeholder_names(a, out); }
        }
        _ => {}
    }
}

fn rewrite_placeholder(ty: &mut Type, bindings: &BTreeMap<String, Type>) {
    match ty {
        Type::Generic(n) => {
            if let Some(bound) = bindings.get(n) { *ty = bound.clone(); }
        }
        Type::Struct(n) if !n.contains("__") => {
            if let Some(bound) = bindings.get(n) { *ty = bound.clone(); }
        }
        Type::Concrete(n, args) if args.is_empty() && !n.contains("__") => {
            if let Some(bound) = bindings.get(n) { *ty = bound.clone(); }
        }
        Type::Pointer { element, .. } | Type::Owned(element) | Type::Tensor(element, _) =>
            rewrite_placeholder(element, bindings),
        Type::Reference(inner, _) => rewrite_placeholder(inner, bindings),
        Type::Array(inner, _, _) => rewrite_placeholder(inner, bindings),
        Type::Fn(args, ret) => {
            for a in args { rewrite_placeholder(a, bindings); }
            rewrite_placeholder(ret, bindings);
        }
        Type::Tuple(elems) => {
            for e in elems { rewrite_placeholder(e, bindings); }
        }
        Type::Concrete(_, args) => {
            for a in args { rewrite_placeholder(a, bindings); }
        }
        _ => {}
    }
}

/// Binds leftover generic placeholders in a resolved call return type to
/// the LAST path segment's turbofish args, positionally. Tracers resolve
/// signatures without fn-generic context, so
/// `Pair::<i64>::swapped_with::<f32>(7)` otherwise keeps raw placeholders
/// (`U`) that later casts reject ("Unsupported explicit cast T -> i32").
pub(crate) fn bind_call_return_placeholders(ret: &Type, turbofish_args: &[Type]) -> Type {
    if turbofish_args.is_empty() { return ret.clone(); }
    let mut names = Vec::new();
    collect_placeholder_names(ret, &mut names);
    if names.is_empty() { return ret.clone(); }
    let mut bindings = BTreeMap::new();
    for (name, arg) in names.into_iter().zip(turbofish_args.iter()) {
        bindings.insert(name, arg.clone());
    }
    let mut out = ret.clone();
    rewrite_placeholder(&mut out, &bindings);
    out
}

fn is_primitive_name(n: &str) -> bool {
    matches!(n, "i8"|"i16"|"i32"|"i64"|"u8"|"u16"|"u32"|"u64"|"usize"|"f32"|"f64"|"bool")
}

/// Public wrapper: rewrites bare template placeholders (including
/// `Struct("T")` forms produced by context-free type resolution)
/// against the given bindings.
pub(crate) fn rewrite_bare_placeholders(mut ty: Type, bindings: &BTreeMap<String, Type>) -> Type {
    rewrite_placeholder(&mut ty, bindings);
    ty
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn map_of(pairs: &[(&str, Type)]) -> BTreeMap<String, Type> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    #[test]
    fn test_substitute_generic_binds() {
        let m = map_of(&[("T", Type::I64)]);
        assert_eq!(substitute_generics(&m, &Type::Generic("T".into())), Type::I64);
    }

    #[test]
    fn test_substitute_struct_key() {
        let m = map_of(&[("T", Type::I64)]);
        assert_eq!(substitute_generics(&m, &Type::Struct("T".into())), Type::I64);
    }

    #[test]
    fn test_substitute_generic_not_in_map() {
        let m = BTreeMap::new();
        assert_eq!(substitute_generics(&m, &Type::Generic("T".into())), Type::Generic("T".into()));
    }

    #[test]
    fn test_substitute_concrete_args() {
        let m = map_of(&[("T", Type::I64)]);
        let ty = Type::Concrete("Vec".into(), vec![Type::Generic("T".into())]);
        let expected = Type::Concrete("Vec".into(), vec![Type::I64]);
        assert_eq!(substitute_generics(&m, &ty), expected);
    }

    #[test]
    fn test_substitute_self_type() {
        let m = map_of(&[("Self", Type::Struct("Foo".into()))]);
        assert_eq!(substitute_generics(&m, &Type::SelfType), Type::Struct("Foo".into()));
    }

    #[test]
    fn test_substitute_pointer_recurse() {
        let m = map_of(&[("T", Type::I32)]);
        let ty = Type::Pointer { element: Box::new(Type::Generic("T".into())), provenance: crate::types::Provenance::Naked, is_mutable: false };
        let expected = Type::Pointer { element: Box::new(Type::I32), provenance: crate::types::Provenance::Naked, is_mutable: false };
        assert_eq!(substitute_generics(&m, &ty), expected);
    }

    #[test]
    fn test_substitute_no_infinite_loop() {
        let m = map_of(&[("T", Type::Generic("T".into()))]);
        assert_eq!(substitute_generics(&m, &Type::Struct("T".into())), Type::Generic("T".into()));
    }

    #[test]
    fn test_substitute_fn_type() {
        let m = map_of(&[("T", Type::I64), ("R", Type::Bool)]);
        let ty = Type::Fn(
            vec![Type::Generic("T".into()), Type::I32],
            Box::new(Type::Generic("R".into())),
        );
        let expected = Type::Fn(
            vec![Type::I64, Type::I32],
            Box::new(Type::Bool),
        );
        assert_eq!(substitute_generics(&m, &ty), expected);
    }

    #[test]
    fn test_substitute_array_type() {
        let m = map_of(&[("T", Type::I32)]);
        let ty = Type::Array(Box::new(Type::Generic("T".into())), 10, false);
        let expected = Type::Array(Box::new(Type::I32), 10, false);
        assert_eq!(substitute_generics(&m, &ty), expected);
    }

    #[test]
    fn test_substitute_tuple_type() {
        let m = map_of(&[("A", Type::I32), ("B", Type::F64)]);
        let ty = Type::Tuple(vec![Type::Generic("A".into()), Type::Generic("B".into())]);
        let expected = Type::Tuple(vec![Type::I32, Type::F64]);
        assert_eq!(substitute_generics(&m, &ty), expected);
    }

    #[test]
    fn test_try_suffix_concrete_no_args() {
        // Concrete("foo__Bar", []) with no args triggers try_suffix:
        // last __ component "Bar" is looked up in the type_map
        let m = map_of(&[("Bar", Type::I64)]);
        let ty = Type::Concrete("foo__Bar".into(), vec![]);
        assert_eq!(substitute_generics(&m, &ty), Type::I64);
    }

    #[test]
    fn test_try_suffix_concrete_with_args_skips_suffix() {
        // Concrete with args bypasses try_suffix and substitutes args instead
        let m = map_of(&[("Bar", Type::I64)]);
        let ty = Type::Concrete("foo__Bar".into(), vec![Type::I32]);
        let expected = Type::Concrete("foo__Bar".into(), vec![Type::I32]);
        assert_eq!(substitute_generics(&m, &ty), expected);
    }

    #[test]
    fn test_substitute_reference_immutable() {
        let m = map_of(&[("T", Type::I64)]);
        let ty = Type::Reference(Box::new(Type::Generic("T".into())), false);
        let expected = Type::Reference(Box::new(Type::I64), false);
        assert_eq!(substitute_generics(&m, &ty), expected);
    }

    #[test]
    fn test_substitute_reference_mutable() {
        let m = map_of(&[("T", Type::I32)]);
        let ty = Type::Reference(Box::new(Type::Generic("T".into())), true);
        let expected = Type::Reference(Box::new(Type::I32), true);
        assert_eq!(substitute_generics(&m, &ty), expected);
    }

    #[test]
    fn test_substitute_self_type_not_in_map() {
        let m = BTreeMap::new();
        assert_eq!(substitute_generics(&m, &Type::SelfType), Type::SelfType);
    }
}

