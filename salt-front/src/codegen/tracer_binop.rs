//! Static typing for Binary/Unary expressions during type tracing.
//!
//! Deliberately mirrors what the EMITTER accepts and produces
//! (`determine_common_binary_type`, `PROMOTION_OPS`, `emit_binary_ptr_add`,
//! plus downstream rejection behavior) so constructor-argument inference can
//! never invent a type that codegen would later reject.

use std::collections::BTreeMap;

use crate::codegen::tracer::TypeTracer;
use crate::types::Type;

pub(crate) fn trace_binary<C: TypeTracer + ?Sized>(
    ctx: &C,
    b: &syn::ExprBinary,
    locals: &BTreeMap<String, Type>,
) -> Result<Type, String> {
    let lt = ctx.trace_expr_type(&b.left, locals)?;
    let rt = ctx.trace_expr_type(&b.right, locals)?;
    common_binop_type(&b.op, lt, rt)
}

pub(crate) fn trace_unary<C: TypeTracer + ?Sized>(
    ctx: &C,
    u: &syn::ExprUnary,
    locals: &BTreeMap<String, Type>,
) -> Result<Type, String> {
    let inner = ctx.trace_expr_type(&u.expr, locals)?;
    common_unop_type(&u.op, inner)
}

/// Static result type of a unary operator. Fail-closed, mirroring emit_unary:
/// `!` needs bool or integer, unary `-` needs numeric, deref yields the
/// POINTEE type (references and pointers alike). No lenient fallback.
pub(crate) fn common_unop_type(op: &syn::UnOp, inner: Type) -> Result<Type, String> {
    match op {
        syn::UnOp::Not(_) if inner == Type::Bool => Ok(Type::Bool),
        syn::UnOp::Not(_) if inner.is_integer() => Ok(inner),
        syn::UnOp::Not(_) => Err(format!(
            "Tracer: '!' requires a bool or integer operand, found {:?}", inner
        )),
        syn::UnOp::Neg(_) if inner.is_numeric() => Ok(inner),
        syn::UnOp::Neg(_) => Err(format!(
            "Tracer: unary '-' requires a numeric operand, found {:?}", inner
        )),
        // Mirrors emit_unary: deref yields the POINTEE type.
        syn::UnOp::Deref(_) => match inner.get_ptr_element() {
            Some(pointee) => Ok(pointee.clone()),
            None => Err(format!("Tracer: cannot deref non-pointer {:?}", inner)),
        },
        // syn::UnOp is non_exhaustive; any future operator fails closed.
        _ => Err(format!("Tracer: cannot type-check unary op on {:?}", inner)),
    }
}

/// Static result type of a binary operation, mirroring emitter outcomes:
/// comparisons yield Bool; logicals need two Bools and yield Bool; shifts
/// need two integers and keep the LHS type; tensor*tensor keeps LHS;
/// pointer arithmetic yields the POINTER operand's type; integer*float mixes
/// are REJECTED (no implicit promotion exists); other arithmetic requires
/// both operands numeric and yields the larger.
pub(crate) fn common_binop_type(
    op: &syn::BinOp,
    lt: Type,
    rt: Type,
) -> Result<Type, String> {
    if is_comparison(op) {
        return Ok(Type::Bool);
    }
    if is_logic(op) {
        return logic_result(&lt, &rt);
    }
    if is_shift(op) {
        return shift_result(&lt, &rt);
    }
    if is_bitwise(op) {
        if lt.is_integer() && rt.is_integer() {
            return Ok(lt);
        }
        return Err("Tracer: bitwise operators require integer operands".to_string());
    }
    if matches!(lt, Type::Tensor(..)) && matches!(rt, Type::Tensor(..)) {
        return Ok(lt);
    }
    if is_pointer_arith(op, &lt, &rt) {
        return Ok(pointer_operand(&lt, &rt).clone());
    }
    arithmetic_result(op, lt, rt)
}

fn logic_result(lt: &Type, rt: &Type) -> Result<Type, String> {
    if *lt == Type::Bool && *rt == Type::Bool {
        return Ok(Type::Bool);
    }
    Err(format!(
        "Tracer: logical operators require bool operands {:?} / {:?}", lt, rt
    ))
}

fn shift_result(lt: &Type, rt: &Type) -> Result<Type, String> {
    if lt.is_integer() && rt.is_integer() {
        return Ok(lt.clone());
    }
    Err(format!(
        "Tracer: shift operators require integer operands {:?} / {:?}", lt, rt
    ))
}

/// Mirrors `emit_binary_ptr_add` (Add only): `ptr + int` / `int + ptr`
/// yields the POINTER operand's own type (GEP support). `ptr - int` is
/// intentionally NOT traced as pointer arithmetic - the emitter does not
/// implement Sub-GEP, so ctor args like `base - offset` fail closed with a
/// clear arithmetic error instead of binding a type emission cannot honor.
fn is_pointer_arith(op: &syn::BinOp, lt: &Type, rt: &Type) -> bool {
    matches!(op, syn::BinOp::Add(_))
        && (matches!(lt, Type::Pointer { .. }) || matches!(rt, Type::Pointer { .. }))
}

fn pointer_operand<'a>(lt: &'a Type, rt: &'a Type) -> &'a Type {
    if matches!(lt, Type::Pointer { .. }) { lt } else { rt }
}

fn arithmetic_result(_op: &syn::BinOp, lt: Type, rt: Type) -> Result<Type, String> {
    if !lt.is_numeric() || !rt.is_numeric() {
        return Err(format!(
            "Tracer: arithmetic on non-numeric operands {:?} / {:?}",
            lt, rt
        ));
    }
    if let Some(res) = usize_special_result(&lt, &rt) {
        return Ok(res);
    }
    if int_float_mix(&lt, &rt) {
        return Err(format!(
            "Tracer: no implicit int-float promotion for {:?} / {:?}",
            lt, rt
        ));
    }
    Ok(if numeric_size(&lt) >= numeric_size(&rt) { lt } else { rt })
}

fn is_comparison(op: &syn::BinOp) -> bool {
    matches!(
        op,
        syn::BinOp::Eq(_)
            | syn::BinOp::Ne(_)
            | syn::BinOp::Lt(_)
            | syn::BinOp::Gt(_)
            | syn::BinOp::Le(_)
            | syn::BinOp::Ge(_)
    )
}

fn is_logic(op: &syn::BinOp) -> bool {
    matches!(op, syn::BinOp::And(_) | syn::BinOp::Or(_))
}

fn is_shift(op: &syn::BinOp) -> bool {
    matches!(op, syn::BinOp::Shl(_) | syn::BinOp::Shr(_))
}

fn is_bitwise(op: &syn::BinOp) -> bool {
    matches!(
        op,
        syn::BinOp::BitAnd(_) | syn::BinOp::BitOr(_) | syn::BinOp::BitXor(_)
    )
}

/// Order-independent Usize specials, mirroring the emitter table:
/// (Usize, I64) -> I64, (Usize, U64) -> U64.
fn usize_special_result(lt: &Type, rt: &Type) -> Option<Type> {
    match (lt, rt) {
        (Type::Usize, other) | (other, Type::Usize) => match other {
            Type::I64 => Some(Type::I64),
            Type::U64 => Some(Type::U64),
            _ => None,
        },
        _ => None,
    }
}

fn int_float_mix(lt: &Type, rt: &Type) -> bool {
    (lt.is_integer() && matches!(rt, Type::F32 | Type::F64))
        || (rt.is_integer() && matches!(lt, Type::F32 | Type::F64))
}

fn numeric_size(t: &Type) -> i32 {
    match t {
        Type::I8 | Type::U8 => 1,
        Type::I16 | Type::U16 => 2,
        Type::I32 | Type::U32 | Type::F32 => 4,
        Type::I64 | Type::U64 | Type::Usize | Type::F64 => 8,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Provenance;

    fn check(op: syn::BinOp, lt: Type, rt: Type) -> Result<Type, String> {
        common_binop_type(&op, lt, rt)
    }

    fn ptr_u8() -> Type {
        Type::Pointer { element: Box::new(Type::U8), provenance: Provenance::Naked, is_mutable: false }
    }

    #[test]
    fn comparisons_yield_bool() {
        let op: syn::BinOp = syn::parse_str("<").unwrap();
        assert_eq!(check(op, Type::I64, Type::I64).unwrap(), Type::Bool);
    }

    #[test]
    fn shifts_keep_lhs_for_integers_only() {
        let shl: syn::BinOp = syn::parse_str("<<").unwrap();
        assert_eq!(check(shl, Type::I32, Type::I64).unwrap(), Type::I32);
        assert!(check(shl, Type::F32, Type::I64).is_err());
        assert!(check(shl, Type::I64, Type::Bool).is_err());
    }

    #[test]
    fn logic_requires_bool_operands() {
        let and: syn::BinOp = syn::parse_str("&&").unwrap();
        assert_eq!(check(and, Type::Bool, Type::Bool).unwrap(), Type::Bool);
        assert!(check(and, Type::I64, Type::Bool).is_err());
        assert!(check(and, Type::I64, Type::I64).is_err());
    }

    #[test]
    fn not_is_fail_closed() {
        let bang = syn::UnOp::Not(Default::default());
        assert_eq!(common_unop_type(&bang, Type::Bool).unwrap(), Type::Bool);
        assert_eq!(common_unop_type(&bang, Type::I32).unwrap(), Type::I32);
        assert!(common_unop_type(&bang, Type::F32).is_err());
        assert!(common_unop_type(&bang, Type::Struct("S".into())).is_err());
    }

    #[test]
    fn neg_requires_numeric_operand() {
        let minus = syn::UnOp::Neg(Default::default());
        assert_eq!(common_unop_type(&minus, Type::F64).unwrap(), Type::F64);
        assert_eq!(common_unop_type(&minus, Type::Usize).unwrap(), Type::Usize);
        assert!(common_unop_type(&minus, Type::Bool).is_err());
        assert!(common_unop_type(&minus, Type::Pointer {
            element: Box::new(Type::U8), provenance: Provenance::Naked, is_mutable: false,
        }).is_err());
    }

    #[test]
    fn pointer_add_yields_pointer_operand_sub_rejected() {
        let add: syn::BinOp = syn::parse_str("+").unwrap();
        let sub: syn::BinOp = syn::parse_str("-").unwrap();
        assert_eq!(check(add, ptr_u8(), Type::I64).unwrap(), ptr_u8());
        assert_eq!(check(add, Type::I64, ptr_u8()).unwrap(), ptr_u8());
        // Sub-GEP is not implemented by the emitter: ptr - int must fail
        // closed here rather than binding a type emission cannot honor.
        assert!(check(sub, ptr_u8(), Type::I64).is_err());
        assert!(check(sub, ptr_u8(), Type::Usize).is_err());
    }

    #[test]
    fn wider_numeric_wins_symmetrically() {
        let add: syn::BinOp = syn::parse_str("+").unwrap();
        assert_eq!(check(add, Type::I32, Type::I64).unwrap(), Type::I64);
        assert_eq!(check(add, Type::I64, Type::I32).unwrap(), Type::I64);
    }

    #[test]
    fn usize_specials_match_emitter() {
        let add: syn::BinOp = syn::parse_str("+").unwrap();
        assert_eq!(check(add, Type::Usize, Type::I64).unwrap(), Type::I64);
        assert_eq!(check(add, Type::I64, Type::Usize).unwrap(), Type::I64);
        assert_eq!(check(add, Type::Usize, Type::U64).unwrap(), Type::U64);
        assert_eq!(check(add, Type::U64, Type::Usize).unwrap(), Type::U64);
    }

    #[test]
    fn int_float_mix_rejected() {
        let add: syn::BinOp = syn::parse_str("+").unwrap();
        assert!(check(add, Type::I64, Type::F64).is_err());
        assert!(check(add, Type::I32, Type::F32).is_err());
    }

    #[test]
    fn non_numeric_arithmetic_rejected() {
        let add: syn::BinOp = syn::parse_str("+").unwrap();
        assert!(check(add, Type::Bool, Type::Bool).is_err());
    }
}
