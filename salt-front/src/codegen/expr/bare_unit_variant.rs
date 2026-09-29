//! Bare unit-variant values: `return Empty`, not `return Beta::Empty`.
//!
//! The expected enum type decides which enum a bare name belongs to when that
//! enum declares it, as it does for bare payload-variant calls
//! (`resolve_unqualified_variant`). Otherwise the first enum, sorted by name,
//! that declares the name does, as before.

use crate::codegen::context::LoweringContext;
use crate::registry::EnumInfo;
use crate::types::Type;

/// Owning enum's registry name, discriminant, and whether it is a unit variant.
type VariantHit = (String, i32, bool);

/// A path that no global or constant claims, read as a unit variant named by
/// its first segment, qualified path or not, as before. Only a single-segment
/// path is anchored on the expected type: `Foo::Bar` must not become the
/// expected enum's `Foo`.
pub(crate) fn resolve_bare_unit_variant(
    ctx: &mut LoweringContext,
    out: &mut String,
    segments: &[String],
    mangled: &str,
    expected: Option<&Type>,
) -> Result<Option<(String, Type)>, String> {
    if ctx.globals().contains_key(mangled) || ctx.evaluator.constant_table.contains_key(mangled) {
        return Ok(None);
    }
    let Some(name) = segments.first().map(String::as_str) else {
        return Ok(None);
    };
    let anchor = expected.filter(|_| segments.len() == 1);
    let hit = expected_enum_variant(ctx, name, anchor).or_else(|| first_enum_variant(ctx, name));
    let Some((enum_name, disc, is_unit)) = hit else {
        return Ok(None);
    };
    if !is_unit {
        return Err(format!("Cannot use tuple variant '{}' as value without arguments", name));
    }
    emit_unit_variant(ctx, out, enum_name, disc).map(Some)
}

/// The variant `name` of the expected enum, looked up in the registry under
/// the expected type's exact spelling (`main__Beta_i32` for Beta<i32>).
fn expected_enum_variant(ctx: &LoweringContext, name: &str, expected: Option<&Type>) -> Option<VariantHit> {
    let key = match expected? {
        ty @ (Type::Enum(_) | Type::Concrete(..)) => ty.mangle_suffix(),
        _ => return None,
    };
    let info = ctx.enum_registry().values().find(|info| info.name == key)?;
    variant_of(info, name)
}

/// The first enum, sorted by name, that declares `name`.
fn first_enum_variant(ctx: &LoweringContext, name: &str) -> Option<VariantHit> {
    let mut sorted_enums: Vec<_> = ctx.enum_registry().values().collect();
    sorted_enums.sort_by_key(|e| &e.name);
    sorted_enums.into_iter().find_map(|info| variant_of(info, name))
}

fn variant_of(info: &EnumInfo, name: &str) -> Option<VariantHit> {
    let (_, payload, disc) = info.variants.iter().find(|(var_name, _, _)| var_name == name)?;
    Some((info.name.clone(), *disc, payload.is_none()))
}

/// The tag alone: a unit variant leaves the payload bytes undefined.
fn emit_unit_variant(
    ctx: &mut LoweringContext,
    out: &mut String,
    enum_name: String,
    disc: i32,
) -> Result<(String, Type), String> {
    let enum_ty = Type::Enum(enum_name);
    let mlir_ty = enum_ty.to_mlir_type(ctx)?;
    let res = format!("%enum_val_{}", ctx.next_id());
    let undef = format!("{}_undef", res);
    out.push_str(&format!("    {} = llvm.mlir.undef : {}\n", undef, mlir_ty));
    let tag_val = format!("{}_tag", res);
    ctx.emit_const_int(out, &tag_val, disc as i64, "i32");
    out.push_str(&format!("    {} = llvm.insertvalue {}, {}[0] : {}\n", res, tag_val, undef, mlir_ty));
    Ok((res, enum_ty))
}
