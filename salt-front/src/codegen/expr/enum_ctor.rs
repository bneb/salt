//! Qualified enum-constructor resolution (`Enum::Variant(args)`).
//!
//! Split out of `utils.rs` so the file respects the 500-line budget.
//! Resolution order: strict registry -> template unification (turbofish,
//! expected type, constructor-argument inference).

use std::collections::{BTreeMap, HashMap};

use crate::codegen::context::{LocalKind, LoweringContext};
use crate::codegen::generic_resolver::{generic_param_name, normalize_generics, unify_types};
use crate::codegen::tracer::TypeTracer;
use crate::common::mangling::Mangler;
use crate::grammar::{EnumDef, EnumVariant, SynType};
use crate::types::Type;

#[derive(Debug, Clone)]
pub struct EnumVariantResolution {
    pub enum_name: String,
    pub variant_name: String,
    pub payload_ty: Option<Type>,
    pub discriminant: i32,
    pub generic_args: Vec<Type>,
}

pub fn resolve_path_to_enum(
    ctx: &mut LoweringContext,
    path_str: &str,
    generic_args: &[Type],
    expected_ty: Option<&Type>,
    call_args: &[syn::Expr],
    local_vars: &HashMap<String, (Type, LocalKind)>,
) -> Result<Option<EnumVariantResolution>, String> {
    let parts: Vec<&str> = path_str.split("__").collect();
    if parts.len() < 2 { return Ok(None); }

    let enum_name_candidate = Mangler::mangle(&parts[..parts.len()-1]);
    let Some(variant_name) = parts.last() else { return Ok(None) };

    // 1. Strict Registry: enum already exists in a concrete specialization.
    //    Payload conformance still applies here: before B2 this fast path
    //    bypassed every argument check, silently punning mistyped payloads.
    if let Some(res) = lookup_specialized_enum(ctx, &enum_name_candidate, variant_name) {
        verify_specialized_ctor_args(ctx, &res, call_args, local_vars)?;
        return Ok(Some(res));
    }

    // 2. Templates (Generic): unify generics from turbofish, the expected
    //    type, or - as a last resort - the constructor argument expressions.
    resolve_via_template(
        ctx, &enum_name_candidate, variant_name, generic_args, expected_ty, call_args, local_vars,
    )
}

/// Template-path resolution. Deliberate breaking change (sanctioned
/// pre-1.0): payload-blind variants (`Err(Status)` for `Result<T>`) with no
/// other generic binding are a clean resolution failure instead of silently
/// adopting an arbitrary prior specialization - annotate or use a
/// payload-typed variant so inference can bind every parameter.
fn resolve_via_template(
    ctx: &mut LoweringContext,
    enum_name_candidate: &str,
    variant_name: &str,
    generic_args: &[Type],
    expected_ty: Option<&Type>,
    call_args: &[syn::Expr],
    local_vars: &HashMap<String, (Type, LocalKind)>,
) -> Result<Option<EnumVariantResolution>, String> {
    let (base_template, def, target_variant) =
        match lookup_template_triple(ctx, enum_name_candidate, variant_name) {
            Some(triple) => triple,
            None => return Ok(None),
        };
    register_local_template(ctx, &base_template, &def);
    let mut final_generics = seed_and_unify_generics(ctx, &base_template, expected_ty, generic_args);
    if final_generics.is_empty() {
        final_generics = infer_ctor_generic_args(ctx, &def, &target_variant, call_args, local_vars);
    }

    if !final_generics.is_empty() {
        verify_ctor_arg_types(
            ctx,
            &base_template,
            &target_variant,
            &def,
            &final_generics,
            call_args,
            local_vars,
        )?;
    }

    Ok(specialize_template_variant(ctx, &base_template, &final_generics, variant_name))
}


/// Template + definition + target variant, or None when this candidate does
/// not denote a locally-known generic enum template.
fn lookup_template_triple(
    ctx: &LoweringContext,
    enum_name_candidate: &str,
    variant_name: &str,
) -> Option<(String, EnumDef, EnumVariant)> {
    if let Some(base) = ctx.find_enum_template_by_name(enum_name_candidate) {
        if let Some(def) = ctx.enum_templates().get(&base) {
            let variant = find_template_variant(def, variant_name)?.clone();
            return Some((base, def.clone(), variant));
        }
    }
    // Imported generic enums live in registry ModuleInfos keyed UNMANGLED or
    // under package names embedding the enum leaf; serve the triple directly
    // from the module definition.
    let registry = ctx.config.registry?;
    for module_info in registry.modules.values() {
        let pkg_mangled = module_info.package.replace('.', "__");
        for (name, def) in &module_info.enum_templates {
            let fqn = format!("{}__{}", pkg_mangled, name);
            let doubled = format!("{}__{}", fqn, name);
            if fqn == enum_name_candidate
                || pkg_mangled == enum_name_candidate
                || doubled == enum_name_candidate {
                let variant = find_template_variant(def, variant_name)?.clone();
                return Some((fqn, def.clone(), variant));
            }
        }
    }
    None
}

/// Expected types that structurally denote this template overwrite the
/// running generics; anything else leaves them untouched (legacy behavior).
fn apply_expected_unify(
    ctx: &LoweringContext,
    base_template: &str,
    expected_ty: Option<&Type>,
    final_generics: &mut Vec<Type>,
) {
    if let Some(exp) = expected_ty {
        unify_expected_type(ctx, base_template, exp, final_generics);
    }
}

/// Imported-template triples arrive from registry module defs; register the
/// base template locally so downstream specialization can find it.
fn register_local_template(ctx: &mut LoweringContext, base: &str, def: &EnumDef) {
    if !ctx.enum_templates().contains_key(base) {
        ctx.enum_templates_mut().insert(base.to_string(), def.clone());
    }
}

fn seed_and_unify_generics(
    ctx: &LoweringContext,
    base_template: &str,
    expected_ty: Option<&Type>,
    generic_args: &[Type],
) -> Vec<Type> {
    let mut generics = generic_args.to_vec();
    apply_expected_unify(ctx, base_template, expected_ty, &mut generics);
    generics
}

/// Payload-slot conformance: after generics are bound, every constructor
/// argument's traced type must be compatible with its substituted payload
/// slot. Identical types pass; numeric pairs pass only where emission casts
/// exist (see ctor_args_compatible); anything else is a hard error. This
/// closes the silent-punning hole where `Result::Ok("boom")` under
/// `-> Result<i64>` stored a string literal as i64 in the payload buffer.
fn verify_ctor_arg_types(
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
fn verify_specialized_ctor_args(
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

/// Registry fast path: find a variant on an already-concrete enum entry.
fn lookup_specialized_enum(
    ctx: &LoweringContext,
    enum_name: &str,
    variant_name: &str,
) -> Option<EnumVariantResolution> {
    let info = ctx.enum_registry().values().find(|i| i.name == enum_name)?;
    let (_, payload, disc) = info.variants.iter().find(|(n, _, _)| n == variant_name)?;
    Some(EnumVariantResolution {
        enum_name: enum_name.to_string(),
        variant_name: variant_name.to_string(),
        payload_ty: payload.clone(),
        discriminant: *disc,
        generic_args: vec![],
    })
}

/// Locate a variant by name inside an enum definition.
fn find_template_variant<'a>(def: &'a EnumDef, variant_name: &str) -> Option<&'a EnumVariant> {
    def.variants.iter().find(|v| v.name == variant_name)
}

/// Structural unification against the expected type.
/// On a structural match the expected type's args become the template args.
/// Returns false when the expected type can never denote an enum instance.
fn unify_expected_type(
    ctx: &LoweringContext,
    base_template: &str,
    expected: &Type,
    final_generics: &mut Vec<Type>,
) -> bool {
    let (exp_name, exp_args) = match expected {
        Type::Enum(name) => (name, vec![]),
        Type::Concrete(name, args) => (name, args.clone()),
        _ => return false,
    };

    // Structural Identity via Registry: check whether exp_name specializes
    // base_template instead of relying on raw string equality alone.
    let matches = if exp_name == base_template {
        true
    } else if let Some(rest) = exp_name.strip_prefix(base_template) {
        // Doubled-leaf form (pkg__Enum__Enum): return-position mangling
        // sometimes embeds the enum leaf twice; the structural prefix still
        // identifies this template.
        rest.starts_with("__")
    } else {
        ctx.enum_registry()
            .values()
            .find(|info| info.name == *exp_name)
            .and_then(|info| info.template_name.as_ref())
            .map(|template| template.as_str() == base_template)
            .unwrap_or(false)
    };

    if matches {
        *final_generics = exp_args;
    }
    true
}

/// Bind template generics from the constructor's own arguments:
/// each variant payload type pattern is unified with the traced type of
/// its argument expression (e.g. Opt::Some(5) binds T = i64).
fn infer_ctor_generic_args(
    ctx: &LoweringContext,
    def: &EnumDef,
    variant: &EnumVariant,
    call_args: &[syn::Expr],
    local_vars: &HashMap<String, (Type, LocalKind)>,
) -> Vec<Type> {
    let Some(generics) = &def.generics else { return vec![] };
    if generics.params.is_empty() || variant.tys.is_empty() {
        return vec![];
    }
    let declared: Vec<String> = generics.params.iter().map(generic_param_name).collect();
    let trace_locals = trace_locals_map(local_vars);
    let mut map: BTreeMap<String, Type> = BTreeMap::new();
    bind_ctor_args(ctx, &declared, variant, call_args, &trace_locals, &mut map);
    ordered_complete_generics(&declared, &map)
}

fn trace_locals_map(
    local_vars: &HashMap<String, (Type, LocalKind)>,
) -> BTreeMap<String, Type> {
    local_vars.iter()
        .map(|(k, (ty, _))| (k.clone(), ty.clone()))
        .collect()
}

fn bind_ctor_args(
    ctx: &LoweringContext,
    declared: &[String],
    variant: &EnumVariant,
    call_args: &[syn::Expr],
    trace_locals: &BTreeMap<String, Type>,
    map: &mut BTreeMap<String, Type>,
) {
    for (payload_ty, arg) in variant.tys.iter().zip(call_args.iter()) {
        if let Ok(concrete) = ctx.trace_expr_type(arg, trace_locals) {
            unify_payload_pattern(declared, payload_ty, &concrete, map);
        }
    }
}

/// Project the binding map onto the template's parameter order. Returns an
/// empty vector unless every declared generic is bound: partial guesses
/// would poison specialization downstream.
pub(crate) fn ordered_complete_generics(declared: &[String], map: &BTreeMap<String, Type>) -> Vec<Type> {
    if declared.is_empty() || declared.iter().any(|name| !map.contains_key(name)) {
        return vec![];
    }
    declared.iter().filter_map(|name| map.get(name).cloned()).collect()
}

pub(crate) fn unify_payload_pattern(
    declared: &[String],
    payload_ty: &SynType,
    concrete: &Type,
    map: &mut BTreeMap<String, Type>,
) {
    if let Some(raw_pat) = Type::from_syn(payload_ty) {
        let pat_ty = normalize_generics(&raw_pat, declared);
        let _ = unify_types(&pat_ty, concrete, map);
    }
}

/// Specialize the matched template and resolve the variant within it.
fn specialize_template_variant(
    ctx: &mut LoweringContext,
    base_template: &str,
    final_generics: &[Type],
    variant_name: &str,
) -> Option<EnumVariantResolution> {
    if final_generics.is_empty() {
        return None;
    }
    let substituted_generics: Vec<Type> = final_generics.to_vec();
    let specialized_name = ctx.specialize_template(base_template, &substituted_generics, true).ok()?.mangle();
    let info = ctx.enum_registry().values().find(|i| i.name == specialized_name)?;
    let (_, payload, disc) = info.variants.iter().find(|(n, _, _)| n == variant_name)?;
    Some(EnumVariantResolution {
        enum_name: base_template.to_string(),
        variant_name: variant_name.to_string(),
        payload_ty: payload.clone(),
        discriminant: *disc,
        generic_args: substituted_generics,
    })
}
