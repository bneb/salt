use crate::types::Type;
use crate::codegen::context::{LoweringContext, LocalKind};
use crate::codegen::expr::emit_expr;
use std::collections::HashMap;


pub(crate) fn emit_ptr_write(
    ctx: &mut LoweringContext,
    out: &mut String,
    args: &[syn::Expr],
    local_vars: &mut HashMap<String, (Type, LocalKind)>,
) -> Result<Option<(String, Type)>, String> {
    if args.len() != 2 {
        return Err("Intrinsic 'ptr_write' expects 2 arguments: (ptr, value)".to_string());
    }
    let (mut ptr, mut ptr_ty) = emit_expr(ctx, out, &args[0], local_vars, None)?;
    
    if let Type::Reference(inner, _) = &ptr_ty {
        let load_val = format!("%loaded_ptr_{}", ctx.next_id());
        ctx.emit_load_logical(out, &load_val, &ptr, inner)?;
        ptr = load_val;
        ptr_ty = (**inner).clone();
    }
    
    let inner_ty = if let Type::Concrete(name, args) = &ptr_ty {
        if (name.ends_with("Ptr") || name.contains("Ptr")) && !args.is_empty() {
            args[0].clone()
        } else { return Err(format!("ptr_write expected Ptr<T>, got {:?}", ptr_ty)); }
    } else if let Type::Struct(name) = &ptr_ty {
        if name.ends_with("_u8") { Type::U8 }
        else if name.ends_with("_i64") { Type::I64 }
        else { return Err(format!("ptr_write expected Ptr<T>, got Struct {}", name)); }
    } else if let Type::Pointer { element, .. } = &ptr_ty {
        (**element).clone()
    } else {
        return Err("ptr_write expects a pointer type".to_string());
    };
    
    let (val, _) = emit_expr(ctx, out, &args[1], local_vars, Some(&inner_ty))?;
    
    let struct_ty = ptr_ty.to_mlir_storage_type(ctx)?;
    
    let is_aggregate = matches!(ptr_ty, Type::Struct(_) | Type::Concrete(_, _) | Type::Array(_, _, _));
    let loaded_ptr = if is_aggregate {
        let load_val = format!("%loaded_struct_{}", ctx.next_id());
        out.push_str(&format!("    {} = llvm.load {} : !llvm.ptr -> {}\n", load_val, ptr, struct_ty));
        load_val
    } else {
        ptr.clone()
    };
    
    let raw_ptr = if struct_ty == "!llvm.ptr" {
         loaded_ptr
    } else {
        let val_i64 = if struct_ty == "i64" {
            loaded_ptr
        } else {
            let val = format!("%ptr_val_w_{}", ctx.next_id());
            ctx.emit_extractvalue(out, &val, &loaded_ptr, 0, &struct_ty);
            val
        };
        
        let raw = format!("%raw_ptr_w_{}", ctx.next_id());
        ctx.emit_inttoptr(out, &raw, &val_i64, "i64");
        raw
    };
    
    ctx.emit_store_logical(out, &val, &raw_ptr, &inner_ty)?;
    Ok(Some(("%unit".to_string(), Type::Unit)))
}

pub(crate) fn emit_from_ref(
    ctx: &mut LoweringContext,
    out: &mut String,
    args: &[syn::Expr],
    local_vars: &mut HashMap<String, (Type, LocalKind)>,
    expected_ty: Option<&Type>,
) -> Result<Option<(String, Type)>, String> {
    if let Some(arg) = args.first() {
        let (val_var, _) = emit_expr(ctx, out, arg, local_vars, None)?;
        
        let addr_var = format!("%addr_{}", ctx.next_id());
        out.push_str(&format!("    {} = llvm.ptrtoint {} : !llvm.ptr to i64\n", addr_var, val_var));
        
        if let Some(expected) = expected_ty {
            let struct_ty = expected.to_mlir_storage_type(ctx)?;
            if struct_ty == "i64" {
                return Ok(Some((addr_var, expected.clone())));
            } else {
                let res = format!("%from_ref_{}", ctx.next_id());
                out.push_str(&format!("    {} = llvm.mlir.undef : {}\n", res, struct_ty));
                let res_final = format!("%res_final_{}", ctx.next_id());
                ctx.emit_insertvalue(out, &res_final, &addr_var, &res, 0, &struct_ty);
                return Ok(Some((res_final, expected.clone())));
            }
        }
    }
    Err("from_ref requires expected type context".to_string())
}


/// Element type behind a logical pointer value (`Ptr<T>` or legacy forms).
pub(crate) fn ptr_pointee_ty(ptr_ty: &Type, what: &str) -> Result<Type, String> {
    if let Type::Concrete(name, args) = ptr_ty {
        if (name.ends_with("Ptr") || name.contains("Ptr")) && !args.is_empty() {
            return Ok(args[0].clone());
        }
    } else if let Type::Struct(name) = ptr_ty {
        if name.ends_with("_u8") {
            return Ok(Type::U8);
        }
        if name.ends_with("_i64") {
            return Ok(Type::I64);
        }
        return Err(format!("{} expected Ptr<T>, got Struct {}", what, name));
    } else if let Type::Pointer { element, .. } = ptr_ty {
        return Ok((**element).clone());
    }
    Err(format!("{} expects a pointer type", what))
}

/// Coerce a logical pointer SSA value to a raw `!llvm.ptr`, loading
/// aggregate pointer structs and converting address-forms as needed.
pub(crate) fn coerce_to_raw_ptr(
    ctx: &mut LoweringContext,
    out: &mut String,
    ptr_val: &str,
    ptr_ty: &Type,
) -> Result<String, String> {
    let struct_ty = ptr_ty.to_mlir_storage_type(ctx)?;
    let is_aggregate = matches!(ptr_ty, Type::Struct(_) | Type::Concrete(_, _) | Type::Array(_, _, _));
    let loaded = if is_aggregate {
        let load_val = format!("%loaded_struct_{}", ctx.next_id());
        out.push_str(&format!("    {} = llvm.load {} : !llvm.ptr -> {}\n", load_val, ptr_val, struct_ty));
        load_val
    } else {
        ptr_val.to_string()
    };
    if struct_ty == "!llvm.ptr" {
        return Ok(loaded);
    }
    let addr = if struct_ty == "i64" {
        loaded
    } else {
        let val_i64 = format!("%ptr_val_{}", ctx.next_id());
        ctx.emit_extractvalue(out, &val_i64, &loaded, 0, &struct_ty);
        val_i64
    };
    let raw = format!("%raw_ptr_{}", ctx.next_id());
    ctx.emit_inttoptr(out, &raw, &addr, "i64");
    Ok(raw)
}

/// Indexed store: `ptr_write_at(ptr, index, value)` — the write-side
/// counterpart of `ptr_read_at`. GEPs to `ptr + index` (element-sized)
/// and stores `value` through the computed address.
pub(crate) fn emit_ptr_write_at(
    ctx: &mut LoweringContext,
    out: &mut String,
    args: &[syn::Expr],
    local_vars: &mut HashMap<String, (Type, LocalKind)>,
) -> Result<Option<(String, Type)>, String> {
    if args.len() != 3 {
        return Err("Intrinsic 'ptr_write_at' expects 3 arguments: (ptr, index, value)".to_string());
    }
    let (mut ptr, mut ptr_ty) = emit_expr(ctx, out, &args[0], local_vars, None)?;
    if let Type::Reference(inner, _) = &ptr_ty {
        let load_val = format!("%loaded_ptr_{}", ctx.next_id());
        ctx.emit_load_logical(out, &load_val, &ptr, inner)?;
        ptr = load_val;
        ptr_ty = (**inner).clone();
    }
    let inner_ty = ptr_pointee_ty(&ptr_ty, "ptr_write_at")?;
    let (index, _) = emit_expr(ctx, out, &args[1], local_vars, Some(&Type::I64))?;
    let (val, _) = emit_expr(ctx, out, &args[2], local_vars, Some(&inner_ty))?;
    let raw_ptr = coerce_to_raw_ptr(ctx, out, &ptr, &ptr_ty)?;
    let elem_mlir = inner_ty.to_mlir_type(ctx)?;
    let gep = format!("%ptr_write_at_gep_{}", ctx.next_id());
    ctx.emit_gep(out, &gep, &raw_ptr, &index, &elem_mlir);
    ctx.emit_store_logical(out, &val, &gep, &inner_ty)?;
    Ok(Some(("%unit".to_string(), Type::Unit)))
}