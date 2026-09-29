//! Where a function body may end. A function returning a value must not
//! reach the end of its body (emit_fn_cleanup rejects it): Salt returns with
//! `return`, never with a block's final expression. A path may still end
//! without one when control cannot continue past it: a call to C's `exit` or
//! `abort`, a `loop` or `while true` with no `break` out of it.

use crate::codegen::collector::MonomorphizationTask;
use crate::codegen::context::{LoweringContext, LocalKind};
use crate::codegen::expr::emit_expr;
use crate::grammar::{SaltFn, Stmt};
use crate::types::Type;
use std::collections::HashMap;

/// C library functions that never return.
const NORETURN_CALLS: [&str; 2] = ["exit", "abort"];

/// Emits an expression statement; returns true when control cannot
/// continue past it: it diverges, or it is a call to C's `exit` or `abort`.
pub(crate) fn emit_expr_stmt(
    ctx: &mut LoweringContext,
    out: &mut String,
    expr: &syn::Expr,
    local_vars: &mut HashMap<String, (Type, LocalKind)>,
) -> Result<bool, String> {
    ctx.emission.last_call_never_returns = false;
    let (val, _) = emit_expr(ctx, out, expr, local_vars, None)?;
    if val == "%unreachable" {
        return Ok(true);
    }
    // The call's own resolution is recorded last, after its arguments'.
    if !(calls_exit_or_abort(expr) && ctx.emission.last_call_never_returns) {
        return Ok(false);
    }
    out.push_str("    llvm.unreachable\n");
    Ok(true)
}

/// Whether a call resolved to `name`, with `task` holding the callee's
/// definition, is C's `exit` or `abort`: an `extern fn`, which the resolver
/// hands over as a template with no body. A Salt function of either name has
/// a body, or a package-mangled name, and returns like any other.
pub(crate) fn is_c_noreturn(name: &str, task: &Option<Box<MonomorphizationTask>>) -> bool {
    NORETURN_CALLS.contains(&name) && task.as_ref().is_none_or(|t| t.func.body.stmts.is_empty())
}

/// Whether `expr` is a call spelled `exit(..)` or `abort(..)`. Such a call
/// may end its block, so to block_has_control_flow a loop body making one
/// cannot be the single block of an scf.for region.
pub(crate) fn calls_exit_or_abort(expr: &syn::Expr) -> bool {
    let syn::Expr::Call(call) = expr else { return false };
    let syn::Expr::Path(callee) = &*call.func else { return false };
    callee.path.get_ident().is_some_and(|name| NORETURN_CALLS.iter().any(|n| name == n))
}

/// Opens a while loop's exit block; returns whether the loop ends its
/// block. Without a `break`, `while true` only reaches the exit block
/// through its condition's false edge, which is never taken.
pub(crate) fn emit_while_exit(out: &mut String, cond: &syn::Expr, label_exit: &str) -> bool {
    let infinite = is_true_literal(cond) && !breaks_to(out, label_exit);
    out.push_str(&format!("  ^{}:\n", label_exit));
    if infinite {
        out.push_str("    llvm.unreachable\n");
    }
    infinite
}

/// Whether `out` holds a `break` to `label`. Matches the whole label: a
/// nested loop's label can extend it (`loop_exit_7`, `loop_exit_75`).
pub(crate) fn breaks_to(out: &str, label: &str) -> bool {
    out.contains(&format!("cf.br ^{}\n", label))
}

fn is_true_literal(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Paren(p) => is_true_literal(&p.expr),
        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Bool(b), .. }) => b.value,
        _ => false,
    }
}

/// For a function returning a value whose body can reach its end.
pub(crate) fn missing_return_error(fn_name: &str) -> String {
    format!(
        "missing return in function '{}': control can reach the end of its body, and Salt does not \
         return a block's final expression. End every path with `return <value>;`",
        fn_name
    )
}

/// `main` returns 0 when control reaches its end, so a final expression
/// with no `;` would be dropped rather than returned.
pub(crate) fn check_main_final_expr(func: &SaltFn) -> Result<(), String> {
    let Some(Stmt::Expr(_, false)) = func.body.stmts.last() else { return Ok(()) };
    Err(format!(
        "missing return in function '{}': its final expression has no `;`, so it would be dropped \
         and 0 returned; Salt does not return a block's final expression. Write `return <value>;`",
        func.name
    ))
}
