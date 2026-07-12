use crate::codegen::context::{LoweringContext, LocalKind};
use crate::codegen::expr::{emit_expr, emit_block_expr};
use crate::types::Type;
use std::collections::HashMap;

pub fn emit_while(ctx: &mut LoweringContext, out: &mut String, w: &syn::ExprWhile, local_vars: &mut HashMap<String, (Type, LocalKind)>) -> Result<(String, Type), String> {
    let loop_header = format!("while_header_{}", ctx.next_id());
    let loop_body = format!("while_body_{}", ctx.next_id());
    let loop_end = format!("while_end_{}", ctx.next_id());
    
    // Jump to header
    ctx.emit_br(out, &loop_header);
    
    // Header: Check condition
    ctx.emit_label(out, &loop_header);
    let (cond_val, cond_ty) = emit_expr(ctx, out, &w.cond, local_vars, Some(&Type::Bool))?;
    let cond_i1 = if cond_ty != Type::Bool {
        // We assume type checking/inference handles strict bools, 
        // or we trust the emit_expr hint. 
        // MLIR requires i1 for cond branch.
        cond_val
    } else {
        cond_val
    };
    
    ctx.emit_cond_br(out, &cond_i1, &loop_body, &loop_end);
    
    // Body
    ctx.emit_label(out, &loop_body);
    let _ = emit_block_expr(ctx, out, &w.body, local_vars, Some(&Type::Unit))?;
    ctx.emit_br(out, &loop_header);
    
    // End
    ctx.emit_label(out, &loop_end);
    
    // Check for infinite loop (while true) to return Never type
    let is_infinite = if let syn::Expr::Lit(l) = &*w.cond {
        if let syn::Lit::Bool(b) = &l.lit {
            b.value
        } else { false }
    } else { false };

    if is_infinite {
         Ok(("%unreachable".to_string(), Type::Never))
    } else {
         Ok(("".to_string(), Type::Unit))
    }
}
