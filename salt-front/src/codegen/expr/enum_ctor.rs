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

mod payload;
pub(crate) use payload::emit_payload_arg;
use payload::{verify_ctor_arg_types, verify_specialized_ctor_args};

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
    resolve_enum_variant(
        ctx, &enum_name_candidate, variant_name, generic_args, expected_ty, call_args, local_vars,
    )
}

/// A bare variant call (`Some(v)`, not `Option::Some(v)`), tried only once no
/// function or struct claimed the name. With no `Enum::` prefix the enum comes
/// from the expected type; without one the call is ambiguous and unresolved.
pub fn resolve_unqualified_variant(
    ctx: &mut LoweringContext,
    variant_name: &str,
    generic_args: &[Type],
    expected_ty: Option<&Type>,
    call_args: &[syn::Expr],
    local_vars: &HashMap<String, (Type, LocalKind)>,
) -> Result<Option<EnumVariantResolution>, String> {
    let Some(enum_name_candidate) = find_unqualified_variant_enum(ctx, variant_name, expected_ty) else {
        return Ok(None);
    };
    resolve_enum_variant(
        ctx, &enum_name_candidate, variant_name, generic_args, expected_ty, call_args, local_vars,
    )
}

/// Strict registry hit, else generic-template unification: the steps every
/// (enum, variant) pair goes through, qualified or recovered from a bare name.
fn resolve_enum_variant(
    ctx: &mut LoweringContext,
    enum_name_candidate: &str,
    variant_name: &str,
    generic_args: &[Type],
    expected_ty: Option<&Type>,
    call_args: &[syn::Expr],
    local_vars: &HashMap<String, (Type, LocalKind)>,
) -> Result<Option<EnumVariantResolution>, String> {
    // 1. Strict Registry: enum already exists in a concrete specialization.
    //    Payload conformance still applies here: before B2 this fast path
    //    bypassed every argument check, silently punning mistyped payloads.
    if let Some(res) = lookup_specialized_enum(ctx, enum_name_candidate, variant_name) {
        verify_specialized_ctor_args(ctx, &res, call_args, local_vars)?;
        return Ok(Some(res));
    }

    // 2. Templates (Generic): unify generics from turbofish, the expected
    //    type, or - as a last resort - the constructor argument expressions.
    resolve_via_template(
        ctx, enum_name_candidate, variant_name, generic_args, expected_ty, call_args, local_vars,
    )
}

/// The expected enum type's name, if it declares `variant_name`: a specialized
/// registry entry, else a template reachable by name.
fn find_unqualified_variant_enum(
    ctx: &LoweringContext,
    variant_name: &str,
    expected_ty: Option<&Type>,
) -> Option<String> {
    let expected_name = match expected_ty {
        Some(Type::Enum(name)) => name.as_str(),
        Some(Type::Concrete(name, _)) => name.as_str(),
        _ => return None,
    };
    if let Some(info) = ctx.enum_registry().values().find(|info| info.name == expected_name) {
        return info.variants.iter().any(|(v, _, _)| v == variant_name)
            .then(|| expected_name.to_string());
    }
    lookup_template_triple(ctx, expected_name, variant_name).map(|(base, _, _)| base)
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
