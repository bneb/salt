//! Lexical Scope Stack for HIR Lowering
//!
//! Mimics how a human reads shadowed variables in nested blocks.
//! Each `push_scope()` / `pop_scope()` pair corresponds to a `{ ... }` block.

use std::collections::HashMap;
use crate::hir::ids::VarId;

/// A stack of lexical scopes for variable resolution.
/// Index 0 is the function's root scope (arguments).
/// The last element is the currently active inner block.
#[derive(Debug, Default)]
pub struct ScopeStack {
    scopes: Vec<HashMap<String, VarId>>,
}

impl ScopeStack {
    /// Create a new ScopeStack with a root scope.
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    /// Enter a new block `{`
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Exit a block `}`
    pub fn pop_scope(&mut self) {
        assert!(self.scopes.len() > 1, "Cannot pop the root scope");
        self.scopes.pop();
    }

    /// Declare a new variable in the CURRENT innermost scope.
    pub fn insert(&mut self, name: String, var_id: VarId) {
        if let Some(current_scope) = self.scopes.last_mut() {
            current_scope.insert(name, var_id);
        }
    }

    /// Lookup a variable, starting from the innermost scope and moving outward.
    pub fn resolve(&self, name: &str) -> Option<VarId> {
        for scope in self.scopes.iter().rev() {
            if let Some(&var_id) = scope.get(name) {
                return Some(var_id);
            }
        }
        None
    }

    /// How many scopes are currently on the stack.
    pub fn depth(&self) -> usize {
        self.scopes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_scope_insert_and_resolve() {
        let mut stack = ScopeStack::new();
        stack.insert("x".into(), VarId(0));
        assert_eq!(stack.resolve("x"), Some(VarId(0)));
        assert_eq!(stack.resolve("y"), None);
    }

    #[test]
    fn test_nested_scope_shadows_outer() {
        let mut stack = ScopeStack::new();
        stack.insert("x".into(), VarId(0));

        stack.push_scope();
        stack.insert("x".into(), VarId(1)); // shadows outer x
        assert_eq!(stack.resolve("x"), Some(VarId(1)));

        stack.pop_scope();
        assert_eq!(stack.resolve("x"), Some(VarId(0))); // outer x restored
    }

    #[test]
    fn test_inner_scope_sees_outer_variables() {
        let mut stack = ScopeStack::new();
        stack.insert("a".into(), VarId(0));

        stack.push_scope();
        stack.insert("b".into(), VarId(1));
        assert_eq!(stack.resolve("a"), Some(VarId(0))); // sees outer
        assert_eq!(stack.resolve("b"), Some(VarId(1))); // sees inner

        stack.pop_scope();
        assert_eq!(stack.resolve("a"), Some(VarId(0)));
        assert_eq!(stack.resolve("b"), None); // b is gone
    }

    #[test]
    fn test_three_level_nesting() {
        let mut stack = ScopeStack::new();
        stack.insert("x".into(), VarId(0));

        stack.push_scope();
        stack.insert("y".into(), VarId(1));

        stack.push_scope();
        stack.insert("z".into(), VarId(2));
        assert_eq!(stack.resolve("x"), Some(VarId(0)));
        assert_eq!(stack.resolve("y"), Some(VarId(1)));
        assert_eq!(stack.resolve("z"), Some(VarId(2)));

        stack.pop_scope();
        assert_eq!(stack.resolve("z"), None);
        assert_eq!(stack.resolve("y"), Some(VarId(1)));

        stack.pop_scope();
        assert_eq!(stack.resolve("y"), None);
        assert_eq!(stack.resolve("x"), Some(VarId(0)));
    }

    #[test]
    #[should_panic(expected = "Cannot pop the root scope")]
    fn test_cannot_pop_root() {
        let mut stack = ScopeStack::new();
        stack.pop_scope(); // should panic
    }

    #[test]
    fn test_depth_tracking() {
        let mut stack = ScopeStack::new();
        assert_eq!(stack.depth(), 1);
        stack.push_scope();
        assert_eq!(stack.depth(), 2);
        stack.push_scope();
        assert_eq!(stack.depth(), 3);
        stack.pop_scope();
        assert_eq!(stack.depth(), 2);
    }
}
