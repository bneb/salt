pub struct Mangler;

impl Mangler {
    /// Joins parts of a path using the standard separator `__`.
    /// 
    /// This is the single source of truth for symbol construction.
    /// Usage: `Mangler::mangle(&["std", "collections", "vec"])` -> "std__collections__vec"
    pub fn mangle<S: AsRef<str>>(parts: &[S]) -> String {
        parts.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join("__")
    }

    /// Reverses the mangling process for display purposes.
    /// 
    /// Usage: `Mangler::demangle("std__collections__vec")` -> "std.collections.vec"
    pub fn demangle(mangled: &str) -> String {
        mangled.replace("__", ".")
    }
    
    /// Mangles a TypeKey into a symbol name.
    /// 
    /// Usage: `Mangler::mangle_type_key(&key)` -> "std__core__slab_alloc__GlobalSlabAlloc"
    pub fn mangle_type_key(key: &crate::types::TypeKey) -> String {
        let mut parts: Vec<String> = key.path.clone();
        parts.push(key.name.clone());
        parts.join("__")
    }
}
