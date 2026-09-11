use crate::grammar::{Stmt};
use crate::types::Type;
use crate::codegen::context::{LoweringContext, LocalKind};
use crate::codegen::expr::emit_expr;
use std::collections::HashMap;
use syn::spanned::Spanned;

/// Phase A: Prove loop invariants hold at entry (base case).
pub(crate) fn prove_while_loop_base_case(
    ctx: &mut LoweringContext,
    stmts: &[Stmt],
    bv: &HashMap<String, (Type, LocalKind)>,
) -> Result<Vec<syn::Expr>, String> {
    if ctx.config.no_verify { return Ok(vec![]); }
    let sc = crate::codegen::verification::SymbolicContext::new(ctx.z3_ctx);
    let mut inv: Vec<syn::Expr> = Vec::new();
    for s in stmts { if let Stmt::Invariant(e) = s { inv.push(e.clone()); } }
    // Check each invariant in an isolated sub-frame, then assert in parent context.
    // No outer push — surrounding constraints (loop bounds, let bindings) are visible.
    for e in &inv {
        if let Ok(z) = crate::codegen::expr::translate_bool_to_z3(ctx, e, bv, &sc) {
            ctx.z3_solver.push(); ctx.z3_solver.assert(&z.not());
            let ck = ctx.z3_solver.check(); ctx.z3_solver.pop(1);
            if ck == crate::z3_shim::SatResult::Sat {
                return Err(crate::errors::coded(
                    "E009",
                    "Z3 verification failed: loop invariant does not hold at entry.                      The solver found a counterexample proving the invariant is false                      with current variable values."
                ));
            }
            ctx.z3_solver.assert(&z);
        }
    }
    Ok(inv)
}

/// Phase B: Inductive step for while loop verification.
///
/// Returns the `{original_name: havoc_name}` map for every variable this
/// call havoc'd, so `verify_while_loop_post_body` can anchor the post-loop
/// fact to THIS loop's specific, permanent havoc symbols rather than the
/// bare (reusable) source name -- see that function's doc comment for why
/// that distinction is load-bearing, not just tidiness.
pub(crate) fn setup_while_loop_inductive_step(
    ctx: &mut LoweringContext,
    stmts: &[Stmt],
    bv: &mut HashMap<String, (Type, LocalKind)>,
    cond: &syn::Expr,
    inv: &[syn::Expr],
) -> Result<HashMap<String, String>, String> {
    if ctx.config.no_verify { return Ok(HashMap::new()); }
    let sc = crate::codegen::verification::SymbolicContext::new(ctx.z3_ctx);
    ctx.z3_solver.push();
    let mut havoc_names: HashMap<String, String> = HashMap::new();
    for n in &crate::codegen::stmt::helpers::collect_mutations(stmts) {
        if let Some((ty, _)) = bv.get(n) {
            if ty.is_integer() {
                let f = format!("{}_havoc_{}", n, ctx.next_id());
                ctx.symbolic_tracker.insert(n.clone(), ctx.mk_var(&f));
                havoc_names.insert(n.clone(), f);
            }
        }
    }
    for e in inv {
        if let Ok(z) = crate::codegen::expr::translate_bool_to_z3(ctx, e, bv, &sc) {
            ctx.z3_solver.assert(&z);
        }
    }
    if let Ok(z) = crate::codegen::expr::translate_bool_to_z3(ctx, cond, bv, &sc) {
        ctx.z3_solver.assert(&z);
    }
    Ok(havoc_names)
}

/// Scans a while loop's body for statement-position calls to a function
/// with a `requires` clause (`need_positive(y);` -- NOT `let x = f(y);` or
/// a method call like `buf.set(off, v)`; both are out of scope for now,
/// see the module-level note on this Houdini-lite mechanism) and returns
/// each requires clause rewritten into the loop's own variable names (the
/// callee's parameters replaced by the actual argument expressions at
/// that call site) as a candidate loop invariant. Callers must still test
/// each candidate against the base case before trusting it -- this only
/// proposes, it doesn't verify.
///
/// Method calls are excluded because resolving one to the right `requires`
/// clause needs the receiver's type to pick the right `impl` block, which
/// this pass -- run before the body is otherwise processed -- doesn't have
/// available. It's also lower value: a method call's bounds-shaped
/// requires (like `Slice::set`'s) is usually already covered by
/// `loop_assumptions` matching the guard directly (test_slice_cursor_proved
/// needs no help from this).
fn collect_call_requires_candidates(ctx: &LoweringContext, stmts: &[Stmt]) -> Vec<syn::Expr> {
    let mut candidates = Vec::new();
    for stmt in stmts {
        // A bare call statement can surface as either grammar shape,
        // depending on which parse path produced it (grammar::Stmt::Expr
        // when the statement parser handled it directly, Stmt::Syn(syn::
        // Stmt::Expr(..)) when it fell through to syn's own parser) --
        // confirmed by inspecting actual parse output, not assumed.
        let call = match stmt {
            Stmt::Expr(syn::Expr::Call(c), _) => c,
            Stmt::Syn(syn::Stmt::Expr(syn::Expr::Call(c), _)) => c,
            _ => continue,
        };
        let syn::Expr::Path(p) = call.func.as_ref() else { continue };
        let Some(fn_name) = p.path.get_ident().map(|i| i.to_string()) else { continue };
        let found = ctx.config.file.items.iter().find_map(|item| {
            if let crate::grammar::Item::Fn(f) = item {
                if f.name == fn_name {
                    let params: Vec<String> = f.args.iter().map(|a| a.name.to_string()).collect();
                    return Some((f.requires.clone(), params));
                }
            }
            None
        });
        let Some((requires, params)) = found else { continue };
        let arg_exprs: Vec<syn::Expr> = call.args.iter().cloned().collect();
        for req in &requires {
            let Some(actual_req) = crate::codegen::verification::unwrap_contract_expr(req) else { continue };
            candidates.push(crate::codegen::verification::substitute_params_with_args(actual_req, &params, &arg_exprs));
        }
    }
    candidates
}

/// Keeps only the candidates (from `collect_call_requires_candidates`)
/// that hold at the loop's base case, permanently asserting each survivor
/// before testing the next -- the same accumulation
/// `prove_while_loop_base_case` already relies on for explicit invariants,
/// so a later candidate can lean on an earlier one already having been
/// established. Silently drops a candidate that doesn't hold rather than
/// failing the compile: unlike a user-written `invariant`, nothing
/// promised this one holds, so a wrong guess is a missed optimization,
/// not an error -- the call it came from still gets checked for real,
/// with its actual concrete arguments, by the normal requires-check path
/// during body emission; this pass only ever adds information, and only
/// after confirming it's true, so it cannot turn that later check from a
/// correct rejection into a wrongly-accepted one.
fn filter_call_requires_candidates(
    ctx: &mut LoweringContext,
    candidates: Vec<syn::Expr>,
    bv: &HashMap<String, (Type, LocalKind)>,
) -> Vec<syn::Expr> {
    if ctx.config.no_verify { return vec![]; }
    let sc = crate::codegen::verification::SymbolicContext::new(ctx.z3_ctx);
    let mut survivors = Vec::new();
    for c in candidates {
        let Ok(z) = crate::codegen::expr::translate_bool_to_z3(ctx, &c, bv, &sc) else { continue };
        ctx.z3_solver.push();
        ctx.z3_solver.assert(&z.not());
        let holds = ctx.z3_solver.check() == crate::z3_shim::SatResult::Unsat;
        ctx.z3_solver.pop(1);
        if holds {
            ctx.z3_solver.assert(&z);
            survivors.push(c);
        }
    }
    survivors
}

/// Try to auto-infer a loop invariant for simple monotonic while loops.
///
/// Supported patterns:
///   `while var < N { var = var + 1 }` → invariant `var >= 0 && var < N`
///   `while var <= N { var = var + 1 }` → invariant `var >= 0 && var <= N`
///   `while cond1 && cond2 { ... }` → tries each sub-condition independently
fn try_infer_while_invariant(
    cond: &syn::Expr,
    body: &[Stmt],
    local_vars: &HashMap<String, (Type, LocalKind)>,
) -> Option<syn::Expr> {
    // 0. Unwrap && conditions: try each side independently.
    if let syn::Expr::Binary(syn::ExprBinary {
        op: syn::BinOp::And(_), left, right, ..
    }) = cond
    {
        if let Some(inv) = try_infer_while_invariant(left, body, local_vars) {
            return Some(inv);
        }
        return try_infer_while_invariant(right, body, local_vars);
    }

    // 1. Parse condition: must be `var < N` or `var <= N` where N is a constant.
    let (var_name, bound, inclusive) = extract_monotonic_bound(cond)?;
    // 2. Find the loop variable's initial value from local_vars.
    // It must have been initialized to a compile-time constant before the loop.
    let (_, kind) = local_vars.get(&var_name)?;
    // Verify the variable has an SSA tracking entry (exists in local_vars).
    if !matches!(kind, LocalKind::SSA(_)) { return None; }
    // 3. Verify the body contains a simple increment: `var = var + K` (K > 0).
    if !has_monotonic_increment(body, &var_name) { return None; }
    // 4. Try to get the initial constant value. We can't easily extract it
    // from Z3 state at this point, so use 0 as the default lower bound.
    // The base case check will reject the invariant if it doesn't hold.
    let lower = 0i64;
    // Synthesize: `invariant var >= lower && var < bound` or `var <= bound`
    if inclusive {
        let inv_str = format!("{} >= {} && {} <= {}", var_name, lower, var_name, bound);
        syn::parse_str(&inv_str).ok()
    } else {
        let inv_str = format!("{} >= {} && {} < {}", var_name, lower, var_name, bound);
        syn::parse_str(&inv_str).ok()
    }
}

/// Extract `(var_name, bound, inclusive)` from a condition like `i < 5` or `i <= n`.
fn extract_monotonic_bound(cond: &syn::Expr) -> Option<(String, i64, bool)> {
    if let syn::Expr::Binary(b) = cond {
        let (var_name, bound, inclusive) = match &b.op {
            syn::BinOp::Lt(_) => {
                let var = extract_var_name(&b.left)?;
                let n = extract_const_i64(&b.right)?;
                (var, n, false)
            }
            syn::BinOp::Le(_) => {
                let var = extract_var_name(&b.left)?;
                let n = extract_const_i64(&b.right)?;
                (var, n, true)
            }
            _ => return None,
        };
        Some((var_name, bound, inclusive))
    } else {
        None
    }
}

/// Extract a variable name from an expression like `i`.
fn extract_var_name(expr: &syn::Expr) -> Option<String> {
    if let syn::Expr::Path(p) = expr {
        p.path.get_ident().map(|id| id.to_string())
    } else { None }
}

/// Extract a compile-time integer constant from an expression like `5`.
fn extract_const_i64(expr: &syn::Expr) -> Option<i64> {
    if let syn::Expr::Lit(lit) = expr {
        if let syn::Lit::Int(i) = &lit.lit {
            i.base10_parse::<i64>().ok()
        } else { None }
    } else { None }
}

/// Check if the loop body monotonically increments `var` by a positive constant.
fn has_monotonic_increment(body: &[Stmt], var_name: &str) -> bool {
    for stmt in body {
        if let Stmt::Syn(s) = stmt {
            let expr = match s {
                syn::Stmt::Expr(e, _) => e,
                _ => continue,
            };
            if let syn::Expr::Binary(syn::ExprBinary {
                ref left, op: syn::BinOp::AddAssign(_), ref right, ..
            }) = expr
            {
                if extract_var_name(left) == Some(var_name.to_string())
                    && extract_const_i64(right).is_some_and(|n| n > 0)
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Renames every bare identifier matching a key in `havoc_names` to that
/// key's havoc'd name (`off` -> `off_havoc_12`). Leaves everything else --
/// other identifiers, field access, calls -- untouched.
///
/// Why this exists: `scoped_facts` entries are raw `syn::Expr`, re-resolved
/// by NAME at whatever point they're later consulted. `off` is an ordinary,
/// reusable source name -- a second while loop reusing it (or a plain
/// `let mut off = ...` shadow) overwrites `symbolic_tracker["off"]` with a
/// NEW havoc symbol, and an unrenamed post-loop fact from the FIRST loop
/// would then resolve against that unrelated second symbol instead of the
/// one it was actually true about. Two sequential loops both touching
/// `off` produced exactly this: a stale `off <= 3` and a live `off >= 100`
/// both resolving to the same symbol, a direct contradiction, and every
/// check after it vacuously "proving" -- caught by test_tier3 empirically
/// before this rename was added, not by inspection. Anchoring to the
/// havoc'd name instead sidesteps it the same way Tier 2's fresh
/// `callres_{id}` identifiers sidestepped the analogous risk for call
/// results: once a fact names a globally-unique symbol instead of a
/// reusable one, nothing can ever redefine out from under it.
struct HavocAnchor<'m> {
    havoc_names: &'m HashMap<String, String>,
}

impl syn::visit_mut::VisitMut for HavocAnchor<'_> {
    fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
        if let syn::Expr::Path(p) = expr {
            if let Some(ident) = p.path.get_ident() {
                if let Some(havoc_name) = self.havoc_names.get(&ident.to_string()) {
                    let new_ident = syn::Ident::new(havoc_name, ident.span());
                    *expr = syn::parse_quote!(#new_ident);
                    return;
                }
            }
        }
        syn::visit_mut::visit_expr_mut(self, expr);
    }
}

/// Phase C: Pop inductive scope and make the standard Hoare post-loop fact
/// -- `invariant && !cond` -- available to code after the loop.
///
/// Sound, not a judgment call, but the argument has two different legs for
/// the two conjuncts, and it's worth being precise about which is which.
/// `!cond` is why the loop exited -- unconditionally true, no caveat.
/// `invariant` is NOT independently proven true for every iteration at
/// compile time: only the BASE CASE is (prove_while_loop_base_case, a hard
/// E009 if it fails). Maintenance across iterations is enforced at RUNTIME
/// instead -- the `invariant` statement inside the loop body compiles to a
/// check on every iteration that calls the `noreturn`-attributed
/// `__salt_contract_violation` (see context.rs's cold+noreturn passthrough
/// for that symbol) if it's ever false. `noreturn` is load-bearing for this
/// argument: LLVM assumes that call never returns, so control can only
/// reach code after the loop if the invariant held on every iteration that
/// ran. This is the SAME pattern emit_requires_runtime_check and
/// emit_ensures_runtime_check already rely on elsewhere in this codebase
/// (a runtime check the compiler can't discharge at compile time, trusted
/// downstream because failing it doesn't fall through) -- not a new kind of
/// trust this fix introduces, applied to a place it wasn't reaching before.
///
/// This is what was missing, not the per-call-site leniency the reverted
/// patch tried (see SPEC.md's havoc entry): `off`'s tracked value is
/// deliberately destroyed on loop entry (havoc'd in
/// setup_while_loop_inductive_step, named `{var}_havoc_{id}`) so the
/// inductive step reasons about an arbitrary iteration, and NOTHING
/// previously re-established anything about it once the loop exited --
/// not even the loop's own invariant. `off` stayed permanently,
/// unconditionally free for the rest of the function (and, since
/// symbolic_tracker isn't reset per function, potentially for functions
/// compiled after it too) unless the caller happened to re-guard
/// immediately before every later use. Pushed onto
/// ctx.emission.scoped_facts (not ctx.z3_solver alone) because
/// requires/ensures checks build a fresh Solver per call site and never
/// read ctx.z3_solver -- Tier 1 and Tier 2's fixes both had to work around
/// the same thing. ctx.z3_solver still gets it too, for nested loops'
/// own base-case/inductive-step reasoning, which reads it directly.
pub(crate) fn verify_while_loop_post_body(
    ctx: &mut LoweringContext,
    cond: &syn::Expr,
    inv: &[syn::Expr],
    havoc_names: &HashMap<String, String>,
    lv: &HashMap<String, (Type, LocalKind)>,
) {
    if ctx.config.no_verify { return; }
    let sc = crate::codegen::verification::SymbolicContext::new(ctx.z3_ctx);
    ctx.z3_solver.pop(1);
    if let Ok(z) = crate::codegen::expr::translate_bool_to_z3(ctx, cond, lv, &sc) {
        ctx.z3_solver.assert(&z.not());
    }
    let mut anchor = HavocAnchor { havoc_names };

    let mut negated_cond = syn::Expr::Unary(syn::ExprUnary {
        attrs: vec![],
        op: syn::UnOp::Not(syn::token::Not::default()),
        expr: Box::new(cond.clone()),
    });
    syn::visit_mut::VisitMut::visit_expr_mut(&mut anchor, &mut negated_cond);
    ctx.emission.scoped_facts.push(negated_cond);

    for e in inv {
        if let Ok(z) = crate::codegen::expr::translate_bool_to_z3(ctx, e, lv, &sc) {
            ctx.z3_solver.assert(&z);
        }
        let mut anchored = e.clone();
        syn::visit_mut::VisitMut::visit_expr_mut(&mut anchor, &mut anchored);
        ctx.emission.scoped_facts.push(anchored);
    }
}

pub(crate) fn emit_while_stmt(ctx: &mut LoweringContext, out: &mut String, w: &crate::grammar::SaltWhile, local_vars: &mut HashMap<String, (Type, LocalKind)>) -> Result<bool, String>  {
            let label_header = format!("while_header_{}", ctx.next_id());
            let label_body = format!("while_body_{}", ctx.next_id());
            let label_exit = format!("while_exit_{}", ctx.next_id());

            out.push_str(&format!("    cf.br ^{}\n", label_header));
            out.push_str(&format!("  ^{}:\n", label_header));

            let (cond_val, cond_ty) = emit_expr(ctx, out, &w.cond, local_vars, None)?;
            // Accept Pointer types as while conditions
            let cond_val = if cond_ty.k_is_ptr_type() {
                let id = ctx.next_id();
                let int_val = format!("%ptrtoint_{}", id);
                let zero_val = format!("%ptr_zero_{}", ctx.next_id());
                let cmp_val = format!("%ptr_nonnull_{}", id);
                out.push_str(&format!("    {} = llvm.ptrtoint {} : !llvm.ptr to i64\n", int_val, cond_val));
                out.push_str(&format!("    {} = arith.constant 0 : i64\n", zero_val));
                out.push_str(&format!("    {} = arith.cmpi ne, {}, {} : i64\n", cmp_val, int_val, zero_val));
                cmp_val
            } else if cond_ty != Type::Bool {
                return Err(format!("While condition must be boolean, found {:?}", cond_ty));
            } else {
                cond_val
            };

            let loc = ctx.loc_tag(w.cond.span());
            out.push_str(&format!("    cf.cond_br {}, ^{}, ^{}{}\n", cond_val, label_body, label_exit, loc));
            out.push_str(&format!("  ^{}:\n", label_body));

            // Heartbeat Injection (simplified, uses @yielding at function level)
            if !*ctx.no_yield() {
                ctx.emit_lto_hook(out, "__salt_yield_check", &[], local_vars, None)?;
            }
            ctx.break_labels_mut().push(label_exit.clone());
            ctx.continue_labels_mut().push(label_header.clone());
            let mut body_vars = local_vars.clone();

            // === Z3 HOARE LOGIC: While Loop Verification ===
            // Auto-infer simple invariants before checking explicit ones.
            // If the loop is `let mut i = K; while i < N { ... i = i + 1; }`,
            // synthesize `invariant i >= K && i < N` automatically.
            let auto_inv = try_infer_while_invariant(&w.cond, &w.body.stmts, local_vars);
            // Houdini-lite: propose each body call's requires clause,
            // substituted into this loop's own variable names, as a
            // further candidate, and keep only the ones that hold at the
            // base case. See docs/SPEC.md's havoc entry for why this is
            // sound: dropping a candidate that doesn't hold changes
            // nothing (the call it came from is still checked for real,
            // with concrete arguments, during body emission below), and
            // keeping one that does only ever adds a true fact.
            let call_candidates = collect_call_requires_candidates(ctx, &w.body.stmts);
            let surviving_candidates = filter_call_requires_candidates(ctx, call_candidates, &body_vars);
            let mut synthesized: Vec<syn::Expr> = auto_inv.iter().cloned().collect();
            synthesized.extend(surviving_candidates);
            let all_stmts: Vec<Stmt> = if !synthesized.is_empty() {
                let mut s = w.body.stmts.clone();
                for inv in synthesized.iter().rev() {
                    s.insert(0, Stmt::Invariant(inv.clone()));
                }
                s
            } else {
                w.body.stmts.clone()
            };
            let invariant_exprs = prove_while_loop_base_case(ctx, &all_stmts, &body_vars)?;
            let body_to_emit = if !synthesized.is_empty() { &all_stmts } else { &w.body.stmts };
            let havoc_names = setup_while_loop_inductive_step(ctx, body_to_emit, &mut body_vars, &w.cond, &invariant_exprs)?;

            // Push loop assumptions so callee precondition verification
            // inside the body can use invariants + guard to discharge bounds.
            let mut assumption_count = 0;
            for inv in &invariant_exprs {
                ctx.emission.loop_assumptions.push(inv.clone());
                assumption_count += 1;
            }
            ctx.emission.loop_assumptions.push(w.cond.clone());
            assumption_count += 1;

            let ptr_narrowing = super::get_narrowing_target(&w.cond);
            if let Some((ref var, true)) = ptr_narrowing { ctx.pointer_tracker.push_scope(); ctx.pointer_tracker.mark_valid(var); }
            let body_diverges = super::emit_block(ctx, out, body_to_emit, &mut body_vars)?;
            if ptr_narrowing.is_some() { ctx.pointer_tracker.pop_scope(); }

            // Pop loop assumptions
            for _ in 0..assumption_count {
                ctx.emission.loop_assumptions.pop();
            }

            verify_while_loop_post_body(ctx, &w.cond, &invariant_exprs, &havoc_names, local_vars);
            ctx.break_labels_mut().pop();
            ctx.continue_labels_mut().pop();

            if !body_diverges {
                out.push_str(&format!("    cf.br ^{}\n", label_header));
            }
            out.push_str(&format!("  ^{}:\n", label_exit));
            Ok(false)
        }
