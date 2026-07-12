use crate::hir::types::Type;
use crate::hir::ids::{DefId, VarId};

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: Type,
    pub span: proc_macro2::Span,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Literal(Literal),
    Path(DefId), 
    /// Resolved local variable reference. VarId is globally unique within the function.
    Var(VarId),
    /// Unresolved identifier (pre-resolution fallback).
    UnresolvedIdent(String),
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    Block(Block),
    If {
        cond: Box<Expr>,
        then_branch: Block,
        else_branch: Option<Box<Expr>>,
    },
    While {
        cond: Box<Expr>,
        body: Block,
    },
    Loop(Block),
    Return(Option<Box<Expr>>),
    Break,
    Continue,
    /// Field access: expr.field
    Field {
        base: Box<Expr>,
        field: String,
    },
    /// Type cast: expr as Type
    Cast {
        expr: Box<Expr>,
        ty: Type,
    },
    /// Struct literal: Name<TypeArgs> { field: value, ... }
    StructLit {
        name: String,
        type_args: Vec<Type>,
        fields: Vec<(String, Expr)>,
    },
    /// Assignment: lhs = rhs
    Assign {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// Index: expr[index]
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    /// Address-of: &expr (produces a Reference type)
    Ref(Box<Expr>),
    /// Method call: receiver.method(args)
    MethodCall {
        receiver: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
    /// `requires(bool_expr)` - pre-condition contract (evaluates to Unit)
    Requires(Box<Expr>),
    /// `ensures(bool_expr)` - post-condition contract (evaluates to Unit)
    Ensures(Box<Expr>),
    /// `yield expr` — suspend coroutine, optionally producing a value.
    /// `yield;` => Yield(None) — OS scheduling suspension
    /// `yield 42;` => Yield(Some(42)) — generator data streaming
    Yield(Option<Box<Expr>>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum BinOp {
    Add, Sub, Mul, Div, Rem,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
    BitAnd, BitOr, BitXor,
    Shl, Shr,
    AddAssign, SubAssign, MulAssign, DivAssign, RemAssign,
}

impl BinOp {
    /// Returns true for comparison/relational operators that produce Bool.
    pub fn is_relational(&self) -> bool {
        matches!(self, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum UnOp {
    Not, Neg, Deref,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<super::stmt::Stmt>,
    pub value: Option<Box<Expr>>,
    pub ty: Type,
}
