//! Payload-slot conformance for enum constructor arguments.
//!
//! Split out of `enum_ctor.rs` so the file respects the 500-line budget.

use std::collections::{BTreeMap, HashMap};

use super::{trace_locals_map, EnumVariantResolution};
use crate::codegen::context::{LocalKind, LoweringContext};
use crate::codegen::generic_resolver::generic_param_name;
use crate::codegen::tracer::TypeTracer;
use crate::grammar::{EnumDef, EnumVariant};
use crate::types::Type;

/// Payload-slot conformance: after generics are bound, every constructor
/// argument's traced type must be compatible with its substituted payload
/// slot. Identical types pass; numeric pairs pass only where emission casts
/// exist (see ctor_args_compatible); anything else is a hard error. This
/// closes the silent-punning hole where `Result::Ok("boom")` under
/// `-> Result<i64>` stored a string literal as i64 in the payload buffer.
pub(super) fn verify_ctor_arg_types(
    ctx: &mut LoweringContext,
    base_template: &str,
    variant: &EnumVariant,
    def: &EnumDef,
    final_generics: &[Type],
    call_args: &[syn::Expr],
    local_vars: &HashMap<String, (Type, LocalKind)>,
) -> Result<(), String> {
    let Some(generics) = &def.generics else { return Ok(()) };
    if generics.params.is_empty() || variant.tys.is_empty() {
        return Ok(());
    }
    let Some(map) = complete_slot_bindings(def, final_generics) else {
        return Ok(());  // incomplete binding: defer to emission diagnostics
    };
    let label = format!("{}::{}", base_template, variant.name);
    let trace_locals = trace_locals_map(local_vars);
    for (idx, (payload_ty, arg)) in variant.tys.iter().zip(call_args.iter()).enumerate() {
        let Some(raw_pat) = Type::from_syn(payload_ty) else { continue };
        let slot = ctx.substitute_generics(&raw_pat, &map);
        check_single_ctor_arg(ctx, &label, idx + 1, slot, arg, &trace_locals)?;
    }
    Ok(())
}

/// Parameter -> concrete-type bindings for the template. Present only when
/// EVERY declared parameter is bound: partial maps would poison downstream
/// specialization.
fn complete_slot_bindings(def: &EnumDef, final_generics: &[Type]) -> Option<BTreeMap<String, Type>> {
    let generics = def.generics.as_ref()?;
    let declared: Vec<String> = generics.params.iter().map(generic_param_name).collect();
    if declared.len() != final_generics.len() {
        return None;
    }
    Some(declared.into_iter().zip(final_generics.iter().cloned()).collect())
}

/// Post-strict-hit conformance, in parity with the template path above:
/// every constructor argument must conform to its payload slot even when the
/// enum was already specialized. Unit variants carry no payload and skip;
/// multi-field variants conform per tuple element exactly as emission stores
/// them. Untraceable and Fn-typed args defer to emission diagnostics.
pub(super) fn verify_specialized_ctor_args(
    ctx: &mut LoweringContext,
    res: &EnumVariantResolution,
    call_args: &[syn::Expr],
    local_vars: &HashMap<String, (Type, LocalKind)>,
) -> Result<(), String> {
    let Some(payload) = &res.payload_ty else { return Ok(()) };  // unit variant
    let slots = payload_slot_list(payload, call_args.len());
    let label = format!("{}::{}", res.enum_name, res.variant_name);
    let trace_locals = trace_locals_map(local_vars);
    for (idx, (slot, arg)) in slots.iter().zip(call_args.iter()).enumerate() {
        check_single_ctor_arg(ctx, &label, idx + 1, slot.clone(), arg, &trace_locals)?;
    }
    Ok(())
}

/// Emission stores multi-field variants as tuples: conform argument i against
/// tuple element i. Single-field variants conform against the payload itself.
fn payload_slot_list(payload: &Type, arg_count: usize) -> Vec<Type> {
    match (payload, arg_count > 1) {
        (Type::Tuple(tys), true) => tys.clone(),
        _ => vec![payload.clone()],
    }
}

/// One argument: trace it; untraceable args DEFER to emission diagnostics;
/// everything else requires structural or numeric compatibility with the slot.
/// A traced function item into a NON-fn slot is rejected here — emission
/// would otherwise silently ptrtoint the function's entry address into the
/// payload (e.g. `Val(answer)` filling an i64 slot with a code pointer).
fn check_single_ctor_arg(
    ctx: &LoweringContext,
    label: &str,
    arg_no: usize,
    slot: Type,
    arg: &syn::Expr,
    trace_locals: &BTreeMap<String, Type>,
) -> Result<(), String> {
    if matches!(slot, Type::Fn(..)) {
        return Ok(());  // fn-pointer slot: genuine fn items coerce at emission
    }
    let traced = match ctx.trace_expr_type(arg, trace_locals) {
        Ok(t) => t,
        Err(_) => return Ok(()),  // untraceable: emission reports what it sees
    };
    if matches!(traced, Type::Fn(..)) {
        return Err(format!(
            "Constructor argument type mismatch in {} argument {}: expected {:?}, found a function item (function names are not values; wrap the call: {}())",
            label, arg_no, slot,
            quote_arg_expr(arg)
        ));
    }
    let traced = auto_deref_for_slot(traced, &slot);
    if !ctor_args_compatible(&slot, &traced) {
        return Err(format!(
            "Constructor argument type mismatch in {} argument {}: expected {:?}, found {:?}",
            label, arg_no, slot, traced
        ));
    }
    Ok(())
}

/// Renders the offending argument expression back to source form for the
/// diagnostic (e.g. `answer()` in "wrap the call: answer()").
fn quote_arg_expr(arg: &syn::Expr) -> String {
    quote::quote!(#arg).to_string()
}

/// Slot/value compatibility, narrowed to what emission ACTUALLY coerces
/// (promote_numeric): structural identity always passes; numeric pairs pass
/// only when a cast exists — int->int widening/narrowing and int->float via
/// sitofp — while a float value into an integer slot is rejected up front
/// instead of failing late with 'Numeric promotion not supported'.
fn ctor_args_compatible(slot: &Type, traced: &Type) -> bool {
    slot.structural_eq(traced)
        || (slot.is_numeric()
            && traced.is_numeric()
            && !(traced.is_float() && slot.is_integer()))
}

/// Emission auto-derefs one reference layer into scalar slots
/// (promote_numeric), so `&local` into an i64 slot is not a false rejection.
fn auto_deref_for_slot(traced: Type, slot: &Type) -> Type {
    match traced {
        Type::Reference(inner, _)
            if !matches!(slot, Type::Reference(..) | Type::Pointer { .. }) => *inner,
        other => other,
    }
}
