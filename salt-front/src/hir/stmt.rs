use crate::hir::expr::{Expr, Block};
use crate::hir::types::Type;
use crate::hir::ids::VarId;

#[derive(Clone, Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: proc_macro2::Span,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    /// `let x: T = expr;`
    Local(Local),
    /// Expression statement (no semicolon, value used)
    Expr(Expr),
    /// Expression statement with semicolon (value discarded)
    Semi(Expr),
    /// `while cond { body }`
    While { cond: Expr, body: Block },
    /// `for pat in iter { body }`
    For { var: VarId, var_name: String, iter: Expr, body: Block },
    /// `loop { body }`
    Loop(Block),
    /// `return expr;`
    Return(Option<Expr>),
    /// `break;`
    Break,
    /// `continue;`
    Continue,
    /// Compiler-injected invariant from CFG analysis.
    /// Unlike `Requires` (which Z3 must prove), `Assume` is unconditionally
    /// absorbed into the solver context as ground truth.
    Assume(Expr),
}

#[derive(Clone, Debug)]
pub struct Local {
    pub pat: Pattern,
    pub ty: Option<Type>,
    pub init: Option<Expr>,
}

/// Pattern in a `let` binding — carries VarId for resolved bindings.
#[derive(Clone, Debug)]
pub enum Pattern {
    /// `let x = ...` or `let mut x = ...`
    Bind { name: String, var_id: VarId, mutable: bool },
    Tuple(Vec<Pattern>),
    Wildcard,
}
