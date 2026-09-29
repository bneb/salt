//! Payload-slot conformance for enum constructor arguments: checked at
//! resolution, and at emission for the calls resolution defers.
//!
//! Split out of `enum_ctor.rs` so the file respects the 500-line budget.

use std::collections::{BTreeMap, HashMap};

use super::{find_unqualified_variant_enum, trace_locals_map, EnumVariantResolution};
use crate::codegen::context::{LocalKind, LoweringContext};
use crate::codegen::expr::emit_expr;
use crate::codegen::expr::literals::emit_enum_constructor;
use crate::codegen::expr::resolver::{CallKind, CallSiteResolver};
use crate::codegen::expr::utils::get_name_from_expr;
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

/// One argument: trace it; untraceable args and bare variant calls of the
/// slot's enum DEFER to emission (traced_or_deferred); everything else
/// requires structural or numeric compatibility with the slot.
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
    let Some(traced) = traced_or_deferred(ctx, arg, &slot, trace_locals) else {
        return Ok(());  // emission resolves the argument and reports what it sees
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

/// The argument's traced type, or None for one left to emission: an
/// untraceable argument, or a call defers_variant_call accepts.
fn traced_or_deferred(
    ctx: &LoweringContext,
    arg: &syn::Expr,
    slot: &Type,
    trace_locals: &BTreeMap<String, Type>,
) -> Option<Type> {
    let traced = ctx.trace_expr_type(arg, trace_locals).ok()?;
    if defers_variant_call(ctx, arg, slot, trace_locals, &traced) {
        return None;
    }
    Some(traced)
}

/// A bare call to a variant of the slot's enum (`Some(x)` in an Option slot)
/// that the tracer typed as Unit, as it types a callee it can't resolve.
/// Emission passes the slot as the call's expected type and builds it with
/// emit_deferred_variant, which checks what resolution picked. A function the
/// tracer resolves keeps its return type, since it claims the call at
/// emission too; one returning unit is deferred, and rejected there.
fn defers_variant_call(
    ctx: &LoweringContext,
    arg: &syn::Expr,
    slot: &Type,
    trace_locals: &BTreeMap<String, Type>,
    traced: &Type,
) -> bool {
    *traced == Type::Unit && is_bare_variant_call(ctx, arg, slot, trace_locals)
}

/// A call to a bare name, without type arguments, that the slot's enum
/// declares as a variant. A name bound to an fn-pointer local doesn't count:
/// emission calls through the local.
fn is_bare_variant_call(
    ctx: &LoweringContext,
    arg: &syn::Expr,
    slot: &Type,
    trace_locals: &BTreeMap<String, Type>,
) -> bool {
    let syn::Expr::Call(call) = arg else { return false };
    let Some(name) = bare_callee(&call.func) else { return false };
    !matches!(trace_locals.get(&name), Some(Type::Fn(..)))
        && find_unqualified_variant_enum(ctx, &name, Some(slot)).is_some()
}

/// The callee's name when it is a single identifier with no type arguments.
fn bare_callee(func: &syn::Expr) -> Option<String> {
    let name = get_name_from_expr(func)?;
    let syn::Expr::Path(p) = func else { return None };
    matches!(p.path.segments[0].arguments, syn::PathArguments::None).then_some(name)
}

/// Emits one payload argument against its slot's type. A call the payload
/// check deferred (defers_variant_call) is built by emit_deferred_variant;
/// any other argument emits as before.
pub(crate) fn emit_payload_arg(
    ctx: &mut LoweringContext,
    out: &mut String,
    arg: &syn::Expr,
    slot: &Type,
    local_vars: &mut HashMap<String, (Type, LocalKind)>,
) -> Result<(String, Type), String> {
    let trace_locals = trace_locals_map(local_vars);
    let deferred = match ctx.trace_expr_type(arg, &trace_locals) {
        Ok(traced) => defers_variant_call(ctx, arg, slot, &trace_locals, &traced),
        Err(_) => false,
    };
    match arg {
        syn::Expr::Call(call) if deferred => emit_deferred_variant(ctx, out, arg, call, slot, local_vars),
        _ => emit_expr(ctx, out, arg, local_vars, Some(slot)),
    }
}

/// Builds a deferred call only as the variant it names. Resolution can hand
/// the name to something the tracer doesn't see (an intrinsic, a struct, a
/// function in any loaded module), and storing that value would get past
/// promote_numeric, which passes an enum with other type arguments by leaf
/// name. So resolution must pick a variant whose MLIR type is the slot's, and
/// the call must pass one argument per payload field.
fn emit_deferred_variant(
    ctx: &mut LoweringContext,
    out: &mut String,
    arg: &syn::Expr,
    call: &syn::ExprCall,
    slot: &Type,
    local_vars: &mut HashMap<String, (Type, LocalKind)>,
) -> Result<(String, Type), String> {
    let res = match CallSiteResolver::new(ctx).resolve_call(call, local_vars, Some(slot))? {
        CallKind::EnumConstructor(res) => res,
        other => return Err(resolves_elsewhere(arg, slot, &describe_call(&other))),
    };
    let built = built_type(&res);
    if built.to_mlir_type(ctx)? != slot.to_mlir_type(ctx)? {
        let found = format!("{}::{} of type {:?}", res.enum_name, res.variant_name, built);
        return Err(resolves_elsewhere(arg, slot, &found));
    }
    check_variant_arity(arg, call, &res)?;
    let args: Vec<syn::Expr> = call.args.iter().cloned().collect();
    emit_enum_constructor(ctx, out, res, &args, local_vars)
}

/// The type emit_enum_constructor gives the value it builds for `res`.
fn built_type(res: &EnumVariantResolution) -> Type {
    if res.generic_args.is_empty() {
        return Type::Enum(res.enum_name.clone());
    }
    Type::Concrete(res.enum_name.clone(), res.generic_args.clone())
}

/// Emission builds one payload field per argument (a tuple payload from
/// several), so any other count drops arguments or emits invalid MLIR. A
/// tuple payload counts its elements.
fn check_variant_arity(arg: &syn::Expr, call: &syn::ExprCall, res: &EnumVariantResolution) -> Result<(), String> {
    let fields = match &res.payload_ty {
        None => 0,
        Some(Type::Tuple(tys)) => tys.len(),
        Some(_) => 1,
    };
    if call.args.len() == fields {
        return Ok(());
    }
    Err(format!(
        "Constructor argument count mismatch: `{}` passes {} argument(s), but {}::{} takes {}",
        quote_arg_expr(arg), call.args.len(), res.enum_name, res.variant_name, fields
    ))
}

fn describe_call(kind: &CallKind) -> String {
    match kind {
        CallKind::Function(name, ret, ..) => format!("function {} returning {:?}", name, ret),
        CallKind::Intrinsic(name, _) => format!("intrinsic {}", name),
        CallKind::StructLiteral(name, _) => format!("struct {}", name),
        CallKind::TransparentVecAccess { method, .. } => format!("vector access {}", method),
        CallKind::EnumConstructor(res) => format!("{}::{}", res.enum_name, res.variant_name),
    }
}

fn resolves_elsewhere(arg: &syn::Expr, slot: &Type, found: &str) -> String {
    format!(
        "Constructor argument type mismatch: `{}` in a {:?} payload slot resolves to {}, not a variant of that enum",
        quote_arg_expr(arg), slot, found
    )
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
