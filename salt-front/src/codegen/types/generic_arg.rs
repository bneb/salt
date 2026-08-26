//! WS-R R1: unified representation replacing string-composed identities
//! (`Generic("SIZE")`, `Concrete("main__SIZE")`, `Struct("64")`, ...) with
//! typed values. R1 ships the API unwired; call sites migrate in R2.
//! Anchors: evaluator.rs:7, types.rs:62, scan_types.rs:139-151, grammar.rs:626.
#![allow(dead_code)] // R1 introduces the API only; consumed from R2 on.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use crate::evaluator::ConstValue;
use crate::grammar::GenericParam;
use crate::types::Type;

/// One generic argument: a nominal type or an evaluated const value.
///
/// Manual `Eq`/`Hash`/`Ord` because `ConstValue` holds `f64`, which derives
/// neither. Floats are identified by bit pattern with `-0.0` folded onto
/// `0.0`, so all three traits share ONE canonical key (identity order, not
/// numeric order) — the rustc approach: evaluated float consts are keyed by
/// raw bits rather than stored in maps as `f64`.
#[derive(Clone, Debug)]
pub(crate) enum GenericArg {
    Type(Type),
    Const(ConstValue),
}

impl GenericArg {
    pub(crate) fn from_type(ty: Type) -> Self { Self::Type(ty) }

    pub(crate) fn from_const_value(value: ConstValue) -> Self { Self::Const(value) }

    pub(crate) fn as_type(&self) -> Option<&Type> {
        match self {
            Self::Type(ty) => Some(ty),
            Self::Const(_) => None,
        }
    }

    pub(crate) fn as_const(&self) -> Option<&ConstValue> {
        match self {
            Self::Const(value) => Some(value),
            Self::Type(_) => None,
        }
    }

    /// Parses a legacy value-spelling such as "64" back into a const arg.
    /// Only strict decimal integers (`[+-]?[0-9]+`) become `Const`: Rust's
    /// f64 parser also accepts "inf"/"nan", which are legal Salt identifiers
    /// (red-team RT4 finding). Anything else becomes `Ok(Type(..))` when
    /// `allow_non_numeric` (placeholders like Struct("SIZE")), else `Err`.
    pub(crate) fn from_legacy_struct_value(spelling: &str, allow_non_numeric: bool) -> Result<Self, String> {
        let digits = spelling.strip_prefix(['+', '-']).unwrap_or(spelling);
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(i) = spelling.parse::<i64>() {
                return Ok(Self::Const(ConstValue::Integer(i)));
            }
        }
        if allow_non_numeric { return Ok(Self::Type(Type::Struct(spelling.to_string()))); }
        Err(format!("not a legacy const spelling: {spelling:?}"))
    }

    /// Maps args to their string spellings. The INTEGER class reproduces the
    /// pre-T-c convention exactly (`Integer(i)` -> `Struct(decimal)`) --
    /// behavior-neutrality for everything that worked before. T-c extends
    /// the previously-BROKEN classes: Bool consts spell as the reserved
    /// keywords "true"/"false" (no user type can collide; see expr/utils
    /// is_value_spelled_leaf) and Float consts as digit-safe IEEE bits
    /// (-0.0 folded onto 0.0 via float_key) -- Display would emit "inf"/
    /// "NaN" (legal identifiers!) and '.', which bypasses the value-leaf
    /// guard and re-opens pkg__2_5-style ghost minting. Array/Complex have
    /// no turbofish literal syntax and keep the legacy Struct("0") spell.
    pub(crate) fn to_legacy_type(&self) -> Type {
        match self {
            Self::Type(ty) => ty.clone(),
            Self::Const(ConstValue::Integer(i)) => Type::Struct(i.to_string()),
            Self::Const(ConstValue::Bool(b)) => Type::Struct(b.to_string()),
            Self::Const(ConstValue::Float(f)) => Type::Struct(float_key(*f).to_string()),
            Self::Const(_) => Type::Struct("0".to_string()),
        }
    }

    /// Extracts one const-generic argument from turbofish syntax with VALUE
    /// semantics: hex/underscore/Unary-Neg literals normalize through the
    /// evaluator exactly as scan_types does, so every creation site agrees
    /// on a single canonical spelling per value. Int literals that do not
    /// fit i64 return the [E003] refusal diagnostic (T-b).
    pub(crate) fn from_const_expr(
        expr: &syn::Expr,
        evaluator: &crate::evaluator::Evaluator,
    ) -> Result<Self, String> {
        if let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(li), .. }) = expr {
            let val = li.base10_parse::<i64>()
                .map_err(|_| unrepresentable_const_diag(li.base10_digits()))?;
            return Ok(Self::Const(ConstValue::Integer(val)));
        }
        let value = evaluator.eval_expr(expr).map_err(|e| format!("{:?}", e))?;
        Ok(Self::Const(value))
    }
}

/// True when `leaf` is spelled as an integer literal: optional sign followed
/// by one or more ASCII digits ("-7", "+5", "007"), or a reserved keyword
/// ("true"/"false"). Such leaves are const-generic VALUES surfacing as
/// `Type::Struct(v.to_string())`, never symbols; package qualification and
/// refusing InstanceId constructors must reject them instead of minting
/// `pkg__-7`-style identities. CANONICAL definition -- expr/utils delegates.
pub(crate) fn is_value_spelled_leaf(leaf: &str) -> bool {
    if leaf == "true" || leaf == "false" { return true; }
    let digits = leaf.strip_prefix('-').or_else(|| leaf.strip_prefix('+')).unwrap_or(leaf);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// WS-R3/T-b refusal diagnostic for an integer literal in const-generic
/// position that cannot represent an `i64`. Compilation must FAIL with this
/// message instead of silently dropping the argument: the drop minted ghost
/// param-name identities like `main__O_K` plus ptr-typed unsuffixed calls.
/// E003 per src/errors.rs conventions: comptime-value failures refine to the
/// compile-stage code (cf. lib.rs "comptime evaluation failed").
pub(crate) fn unrepresentable_const_diag(digits: &str) -> String {
    // Cause-line convention: refine without repeating the banner code.
    format!(
        "const generic argument `{digits}` does not fit in i64 \
         (valid range -9223372036854775808..=9223372036854775807)"
    )
}

/// Canonical float identity key: IEEE bits with -0.0 folded onto 0.0.
fn float_key(f: f64) -> u64 {
    let bits = f.to_bits();
    if bits == 0 || bits == 0x8000_0000_0000_0000 { 0 } else { bits }
}

const fn const_rank(value: &ConstValue) -> u8 {
    match value {
        ConstValue::Integer(_) => 0,
        ConstValue::Float(_) => 1,
        ConstValue::Bool(_) => 2,
        ConstValue::String(_) => 3,
        ConstValue::Array(_) => 4,
        ConstValue::Complex => 5,
    }
}

fn const_eq(a: &ConstValue, b: &ConstValue) -> bool {
    match (a, b) {
        (ConstValue::Integer(x), ConstValue::Integer(y)) => x == y,
        (ConstValue::Float(x), ConstValue::Float(y)) => float_key(*x) == float_key(*y),
        (ConstValue::Bool(x), ConstValue::Bool(y)) => x == y,
        (ConstValue::String(x), ConstValue::String(y)) => x == y,
        (ConstValue::Array(xs), ConstValue::Array(ys)) => {
            xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| const_eq(x, y))
        }
        (ConstValue::Complex, ConstValue::Complex) => true,
        _ => false,
    }
}

fn cmp_arrays(xs: &[ConstValue], ys: &[ConstValue]) -> Ordering {
    for (x, y) in xs.iter().zip(ys) {
        let ord = const_cmp(x, y);
        if ord != Ordering::Equal { return ord; }
    }
    xs.len().cmp(&ys.len())
}

fn const_cmp(a: &ConstValue, b: &ConstValue) -> Ordering {
    let ord = const_rank(a).cmp(&const_rank(b));
    if ord != Ordering::Equal { return ord; }
    match (a, b) {
        (ConstValue::Integer(x), ConstValue::Integer(y)) => x.cmp(y),
        (ConstValue::Float(x), ConstValue::Float(y)) => float_key(*x).cmp(&float_key(*y)),
        (ConstValue::Bool(x), ConstValue::Bool(y)) => x.cmp(y),
        (ConstValue::String(x), ConstValue::String(y)) => x.cmp(y),
        (ConstValue::Array(xs), ConstValue::Array(ys)) => cmp_arrays(xs, ys),
        _ => Ordering::Equal, // only Complex/Complex shares a rank here
    }
}

impl PartialEq for GenericArg {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Type(a), Self::Type(b)) => a == b,
            (Self::Const(a), Self::Const(b)) => const_eq(a, b),
            _ => false,
        }
    }
}
impl Eq for GenericArg {}

impl Hash for GenericArg {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Type(ty) => { state.write_u8(0); ty.hash(state); }
            Self::Const(value) => { state.write_u8(1); hash_const(value, state); }
        }
    }
}

fn hash_const<H: Hasher>(value: &ConstValue, state: &mut H) {
    match value {
        ConstValue::Integer(i) => { state.write_u8(0); i.hash(state); }
        ConstValue::Float(f) => { state.write_u8(1); state.write_u64(float_key(*f)); }
        ConstValue::Bool(b) => { state.write_u8(2); b.hash(state); }
        ConstValue::String(s) => { state.write_u8(3); s.hash(state); }
        ConstValue::Array(items) => {
            state.write_u8(4);
            state.write_usize(items.len());
            for item in items { hash_const(item, state); }
        }
        ConstValue::Complex => state.write_u8(5),
    }
}

impl Ord for GenericArg {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Type(a), Self::Type(b)) => a.cmp(b),
            (Self::Const(a), Self::Const(b)) => const_cmp(a, b),
            (Self::Type(_), Self::Const(_)) => Ordering::Less,
            (Self::Const(_), Self::Type(_)) => Ordering::Greater,
        }
    }
}
impl PartialOrd for GenericArg {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

/// Mangled name of the item declaring the parameter list ("main__Cache").
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ParamOwner(String);

impl ParamOwner {
    pub(crate) fn mangled(owner: &str) -> Self { Self(owner.to_string()) }
    pub(crate) fn as_str(&self) -> &str { &self.0 }
}

/// Identity of one declared parameter: `(owner, declaration index)` — what
/// placeholders become instead of bare strings. The declared name rides along
/// for diagnostics only and is excluded from Eq/Hash/Ord.
#[derive(Clone, Debug)]
pub(crate) struct ParamId {
    owner: ParamOwner,
    index: u32,
    name: String,
}

impl ParamId {
    /// Captures the declared name of `param`; identity stays `(owner, index)`.
    pub(crate) fn from_grammar_param(owner: &str, param: &GenericParam, index: u32) -> Self {
        let name = match param {
            GenericParam::Type { name, .. } | GenericParam::Const { name, .. } => name.to_string(),
        };
        Self { owner: ParamOwner::mangled(owner), index, name }
    }

    pub(crate) fn owner(&self) -> &ParamOwner { &self.owner }
    pub(crate) fn index(&self) -> u32 { self.index }
}

impl fmt::Display for ParamId {
    /// Diagnostic rendering for unavoidable legacy string contexts,
    /// e.g. "main__Cache::NODE#1".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}#{}", self.owner.as_str(), self.name, self.index)
    }
}

impl PartialEq for ParamId {
    fn eq(&self, other: &Self) -> bool { self.owner == other.owner && self.index == other.index }
}
impl Eq for ParamId {}

impl Hash for ParamId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.owner.hash(state);
        self.index.hash(state);
    }
}

impl Ord for ParamId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.owner.cmp(&other.owner).then(self.index.cmp(&other.index))
    }
}
impl PartialOrd for ParamId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
