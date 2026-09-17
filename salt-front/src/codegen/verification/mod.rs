//! Verification Module - Z3-based formal verification for Salt
//!
//! This module provides:
//! - `SymbolicContext`: Cache for Z3 uninterpreted functions (field access)
//! - `VerificationEngine`: Contract verification for #requires clauses
//! - `Z3StateTracker`: Ownership state machine for memory safety proofs
//! - `ArenaVerifier`: Z3 verification of arena mark/reset use-after-free safety
//! - `exhaustiveness`: Z3-based match exhaustiveness checking

mod state_tracker;
pub(crate) mod loop_bounds;
pub(crate) mod array_tracker;
pub mod malloc_tracker;
pub mod exhaustiveness;
pub mod arena_verifier;
pub mod hash_loop_verifier;
pub mod proof_witness;
pub mod slice_verifier;
#[cfg(test)] mod slice_verifier_tests;
pub mod silicon_ingest;
pub mod executor_verifier;
pub mod hardware_target;
pub mod c10m_validator;
pub mod stack_stability;
pub mod pointer_state;
pub mod arena_escape;
pub mod ptr_bounds_verifier;
pub mod proof_hint;
pub(crate) mod contract_inheritance;
mod fold_constants;
#[cfg(test)] mod z3_smoke_tests;

pub use state_tracker::{OwnershipState, Z3StateTracker};
pub use malloc_tracker::MallocTracker;
pub use pointer_state::{PointerState, PointerStateTracker};
pub use exhaustiveness::{check_exhaustiveness, ExhaustivenessResult};
pub use arena_verifier::ArenaVerifier;
pub use arena_escape::ArenaEscapeTracker;
pub use proof_witness::{ProofHint, VerificationFailure};

use crate::codegen::context::LoweringContext;
use crate::types::Type;
use std::collections::HashMap;
use crate::z3_shim::ast::Ast;
use syn::spanned::Spanned;

use std::rc::Rc;

pub struct SymbolicContext<'ctx> {
    pub z3_ctx: &'ctx crate::z3_shim::Context,
    // Cache for field access functions: "len" -> FuncDecl(Ptr -> Int)
    field_decls: std::cell::RefCell<HashMap<String, Rc<crate::z3_shim::FuncDecl<'ctx>>>>,
}

impl<'ctx> SymbolicContext<'ctx> {
    pub fn new(z3_ctx: &'ctx crate::z3_shim::Context) -> Self {
        Self {
            z3_ctx,
            field_decls: std::cell::RefCell::new(HashMap::new()),
        }
    }

    pub fn get_field_func(&self, name: &str) -> Rc<crate::z3_shim::FuncDecl<'ctx>> {
        let mut cache = self.field_decls.borrow_mut();
        if let Some(decl) = cache.get(name) {
            return decl.clone();
        }
        
        // Create a new uninterpreted function: Field(Object) -> Int
        // This is where we solve the move error: use a reference/clone here
        let symbol = crate::z3_shim::Symbol::String(name.to_string());
        let decl = crate::z3_shim::FuncDecl::new(
            self.z3_ctx,
            symbol,
            &[&crate::z3_shim::Sort::int(self.z3_ctx)], // Domain: Struct/Object (as Int/Ptr)
            &crate::z3_shim::Sort::int(self.z3_ctx)     // Range: Field Value (Int)
        );
        let decl_rc = Rc::new(decl);
        
        cache.insert(name.to_string(), decl_rc.clone());
        decl_rc
    }
}

/// Z3 proof budget for both `requires` and `ensures` checks, as an
/// `rlimit` (Z3-internal resource units) rather than a wall-clock
/// millisecond timeout. Same proof decision every time regardless of
/// machine speed or load -- a wall-clock "timeout" made the compiler's
/// output depend on how busy the machine happened to be at build time:
/// a borderline check could be proven on an idle box and silently
/// deferred to a runtime check on a loaded one, so identical source
/// could produce different binaries. Confirmed directly (see the commit
/// that introduced this constant): rebuilding an earlier, wall-clock-
/// timeout commit and running mlir_determinism_gate.sh under the same
/// conditions reproduced the same intermittent drift; switching to
/// rlimit here did not, across a much larger sample.
///
/// 2,000,000 was chosen empirically, not derived: high enough that the
/// full z3_contracts suite (76 fixtures) still compiles in ~20s total
/// with no fixture individually slow, low enough to avoid a cliff this
/// investigation found by testing larger values directly -- a single
/// bitwise-monotonicity query (test_bv.salt's or_monotonic) that returns
/// in well under a second at this budget took over a minute and was
/// killed, unfinished, at 25x more. Rlimit cost does not scale linearly
/// with problem difficulty for every query shape; treat "just raise the
/// number" as a real compile-time-cost risk, not a free knob, if this
/// ever needs retuning.
const Z3_PROOF_RLIMIT: u32 = 2_000_000;

pub struct VerificationEngine;

impl VerificationEngine {
    #[allow(clippy::cognitive_complexity)]
    pub fn verify(
        ctx: &mut LoweringContext<'_, '_>,
        out: &mut String,
        requires: &[syn::Expr],
        params: &[String],
        arg_exprs: &[syn::Expr],
        local_vars: &mut HashMap<String, (Type, crate::codegen::context::LocalKind)>,
        param_tys: &[Type],
    ) -> Result<(), String> {
        if requires.is_empty() || ctx.config.no_verify {
            return Ok(());
        }

        // Initialize Symbolic Context
        let sym_ctx = SymbolicContext::new(ctx.z3_ctx);

        // 1. Translate Arguments to Z3 values
        // These must be kept alive for the duration of verification
        let mut call_vals_z3 = Vec::new();
        
        for arg_expr in arg_exprs {
            if let Ok(z3_val) = crate::codegen::expr::translate_to_z3(ctx, arg_expr, local_vars) {
                call_vals_z3.push(z3_val);
            } else {
                // Hard error on translation failure.
                // If we can't translate an argument, we cannot verify the precondition.
                // Silently substituting zero would create false positive verification.
                return Err(crate::errors::coded(
                    "E009",
                    format!(
                        "FORMAL SOUNDNESS ERROR: Cannot translate argument {:?} to Z3. \
                         Verification requires all arguments be expressible in the solver domain.",
                        arg_expr
                    )
                ));
            }
        }

        // 2. Prepare Substitution Map
        // Fresh constants are created for the parameters: "p0", "p1", etc.
        // And they are mapped to the actual argument values.
        
        let mut created_symbols = Vec::new(); // Owner of parameter symbols
        let mut dummy_locals = HashMap::new(); // For resolving parameter names in `requires` exprs
       
        for (i, p_name) in params.iter().enumerate() {
             if i < call_vals_z3.len() {
                 let sym = crate::z3_shim::ast::Int::new_const(ctx.z3_ctx, p_name.clone());
                 created_symbols.push(sym);
                 
                 // SSA kind is used which triggers fallback in translate_to_z3 to mk_var,
                 // ensuring consistent name usage.
                 dummy_locals.insert(p_name.clone(), (Type::Unit, crate::codegen::context::LocalKind::SSA(p_name.clone())));
             }
        }

        let mut from_vec = Vec::new();
        let mut to_vec = Vec::new();
        for (i, sym) in created_symbols.iter().enumerate() {
            from_vec.push(sym);
            if let Some(val) = call_vals_z3.get(i) {
                to_vec.push(val);
            }
        }
        
        let substitutions: Vec<(&crate::z3_shim::ast::Int, &crate::z3_shim::ast::Int)> = from_vec.iter().zip(to_vec.iter())
            .map(|(f, t)| (*f, *t))
            .collect();

        // 2.4. Record call-site concrete parameter values for quantifier expansion.
        // Enables forall/exists bounds like 0..(n-1) to be resolved to concrete
        // integers at call sites where n is a literal argument.
        for (i, p_name) in params.iter().enumerate() {
            if i < call_vals_z3.len() {
                if let Some(val) = call_vals_z3[i].as_i64() {
                    crate::codegen::verification::loop_bounds::set_call_site_param(p_name, val);
                }
            }
        }

        // 2.5. Build known-length map for .length()/.len() constant folding
        let mut known_lengths: HashMap<String, i64> = HashMap::new();
        for (i, arg) in arg_exprs.iter().enumerate() {
            if i < params.len() {
                let param = &params[i];
                // String literal arguments: .length() folds to byte count
                if let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) = arg {
                    known_lengths.insert(param.clone(), s.value().len() as i64);
                }
                // Let-bound string literals: `let x = "hello"; f(x)`
                if let syn::Expr::Path(p) = arg {
                    if let Some(ident) = p.path.get_ident() {
                        if let Some(&len) = ctx.emission.known_string_lengths.get(&ident.to_string()) {
                            known_lengths.insert(param.clone(), len);
                        }
                        // Let-bound slice constructions: `let s = Slice::new(p, 100); f(s)`
                        if let Some(&len) = ctx.emission.known_slice_lengths.get(&ident.to_string()) {
                            known_lengths.insert(param.clone(), len);
                        }
                    }
                }
                // Array-typed parameters: [T; N] has known length N.
                // Unwrap references: &[T; N] still has compile-time-known length N.
                if i < param_tys.len() {
                    let ty = &param_tys[i];
                    let array_ty = match ty {
                        crate::types::Type::Array(..) => Some(ty),
                        crate::types::Type::Reference(inner, _) => {
                            if matches!(inner.as_ref(), crate::types::Type::Array(..)) {
                                Some(inner.as_ref())
                            } else { None }
                        }
                        _ => None,
                    };
                    if let Some(crate::types::Type::Array(_, len, _)) = array_ty {
                        known_lengths.insert(param.clone(), *len as i64);
                    }
                }
            }
        }

        // 3. Verify Each Clause
        for req in requires {
            // Unwrap Block: Grammar parses `requires { expr }` as Expr::Block
            // The inner expression must be extracted for Z3 translation.
            let actual_req = if let syn::Expr::Block(block) = req {
                if let Some(syn::Stmt::Expr(inner, _)) = block.block.stmts.first() {
                    inner
                } else {
                    return Err(crate::errors::coded("E009", "Empty requires block"));
                }
            } else {
                req
            };

            // Tier 1: try compile-time evaluation with known argument values.
            // If the expression resolves to a concrete boolean, skip Z3 entirely.
            if let Some(value) = fold_constants::try_eval(actual_req, &known_lengths, params, arg_exprs) {
                if let crate::evaluator::ConstValue::Bool(false) = value {
                    return Err(crate::errors::coded(
                        "E009",
                        "VERIFICATION ERROR: contract evaluates to false with the given arguments"
                    ));
                }
                // Bool(true): proven at compile time, skip Z3
                if matches!(value, crate::evaluator::ConstValue::Bool(true)) {
                    continue;
                }
                // Non-bool result: fall through to Z3
            }

            // Tier 2: Z3 symbolic verification
            if let Ok(z3_req_sym) = crate::codegen::expr::translate_bool_to_z3(ctx, actual_req, &dummy_locals, &sym_ctx) {
                 let z3_req_subst = z3_req_sym.substitute(&substitutions);
                 
                 // ═══════════════════════════════════════════════════════════════
                 // Z3 PROOF-OR-PANIC — SAT/UNSAT POLARITY (DO NOT INVERT)
                 // ═══════════════════════════════════════════════════════════════
                 // The Z3 solver checks the NEGATION of the requirement:
                 //
                 //   Z3.assert(NOT(requirement))
                 //   Z3.check()
                 //
                 //   UNSAT → NOT(requirement) is impossible
                 //        → requirement is ALWAYS TRUE
                 //        → VERIFIED ✓ (check elided, zero runtime cost)
                 //
                 //   SAT   → NOT(requirement) has a satisfying assignment
                 //        → requirement CAN BE VIOLATED
                 //        → COMPILE ERROR ✗ (counterexample reported)
                 //
                 //   UNKNOWN → Z3 exceeded its proof budget (Z3_PROOF_RLIMIT)
                 //          → Emit runtime assertion as safe fallback
                 //
                 // REGRESSION GUARD: salt-front/tests/z3_contracts/run_tests.sh
                 //   - test_contract_proved.salt: requires(x != 0) with x=10 → UNSAT expected
                 //   - test_contract_rejected.salt: requires(x != 0) with x=0  → SAT expected
                 //   - test_contract_timeout.salt: complex non-linear constraint
                 //
                 // If these tests ever fail, the SAT/UNSAT polarity has been inverted.
                 // ═══════════════════════════════════════════════════════════════

                 // 3-state verification:
                 // - Check if the substituted requirement is DEFINITELY FALSE
                 //   by checking if `NOT(requirement)` is a tautology (always true).
                 // - If requirement is definitely false (e.g., 0 > 0) → REJECT
                 // - If requirement is definitely true → PASS
                 // - If Z3 can't determine (uninterpreted functions) → PASS (conservative)

                 // The negation of the requirement is checked for satisfiability.
                 // If NOT(req) is UNSAT, then req is ALWAYS TRUE (proven).
                 let solver = crate::z3_shim::Solver::new(ctx.z3_ctx);
                 let mut solver_params = crate::z3_shim::Params::new(ctx.z3_ctx);
                 solver_params.set_u32("rlimit", Z3_PROOF_RLIMIT);
                 solver.set_params(&solver_params);
                 
                 // Assert the caller's preconditions (this function's requires)
                 // to narrow the argument domain. Enabled when the caller has
                 // its own requires clauses that constrain parameters. Was a
                 // silent no-op until now: `requires { x <= LIMIT }` parses as
                 // Expr::Block, and translate_bool_to_z3 has no arm for that,
                 // so every entry here failed to translate and got dropped by
                 // the `if let Ok(..)` below -- caller_preconditions was
                 // populated and consumed correctly, just never successfully
                 // translated. `requires(x <= LIMIT)` (paren form) was never
                 // affected, which is why this stayed hidden.
                 let caller_pcs = ctx.emission.caller_preconditions.clone();
                 for pc in &caller_pcs {
                     let Some(actual_pc) = unwrap_contract_expr(pc) else { continue };
                     let dummy_locals_for_caller = local_vars.clone();
                     if let Ok(z3_pc) = crate::codegen::expr::translate_bool_to_z3(
                         ctx, actual_pc, &dummy_locals_for_caller, &sym_ctx,
                     ) {
                         solver.assert(&z3_pc);
                     }
                 }

                 // Also add path conditions from the caller's context to constrain the arguments
                 let path_conditions = ctx.emission.path_conditions.clone();
                 for pc in &path_conditions {
                     let dummy_locals_for_pc = local_vars.clone();
                     if let Ok(z3_pc) = crate::codegen::expr::translate_bool_to_z3(ctx, pc, &dummy_locals_for_pc, &sym_ctx) {
                         solver.assert(&z3_pc);
                     }
                 }

                 // Also add loop assumptions (while-loop invariants + guard)
                 // so callee bounds contracts can be discharged inside loops.
                 let loop_assumptions = ctx.emission.loop_assumptions.clone();
                 let locals_snapshot = local_vars.clone();
                 for la in &loop_assumptions {
                     if let Ok(z3_la) = crate::codegen::expr::translate_bool_to_z3(ctx, la, &locals_snapshot, &sym_ctx) {
                         solver.assert(&z3_la);
                     }
                 }

                 // Also add `name == init` for every non-`mut` local in
                 // scope (see assert_local_expr_in_z3 in codegen/stmt/mod.rs)
                 // so a requires stated over a let-bound name's defining
                 // expression -- or vice versa -- can be related to it.
                 let scoped_facts = ctx.emission.scoped_facts.clone();
                 for lb in &scoped_facts {
                     if let Ok(z3_lb) = crate::codegen::expr::translate_bool_to_z3(ctx, lb, &locals_snapshot, &sym_ctx) {
                         solver.assert(&z3_lb);
                     }
                 }

                 // Inject type-based bounds so Z3 proves contracts
                 // implied by the type system (e.g., u8 ∈ [0, 255]).
                 assert_type_bounds(ctx, &call_vals_z3, param_tys, &solver);

                 // Also bound every OTHER typed value the check actually
                 // depends on -- not just this call's own arguments, but not
                 // every local in the caller's scope either (see
                 // assert_scope_type_bounds). A local referenced only in a
                 // path condition (e.g. `if dense_idx >= count`) never
                 // appears in call_vals_z3, so without this its
                 // non-negativity was never available to derive facts like
                 // "count >= 1" from "dense_idx < count".
                 let relevant = collect_ident_names_from(
                     std::iter::once(actual_req)
                         .chain(caller_pcs.iter())
                         .chain(path_conditions.iter())
                         .chain(loop_assumptions.iter())
                         .chain(scoped_facts.iter()),
                 );
                 assert_scope_type_bounds(ctx, local_vars, &relevant, &solver);

                 // Inject Pointer State Tokens
                 // For each argument that is a known variable, map its pointer state into Z3
                 for (i, _p_name) in params.iter().enumerate() {
                     if let Some(arg_expr) = arg_exprs.get(i) {
                         if let Some(var_name) = crate::codegen::expr::extract_ident_name(arg_expr) {
                             if let Some(state) = ctx.pointer_tracker.get_state(&var_name) {
                                 if let Some(z3_val) = call_vals_z3.get(i) {
                                     let sort_refs = [&crate::z3_shim::Sort::int(ctx.z3_ctx)];
                                     
                                     let valid_func = crate::z3_shim::FuncDecl::new(
                                         ctx.z3_ctx,
                                         crate::z3_shim::Symbol::String("valid".to_string()),
                                         &sort_refs,
                                         &crate::z3_shim::Sort::bool(ctx.z3_ctx),
                                     );
                                     let freed_func = crate::z3_shim::FuncDecl::new(
                                         ctx.z3_ctx,
                                         crate::z3_shim::Symbol::String("freed".to_string()),
                                         &sort_refs,
                                         &crate::z3_shim::Sort::bool(ctx.z3_ctx),
                                     );
                                     
                                     let arg_refs: Vec<&dyn crate::z3_shim::ast::Ast> = vec![z3_val as &dyn crate::z3_shim::ast::Ast];
                                     let valid_app = valid_func.apply(&arg_refs).as_bool().unwrap();
                                     let freed_app = freed_func.apply(&arg_refs).as_bool().unwrap();
                                     
                                     
                                     match state {
                                         crate::codegen::verification::PointerState::Valid => {
                                             solver.assert(&valid_app._eq(&crate::z3_shim::ast::Bool::from_bool(ctx.z3_ctx, true)));
                                             solver.assert(&freed_app._eq(&crate::z3_shim::ast::Bool::from_bool(ctx.z3_ctx, false)));
                                         }
                                         crate::codegen::verification::PointerState::Freed => {
                                             solver.assert(&valid_app._eq(&crate::z3_shim::ast::Bool::from_bool(ctx.z3_ctx, false)));
                                             solver.assert(&freed_app._eq(&crate::z3_shim::ast::Bool::from_bool(ctx.z3_ctx, true)));
                                         }
                                         _ => {}
                                     }
                                 }
                             } 
                         }
                     }
                 }
                 
                 solver.assert(&z3_req_subst.not());
                 
                 *ctx.total_checks += 1;
                 
                 match solver.check() {
                     crate::z3_shim::SatResult::Sat => {
                         // The negation CAN be satisfied → the requirement can be VIOLATED!
                         let constraint_str = format!("{}", z3_req_subst);
                         let span = actual_req.span();
                         let line = span.start().line;
                         let source_info = if !ctx.config.source_file.is_empty() {
                             format!("{}:{}", ctx.config.source_file, line)
                         } else {
                             format!("line {}", line)
                         };

                         // Extract counterexample values from the substitution map
                         let mut counterexample_values = Vec::new();
                         if let Some(model) = solver.get_model() {
                             for (i, p_name) in params.iter().enumerate() {
                                 if let Some(z3_val) = call_vals_z3.get(i) {
                                     if let Some(val) = model.eval(z3_val, true) {
                                         counterexample_values.push((p_name.clone(), val.as_i64().unwrap_or(0)));
                                     }
                                 }
                             }
                         }

                         let mut failure = if counterexample_values.is_empty() {
                             proof_witness::VerificationFailure::new(
                                 constraint_str,
                                 format!("precondition check ({})", source_info),
                             )
                         } else {
                             proof_witness::VerificationFailure::with_counterexample(
                                 constraint_str,
                                 format!("precondition check ({})", source_info),
                                 counterexample_values,
                             )
                         };
                         // classify_constraint already caught the havoc'd-argument
                         // shape generically (AddInvariant(None)) from the "_havoc_"
                         // substring alone; upgrade to the specific suggestion now
                         // that actual_req/params/arg_exprs are in scope to rewrite
                         // the callee's clause into the caller's own terms.
                         if failure.hints.iter().any(|h| matches!(h, proof_witness::ProofHint::AddInvariant(None))) {
                             let suggestion = rewrite_requires_in_caller_terms(actual_req, params, arg_exprs);
                             failure.hints = vec![proof_witness::ProofHint::AddInvariant(Some(suggestion))];
                         }
                         return Err(failure.format_error());
                     }
                     crate::z3_shim::SatResult::Unsat => {
                         // The negation CANNOT be satisfied → the requirement is PROVEN!
                         *ctx.elided_checks += 1;
                     }
                     crate::z3_shim::SatResult::Unknown => {
                         // Z3 could not determine satisfiability (timeout / incomplete theory)
                         let constraint_str = format!("{}", z3_req_subst);
                         eprintln!(
                             "WARNING: Z3 could not prove `requires({})` within budget. \
                              Emitting runtime check.",
                             constraint_str
                         );
                         emit_requires_runtime_check(
                             ctx, out, actual_req, params, arg_exprs,
                             local_vars, param_tys,
                         )?;
                     }
                 }
            } else {
                // Failed to translate requirement.
                return Err(crate::errors::coded(
                    "E009",
                    format!("Verification Logic Error: Could not translate requirement expression: {:?}", req)
                ));
            }
        }

        Ok(())
    }

    /// Apply postconditions to the caller's context (e.g. updating PointerStateTracker)
    pub fn apply_postconditions(
        ctx: &mut LoweringContext<'_, '_>,
        ensures: &[syn::Expr],
        params: &[String],
        arg_exprs: &[syn::Expr],
    ) {
        for ens in ensures {
            let actual_ens = if let syn::Expr::Block(block) = ens {
                if let Some(syn::Stmt::Expr(inner, _)) = block.block.stmts.first() {
                    inner
                } else {
                    ens
                }
            } else {
                ens
            };

            if let syn::Expr::Call(call) = actual_ens {
                let func_name = if let syn::Expr::Path(p) = &*call.func {
                    p.path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>().join("_")
                } else {
                    "".to_string()
                };


                if (func_name == "valid" || func_name == "freed") && call.args.len() == 1 {
                    // Check if it's `result`
                    if let syn::Expr::Path(p) = &call.args[0] {
                        let arg_name = p.path.get_ident().map(|i| i.to_string()).unwrap_or_default();
                        if arg_name == "result" {
                            let state = if func_name == "valid" {
                                crate::codegen::verification::PointerState::Valid
                            } else {
                                crate::codegen::verification::PointerState::Freed
                            };
                            *ctx.pending_pointer_state = Some(state);
                            continue;
                        }

                        // Otherwise find which parameter this corresponds to
                        let arg_idx = params.iter().position(|name| name == &arg_name);

                        if let Some(idx) = arg_idx {
                            if let Some(arg_expr) = arg_exprs.get(idx) {
                                if let Some(var_name) = crate::codegen::expr::extract_ident_name(arg_expr) {
                                    if func_name == "valid" {
                                        ctx.pointer_tracker.mark_valid(&var_name);
                                    } else if func_name == "freed" {
                                        ctx.pointer_tracker.mark_freed(&var_name);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Apply store records for a specific array to a solver.
    /// Asserts update assertions + unbounded ForAll frame axioms.
    fn apply_stores_for_array(
        ctx: &mut LoweringContext<'_, '_>,
        solver: &crate::z3_shim::Solver<'_>,
        base_name: &str,
        local_vars: &HashMap<String, (Type, crate::codegen::context::LocalKind)>,
    ) {
        let stores = crate::codegen::verification::array_tracker::get_stores(base_name);
        let applied = crate::codegen::verification::array_tracker::stores_applied(base_name);
        if applied >= stores.len() {
            return;
        }
        let mut cur_ver = applied;
        for store in &stores[applied..] {
            let old_ver = cur_ver;
            let new_ver = cur_ver + 1;
            cur_ver = new_ver;
            let old_name = format!("{}_v{}", base_name, old_ver);
            let new_name = format!("{}_v{}", base_name, new_ver);
            let int_sort = crate::z3_shim::Sort::int(ctx.z3_ctx);
            let old_func = crate::z3_shim::FuncDecl::new(
                ctx.z3_ctx, crate::z3_shim::Symbol::String(old_name),
                &[&int_sort], &int_sort,
            );
            let new_func = crate::z3_shim::FuncDecl::new(
                ctx.z3_ctx, crate::z3_shim::Symbol::String(new_name),
                &[&int_sort], &int_sort,
            );
            if let (Ok(s_idx), Ok(s_val)) = (
                crate::codegen::expr::translate_to_z3(ctx, &store.index_expr, local_vars),
                crate::codegen::expr::translate_to_z3(ctx, &store.value_expr, local_vars),
            ) {
                use crate::z3_shim::ast::Ast;
                // Update assertion: new_func(store_idx) == store_val
                let new_at_idx = new_func.apply(&[&s_idx]);
                if let Some(new_int) = new_at_idx.as_int() {
                    solver.assert(&new_int._eq(&s_val));
                }
                // Unbounded ForAll frame axiom: forall k, k != store_idx ⇒ new(k)==old(k)
                let k_name = format!("k_fr_pc_{}", ctx.next_id());
                let k_fr = crate::z3_shim::ast::Int::new_const(ctx.z3_ctx, k_name.as_str());
                let k_ne_i = k_fr._eq(&s_idx).not();
                let old_at_k = old_func.apply(&[&k_fr]);
                let new_at_k = new_func.apply(&[&k_fr]);
                if let (Some(old_int), Some(new_int)) = (old_at_k.as_int(), new_at_k.as_int()) {
                    let frame_eq = new_int._eq(&old_int);
                    let frame_body = crate::z3_shim::ast::Bool::or(
                        ctx.z3_ctx, &[&k_ne_i.not(), &frame_eq],
                    );
                    solver.assert(&crate::z3_shim::ast::forall_const(
                        ctx.z3_ctx, &[&k_fr], &[], &frame_body,
                    ));
                }
            }
        }
    }

    /// Weakest Precondition verification for `ensures` clauses.
    ///
    /// At each return site, substitutes `result` in the ensures expression with
    /// the actual return value, then checks the obligation via Z3.
    ///
    /// Verification logic:
    ///   1. Create symbolic variables for all function parameters
    ///   2. Assume all `requires` preconditions (narrow the input domain)
    ///   3. For each `ensures` clause, substitute `result` with the return value
    ///   4. Check: can the negation of the postcondition be satisfied?
    ///      - UNSAT → postcondition is PROVEN (violation impossible)
    ///      - SAT → postcondition VIOLATED (counterexample found)
    ///      - Unknown → deferred to runtime assertion
    #[allow(clippy::too_many_arguments)] // REASON: all 9 params independently meaningful; bundling would obscure intent
    pub fn verify_postcondition(
        ctx: &mut LoweringContext<'_, '_>,
        out: &mut String,
        ensures: &[syn::Expr],
        requires: &[syn::Expr],
        return_expr: &syn::Expr,
        params: &[String],
        local_vars: &HashMap<String, (Type, crate::codegen::context::LocalKind)>,
        fn_name: &str,
        return_ty: &Type,
    ) -> Result<bool, String> {
        if ensures.is_empty() || ctx.config.no_verify {
            return Ok(false);
        }

        let sym_ctx = SymbolicContext::new(ctx.z3_ctx);
        let mut verified = false;
        use crate::z3_shim::ast::Ast;

        // Create a fresh solver with a bounded proof budget for postcondition proofs
        let solver = crate::z3_shim::Solver::new(ctx.z3_ctx);
        let mut solver_params = crate::z3_shim::Params::new(ctx.z3_ctx);
        solver_params.set_u32("rlimit", Z3_PROOF_RLIMIT);
        solver.set_params(&solver_params);

        // 1. Create symbolic constants for function parameters
        let mut param_symbols = Vec::new();
        for p_name in params {
            let sym = crate::z3_shim::ast::Int::new_const(ctx.z3_ctx, p_name.clone());
            param_symbols.push((p_name.clone(), sym));
        }

        // Build a dummy local_vars map for parameter name resolution in Z3
        let mut z3_locals = local_vars.clone();
        for (name, _) in &param_symbols {
            if !z3_locals.contains_key(name) {
                z3_locals.insert(name.clone(), (Type::I32, crate::codegen::context::LocalKind::SSA(name.clone())));
            }
        }

        // 2. Assume preconditions (requires clauses narrow the input domain)
        for req in requires {
            let actual_req = if let syn::Expr::Block(block) = req {
                if let Some(syn::Stmt::Expr(inner, _)) = block.block.stmts.first() {
                    inner
                } else {
                    continue;
                }
            } else {
                req
            };

            if let Ok(z3_req) = crate::codegen::expr::translate_bool_to_z3(ctx, actual_req, &z3_locals, &sym_ctx) {
                solver.assert(&z3_req);
            }
        }

        // 2b. Assume branch conditions (path guards)
        // These are pushed by emit_if_expr when entering then/else branches.
        // They tell Z3 what branch we're in (e.g., "x < 0" in the then-branch).
        let path_conds = ctx.emission.path_conditions.clone();
        for pc in &path_conds {
            if let Ok(z3_pc) = crate::codegen::expr::translate_bool_to_z3(ctx, pc, &z3_locals, &sym_ctx) {
                solver.assert(&z3_pc);
            }
        }

        // 2c. Assume `name == init` for every non-`mut` local in scope (see
        // assert_local_expr_in_z3 in codegen/stmt/mod.rs), so an ensures
        // clause stated over a let-bound name's defining expression -- or
        // vice versa, as in is_valid_user_ptr's `end`/`ptr + len` -- proves.
        let scoped_facts = ctx.emission.scoped_facts.clone();
        for lb in &scoped_facts {
            if let Ok(z3_lb) = crate::codegen::expr::translate_bool_to_z3(ctx, lb, &z3_locals, &sym_ctx) {
                solver.assert(&z3_lb);
            }
        }

        // Inject Pointer State Tokens for ensures
        for p_name in params.iter() {
            if let Some(state) = ctx.pointer_tracker.get_state(p_name) {
                if let Some((_, sym)) = param_symbols.iter().find(|(n, _)| n == p_name) {
                    let sort_refs = [&crate::z3_shim::Sort::int(ctx.z3_ctx)];
                    let valid_func = crate::z3_shim::FuncDecl::new(
                        ctx.z3_ctx,
                        crate::z3_shim::Symbol::String("valid".to_string()),
                        &sort_refs,
                        &crate::z3_shim::Sort::bool(ctx.z3_ctx),
                    );
                    let freed_func = crate::z3_shim::FuncDecl::new(
                        ctx.z3_ctx,
                        crate::z3_shim::Symbol::String("freed".to_string()),
                        &sort_refs,
                        &crate::z3_shim::Sort::bool(ctx.z3_ctx),
                    );
                    let arg_refs: Vec<&dyn crate::z3_shim::ast::Ast> = vec![sym as &dyn crate::z3_shim::ast::Ast];
                    let valid_app = valid_func.apply(&arg_refs).as_bool().unwrap();
                    let freed_app = freed_func.apply(&arg_refs).as_bool().unwrap();
                    
                    match state {
                        crate::codegen::verification::PointerState::Valid => {
                            solver.assert(&valid_app._eq(&crate::z3_shim::ast::Bool::from_bool(ctx.z3_ctx, true)));
                            solver.assert(&freed_app._eq(&crate::z3_shim::ast::Bool::from_bool(ctx.z3_ctx, false)));
                        }
                        crate::codegen::verification::PointerState::Freed => {
                            solver.assert(&valid_app._eq(&crate::z3_shim::ast::Bool::from_bool(ctx.z3_ctx, false)));
                            solver.assert(&freed_app._eq(&crate::z3_shim::ast::Bool::from_bool(ctx.z3_ctx, true)));
                        }
                        _ => {}
                    }
                }
            }
        }

        // 2c. [v4.0] Axiomatize intrinsics in the return expression
        Self::axiomatize_intrin_find_byte(ctx, return_expr, &solver, &z3_locals);

        // 2d. Apply array stores from the function body to the postcondition solver.
        // This connects body writes (arr[0]=10) to ensures forall (arr[0]<=arr[1]).
        // Sets STORES_APPLIED so translate_to_z3:Expr::Index (called during ensures
        // checking below) skips store re-application into ctx.z3_solver.
        {
            let store_names = crate::codegen::verification::array_tracker::get_store_names();
            for name in &store_names {
                Self::apply_stores_for_array(ctx, &solver, name, &z3_locals);
                let stores = crate::codegen::verification::array_tracker::get_stores(name);
                crate::codegen::verification::array_tracker::mark_stores_applied(name, stores.len());
            }
        }

        // 3. Translate the return value expression to Z3
        let z3_return_val = crate::codegen::expr::translate_to_z3(ctx, return_expr, &z3_locals);

        // 4. For each ensures clause, substitute `result` and verify
        for ens in ensures {
            let actual_ens = if let syn::Expr::Block(block) = ens {
                if let Some(syn::Stmt::Expr(inner, _)) = block.block.stmts.first() {
                    inner
                } else {
                    continue;
                }
            } else {
                ens
            };

            // Create a `result` symbol and register it in the Z3 locals, typed
            // by the function's REAL return type rather than a placeholder --
            // this used to be hardcoded to Type::I32 regardless of what the
            // function actually returned, so `ensures { result >= 0 }` on a
            // u32-returning function got no bound at all on `result` itself.
            let result_sym = crate::z3_shim::ast::Int::new_const(ctx.z3_ctx, "result");
            let mut ens_locals = z3_locals.clone();
            ens_locals.insert("result".to_string(), (return_ty.clone(), crate::codegen::context::LocalKind::SSA("result".to_string())));

            if let Ok(z3_ens) = crate::codegen::expr::translate_bool_to_z3(ctx, actual_ens, &ens_locals, &sym_ctx) {
                if let Ok(ref ret_val) = z3_return_val {
                    // WP Check: Assume result == return_value, then check NOT(postcondition)
                    let binding = result_sym._eq(ret_val);

                    solver.push();
                    solver.assert(&binding);
                    solver.assert(&z3_ens.not());
                    // Every typed value the postcondition itself mentions,
                    // INCLUDING result now that it carries its real type,
                    // gets its type's range asserted -- the postcondition
                    // check never had this at all before. Also scoped to
                    // scoped_facts' free variables (e.g. `ptr`/`len` behind
                    // `end == ptr + len`), the same reasoning as `verify`'s
                    // requires-check scoping above -- AND to return_expr's
                    // free variables: `binding` ties `result` to `ret_val`
                    // (return_expr, translated), so a postcondition over
                    // `result` alone (`ensures { result > 0 }` on
                    // `return n + 1`) needs `n`'s bound to derive anything,
                    // even though `n` never appears in `actual_ens` itself.
                    // Without this, `n: u64`'s non-negativity was invisible
                    // to this check and Z3 could pick n = -1.
                    let relevant = collect_ident_names_from(
                        std::iter::once(actual_ens).chain(scoped_facts.iter()).chain(std::iter::once(return_expr)),
                    );
                    assert_scope_type_bounds(ctx, &ens_locals, &relevant, &solver);
                    *ctx.total_checks += 1;

                    match solver.check() {
                        crate::z3_shim::SatResult::Unsat => {
                            // PROVEN: No input can violate the postcondition
                            *ctx.elided_checks += 1;
                            verified = true;
                        }
                        crate::z3_shim::SatResult::Sat => {
                            // VIOLATION: Z3 found inputs that violate the postcondition
                            // BUT: Check if the return expression uses untracked local variables
                            // (mutated locals like `acc` that Z3 treats as unconstrained).
                            // In that case, the SAT result is due to incomplete symbolic tracking,
                            // not a genuine violation. Defer to runtime assertion.
                            //
                            // "Defer to runtime assertion" was the comment, not the
                            // behavior: this branch was empty. A mutated local like
                            // `acc` made the postcondition SAT for a reason that had
                            // nothing to do with the code being wrong, and the result
                            // was silence -- no compile error, no runtime check, no
                            // warning. The build succeeded as if the ensures clause had
                            // been proven, which it never was.
                            let return_uses_untracked = Self::expr_uses_untracked_local(return_expr, params);
                            if return_uses_untracked {
                                eprintln!(
                                    "WARNING: Z3 could not determine `ensures({:?})` for '{}' \
                                     (return expression uses an untracked local). Emitting runtime check.",
                                    actual_ens, fn_name
                                );
                                emit_ensures_runtime_check(ctx, out, actual_ens, return_expr, local_vars, return_ty)?;
                            } else {
                                // Genuine violation: the return expression only uses tracked params/literals
                                let model = solver.get_model();
                                let mut counterexample = Vec::new();
                                if let Some(model) = model {
                                    for (name, sym) in &param_symbols {
                                        if let Some(val) = model.eval(sym, true) {
                                            counterexample.push(format!("  {} := {}", name, val));
                                        }
                                    }
                                }

                                let ce_str = if counterexample.is_empty() {
                                    String::new()
                                } else {
                                    format!("\n[Formal Shadow] Z3 counter-example:\n{}", counterexample.join("\n"))
                                };

                                solver.pop(1);
                                return Err(crate::errors::coded(
                                    "E009",
                                    format!(
                                        "Postcondition violation in '{}': ensures({:?}) is not satisfied \
                                         for all return paths.{}",
                                        fn_name, actual_ens, ce_str
                                    )
                                ));
                            }
                        }
                        crate::z3_shim::SatResult::Unknown => {
                            // BUDGET EXCEEDED: Z3 couldn't determine within
                            // Z3_PROOF_RLIMIT. The comment here matched
                            // requires' handling in wording, but not in
                            // behavior -- requires' Unknown branch calls
                            // emit_requires_runtime_check; this one called
                            // nothing. Every ensures clause complex enough to
                            // exceed budget was therefore COMPLETELY
                            // unenforced: not proven, not checked at runtime,
                            // no warning printed. Confirmed directly: an
                            // astronomically-wrong bound on a multi-branch
                            // function's postcondition compiled clean with zero
                            // indication anything was ever verified.
                            eprintln!(
                                "WARNING: Z3 could not prove `ensures({:?})` for '{}' within budget. \
                                 Emitting runtime check.",
                                actual_ens, fn_name
                            );
                            emit_ensures_runtime_check(ctx, out, actual_ens, return_expr, local_vars, return_ty)?;
                        }
                    }
                    solver.pop(1);
                }
            }
        }

        Ok(verified)
    }

    /// Check if a return expression uses local variables that aren't tracked
    /// as function parameters. Mutated locals like `acc` are unconstrained in Z3,
    /// leading to false SAT (violation) results.
    fn expr_uses_untracked_local(expr: &syn::Expr, params: &[String]) -> bool {
        match expr {
            syn::Expr::Path(p) => {
                if let Some(ident) = p.path.get_ident() {
                    let name = ident.to_string();
                    // If it's not a parameter and not "result", it's an untracked local
                    !params.contains(&name) && name != "result"
                } else {
                    false
                }
            }
            syn::Expr::Binary(b) => {
                Self::expr_uses_untracked_local(&b.left, params) ||
                Self::expr_uses_untracked_local(&b.right, params)
            }
            syn::Expr::Unary(u) => Self::expr_uses_untracked_local(&u.expr, params),
            syn::Expr::Paren(p) => Self::expr_uses_untracked_local(&p.expr, params),
            syn::Expr::Lit(_) => false,
            _ => false,
        }
    }

    fn axiomatize_intrin_find_byte<'a, 'ctx>(
        ctx: &mut LoweringContext<'a, 'ctx>,
        expr: &syn::Expr,
        solver: &crate::z3_shim::Solver<'ctx>,
        local_vars: &HashMap<String, (Type, crate::codegen::context::LocalKind)>
    ) {
        match expr {
            syn::Expr::Call(call) => {
                let func_name = if let syn::Expr::Path(p) = &*call.func {
                    p.path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>().join("_")
                } else {
                    "".to_string()
                };
                if func_name == "intrin_find_byte" && call.args.len() == 3 {
                    if let Ok(res_val) = crate::codegen::expr::translate_to_z3(ctx, expr, local_vars) {
                        if let Ok(len_val) = crate::codegen::expr::translate_to_z3(ctx, &call.args[1], local_vars) {
                            
                            let minus_one = crate::z3_shim::ast::Int::from_i64(ctx.z3_ctx, -1);
                            let zero = crate::z3_shim::ast::Int::from_i64(ctx.z3_ctx, 0);
                            
                            // res >= -1
                            solver.assert(&res_val.ge(&minus_one));
                            // res >= 0 => res < len
                            let is_pos = res_val.ge(&zero);
                            let is_less = res_val.lt(&len_val);
                            solver.assert(&is_pos.implies(&is_less));
                        }
                    }
                }
                for arg in &call.args {
                    Self::axiomatize_intrin_find_byte(ctx, arg, solver, local_vars);
                }
            }
            syn::Expr::Binary(b) => {
                Self::axiomatize_intrin_find_byte(ctx, &b.left, solver, local_vars);
                Self::axiomatize_intrin_find_byte(ctx, &b.right, solver, local_vars);
            }
            syn::Expr::Unary(u) => Self::axiomatize_intrin_find_byte(ctx, &u.expr, solver, local_vars),
            syn::Expr::Paren(p) => Self::axiomatize_intrin_find_byte(ctx, &p.expr, solver, local_vars),
            syn::Expr::Field(f) => Self::axiomatize_intrin_find_byte(ctx, &f.base, solver, local_vars),
            syn::Expr::MethodCall(mc) => {
                Self::axiomatize_intrin_find_byte(ctx, &mc.receiver, solver, local_vars);
                for arg in &mc.args {
                    Self::axiomatize_intrin_find_byte(ctx, arg, solver, local_vars);
                }
            }
            _ => {}
        }
    }
}

/// Assert type-based bounds into a Z3 solver so contracts implied by
/// the type system are proved at compile time. Covers all integer types,
/// bool, and unwraps Atomic<T> to the inner type.
/// Assert the range a Salt integer/bool TYPE guarantees for one Z3 value.
///
/// Z3's native Int is arbitrary-precision with no inherent range, so without
/// this a `u32` is indistinguishable from an unbounded signed integer to the
/// solver -- "x < y implies y > 0", true for any real unsigned pair, is not
/// derivable. This was previously inlined into assert_type_bounds and applied
/// ONLY to the direct arguments of one call-site check; factored out so the
/// same true-by-construction facts can be asserted anywhere a typed Z3 value
/// is in scope (see assert_scope_type_bounds, and try_elide_overflow_check
/// below which reuses it to bound arithmetic operands).
pub(crate) fn assert_bound_for_type<'ctx>(
    ctx: &mut LoweringContext<'_, '_>,
    val: &crate::z3_shim::ast::Int<'ctx>,
    ty: &Type,
    solver: &crate::z3_shim::Solver<'ctx>,
) {
    let zero = crate::z3_shim::ast::Int::from_i64(ctx.z3_ctx, 0);
    // Unwrap Atomic<T> to the storage type for bounds
    let ty = match ty {
        Type::Atomic(inner) => inner.as_ref(),
        other => other,
    };
    match ty {
        Type::U8 => {
            let max = crate::z3_shim::ast::Int::from_i64(ctx.z3_ctx, 255);
            solver.assert(&val.ge(&zero));
            solver.assert(&val.le(&max));
        }
        Type::U16 => {
            let max = crate::z3_shim::ast::Int::from_i64(ctx.z3_ctx, 65535);
            solver.assert(&val.ge(&zero));
            solver.assert(&val.le(&max));
        }
        Type::U32 => {
            let max = crate::z3_shim::ast::Int::from_i64(ctx.z3_ctx, 4294967295);
            solver.assert(&val.ge(&zero));
            solver.assert(&val.le(&max));
        }
        // u64::MAX (18446744073709551615) exceeds i64's range, so from_i64
        // can't represent it -- from_u64 can.
        Type::U64 | Type::Usize => {
            let max = crate::z3_shim::ast::Int::from_u64(ctx.z3_ctx, u64::MAX);
            solver.assert(&val.ge(&zero));
            solver.assert(&val.le(&max));
        }
        Type::I8 => {
            let min = crate::z3_shim::ast::Int::from_i64(ctx.z3_ctx, -128);
            let max = crate::z3_shim::ast::Int::from_i64(ctx.z3_ctx, 127);
            solver.assert(&val.ge(&min));
            solver.assert(&val.le(&max));
        }
        Type::I16 => {
            let min = crate::z3_shim::ast::Int::from_i64(ctx.z3_ctx, -32768);
            let max = crate::z3_shim::ast::Int::from_i64(ctx.z3_ctx, 32767);
            solver.assert(&val.ge(&min));
            solver.assert(&val.le(&max));
        }
        // Previously missing entirely: an i32 (arguably the single most
        // common integer type in real code) had no bounds asserted at all,
        // so the solver treated it as fully unbounded -- see
        // test_i32_i64_full_range_proved.salt for the concrete,
        // reliably-reproduced consequence (a trivially-true fact about
        // real i32 values gets a "counterexample" outside i32's actual
        // range).
        Type::I32 => {
            let min = crate::z3_shim::ast::Int::from_i64(ctx.z3_ctx, i32::MIN as i64);
            let max = crate::z3_shim::ast::Int::from_i64(ctx.z3_ctx, i32::MAX as i64);
            solver.assert(&val.ge(&min));
            solver.assert(&val.le(&max));
        }
        Type::I64 => {
            let min = crate::z3_shim::ast::Int::from_i64(ctx.z3_ctx, i64::MIN);
            let max = crate::z3_shim::ast::Int::from_i64(ctx.z3_ctx, i64::MAX);
            solver.assert(&val.ge(&min));
            solver.assert(&val.le(&max));
        }
        Type::Bool => {
            let one = crate::z3_shim::ast::Int::from_i64(ctx.z3_ctx, 1);
            solver.assert(&val.ge(&zero));
            solver.assert(&val.le(&one));
        }
        _ => {}
    }
}

fn assert_type_bounds<'ctx>(
    ctx: &mut LoweringContext<'_, '_>,
    call_vals_z3: &[crate::z3_shim::ast::Int<'ctx>],
    param_tys: &[Type],
    solver: &crate::z3_shim::Solver<'ctx>,
) {
    for (i, arg_val) in call_vals_z3.iter().enumerate() {
        if i >= param_tys.len() { continue; }
        assert_bound_for_type(ctx, arg_val, &param_tys[i], solver);
    }
}

/// Grammar parses `requires { expr }` / `ensures { expr }` as `Expr::Block`;
/// unwrap it to the inner expression for Z3 translation (the paren form,
/// `requires(expr)`, needs no unwrapping). Returns `None` for a block that
/// isn't exactly one bare expression -- callers should skip the fact rather
/// than hard-fail, since every call site this is used from is an auxiliary
/// constraint (caller_preconditions, callee ensures), not the primary check.
pub(crate) fn unwrap_contract_expr(expr: &syn::Expr) -> Option<&syn::Expr> {
    match expr {
        syn::Expr::Block(block) => match block.block.stmts.first() {
            Some(syn::Stmt::Expr(inner, _)) => Some(inner),
            _ => None,
        },
        _ => Some(expr),
    }
}

/// Rewrites an expression by replacing each bare identifier matching a
/// parameter name with the actual argument expression at some call site
/// (`params[i]` -> `arg_exprs[i]`). Used to turn a callee's `requires`
/// clause into a fact stated in a CALLER's own terms -- required whenever
/// that fact is going to be used or displayed somewhere the callee's
/// parameter names aren't in scope (the diagnostic hint below, and
/// while_stmt.rs's call-requires candidate invariants).
pub(crate) fn substitute_params_with_args(expr: &syn::Expr, params: &[String], arg_exprs: &[syn::Expr]) -> syn::Expr {
    struct ParamSubst<'a> {
        param_subs: &'a HashMap<String, syn::Expr>,
    }
    impl syn::visit_mut::VisitMut for ParamSubst<'_> {
        fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
            if let syn::Expr::Path(p) = expr {
                if let Some(ident) = p.path.get_ident() {
                    if let Some(replacement) = self.param_subs.get(&ident.to_string()) {
                        // Bare identifiers and literals never need the
                        // parens; anything else might, for precedence.
                        *expr = if matches!(replacement, syn::Expr::Path(_) | syn::Expr::Lit(_)) {
                            replacement.clone()
                        } else {
                            syn::parse_quote!((#replacement))
                        };
                        return;
                    }
                }
            }
            syn::visit_mut::visit_expr_mut(self, expr);
        }
    }

    let param_subs: HashMap<String, syn::Expr> = params.iter().enumerate()
        .filter_map(|(i, p)| arg_exprs.get(i).map(|a| (p.clone(), a.clone())))
        .collect();
    let mut rewritten = expr.clone();
    syn::visit_mut::VisitMut::visit_expr_mut(&mut ParamSubst { param_subs: &param_subs }, &mut rewritten);
    rewritten
}

/// Rewrites a callee's `requires` clause into the caller's own terms --
/// each parameter name replaced by the actual argument expression at this
/// call site -- for use in the AddInvariant diagnostic hint. Without this,
/// suggesting the clause verbatim (in the callee's own parameter names)
/// would reference names not in scope at the call site whenever an
/// argument isn't a bare variable of the same name as the parameter.
fn rewrite_requires_in_caller_terms(expr: &syn::Expr, params: &[String], arg_exprs: &[syn::Expr]) -> String {
    let rewritten = substitute_params_with_args(expr, params, arg_exprs);
    // quote!'s token-by-token join isn't a real pretty-printer -- it has no
    // opinion on spacing, just spaces between tokens by default. Only "."
    // is cleaned up: it's by far the most common punctuation in a contract
    // (field/method access), and "self . len" reads as broken in a way
    // "off + 1" doesn't.
    quote::quote!(#rewritten).to_string().replace(" . ", ".")
}

/// Assert type-derived bounds for EVERY typed value in scope, not only the
/// arguments of the one call being checked.
///
/// assert_type_bounds alone misses a real case: a local referenced ONLY in a
/// path condition (an `if` guard) rather than passed as an argument to the
/// call under verification. `dense_idx` in `if dense_idx >= count { return; }
/// return sub_checked(count, 1);` is exactly this -- it never appears in
/// sub_checked's own argument list, so its non-negativity was never
/// asserted, and "count >= 1" (needed for sub_checked's `b <= a`) could not
/// be derived from "dense_idx < count" without it.
struct IdentCollector {
    names: std::collections::HashSet<String>,
}
impl<'ast> syn::visit::Visit<'ast> for IdentCollector {
    fn visit_expr_path(&mut self, p: &'ast syn::ExprPath) {
        if let Some(id) = p.path.get_ident() {
            self.names.insert(id.to_string());
        }
        syn::visit::visit_expr_path(self, p);
    }
}

/// Every bare identifier a `syn::Expr` references, e.g. `dense_idx` and
/// `count` from `dense_idx >= count`. Used to scope assert_scope_type_bounds
/// down to the variables a check actually depends on, rather than every
/// local in the function -- see assert_scope_type_bounds for why that
/// distinction is load-bearing, not just tidiness.
fn collect_ident_names(expr: &syn::Expr) -> std::collections::HashSet<String> {
    use syn::visit::Visit;
    let mut c = IdentCollector { names: std::collections::HashSet::new() };
    c.visit_expr(expr);
    c.names
}

fn collect_ident_names_from<'e>(
    exprs: impl IntoIterator<Item = &'e syn::Expr>,
) -> std::collections::HashSet<String> {
    let mut all = std::collections::HashSet::new();
    for e in exprs {
        all.extend(collect_ident_names(e));
    }
    all
}

/// Assert type-derived bounds for every typed value NAMED in `relevant`.
///
/// assert_type_bounds alone misses a real case: a local referenced ONLY in a
/// path condition (an `if` guard) rather than passed as an argument to the
/// call under verification. `dense_idx` in `if dense_idx >= count { return; }
/// return sub_checked(count, 1);` is exactly this -- it never appears in
/// sub_checked's own argument list, so its non-negativity was never
/// asserted, and "count >= 1" (needed for sub_checked's `b <= a`) could not
/// be derived from "dense_idx < count" without it.
///
/// Scoped to `relevant` rather than every entry in `locals`: asserting bounds
/// for variables the constraint under check does not even mention only adds
/// solver work, and on at least one existing fixture (test_bv.salt, a
/// bitvector-heavy proof) that extra work was enough to push a previously
/// UNSAT-in-budget check past the proof budget into an UNKNOWN/timeout --
/// still sound, but a real loss of what proves. Confirmed by measurement,
/// not assumed: unscoped, proof_gate regressed by exactly this fixture;
/// scoped to free variables, it does not.
fn assert_scope_type_bounds<'ctx>(
    ctx: &mut LoweringContext<'_, '_>,
    locals: &HashMap<String, (Type, crate::codegen::context::LocalKind)>,
    relevant: &std::collections::HashSet<String>,
    solver: &crate::z3_shim::Solver<'ctx>,
) {
    let entries: Vec<(String, Type)> = locals.iter()
        .filter(|(name, _)| relevant.contains(*name))
        .map(|(name, (ty, _))| (name.clone(), ty.clone()))
        .collect();
    for (name, ty) in entries {
        let val = crate::z3_shim::ast::Int::new_const(ctx.z3_ctx, name);
        assert_bound_for_type(ctx, &val, &ty, solver);
    }
}

/// The (min, max) an integer TYPE guarantees -- same cases as
/// assert_bound_for_type, but returned rather than asserted, since
/// try_elide_overflow_check needs the bound as a value to compare an
/// operation's result against, not just as a fact about one variable.
/// Kept as a small, separate duplication of the same constants rather
/// than refactoring assert_bound_for_type's signature to return them:
/// lower risk to an already-tested function, and the numbers themselves
/// never change independent of the type system.
fn type_min_max<'ctx>(ctx: &LoweringContext<'_, 'ctx>, ty: &Type) -> Option<(crate::z3_shim::ast::Int<'ctx>, crate::z3_shim::ast::Int<'ctx>)> {
    let mk = |v: i64| crate::z3_shim::ast::Int::from_i64(ctx.z3_ctx, v);
    // Same unwrap as assert_bound_for_type: Atomic<T>'s MLIR type is T's
    // own (atomicity is a load/store property, not a distinct value
    // representation), so emit_overflow_check's widen-check already
    // fires for atomic arithmetic -- this needs to match, or atomics
    // would silently never even be considered for elision.
    let ty = match ty {
        Type::Atomic(inner) => inner.as_ref(),
        other => other,
    };
    match ty {
        Type::U8 => Some((mk(0), mk(255))),
        Type::U16 => Some((mk(0), mk(65535))),
        Type::U32 => Some((mk(0), mk(4294967295))),
        Type::U64 | Type::Usize => Some((mk(0), crate::z3_shim::ast::Int::from_u64(ctx.z3_ctx, u64::MAX))),
        Type::I8 => Some((mk(-128), mk(127))),
        Type::I16 => Some((mk(-32768), mk(32767))),
        Type::I32 => Some((mk(i32::MIN as i64), mk(i32::MAX as i64))),
        Type::I64 => Some((mk(i64::MIN), mk(i64::MAX))),
        _ => None,
    }
}

/// Attempts to prove that an Add/Sub/Mul on a fixed-width integer type
/// cannot overflow, so emit_overflow_check's runtime widen-check can be
/// skipped entirely -- zero-cost, not a weaker check. Pure optimization,
/// never a rejection: if this can't prove safety (translation failure,
/// Z3 finds a real counterexample, or the budget runs out undecided),
/// the caller keeps emitting the exact runtime check it always has.
/// There is deliberately no "provably overflows -> hard error" branch --
/// an ordinary, unconstrained `a + b` has a trivial counterexample
/// (i32::MAX + 1) for nearly any function that doesn't happen to state a
/// requires bounding its inputs, so treating a found counterexample as a
/// compile error would turn most ordinary arithmetic in the language
/// into a compile failure. A provable overflow is treated exactly like
/// an undecidable one: the existing runtime check stays, silently, same
/// as if this function had never run.
///
/// Only consults this function's own `requires` clauses for now
/// (caller_preconditions) -- path_conditions, loop_assumptions and
/// scoped_facts are the same translate-and-assert pattern used in verify
/// and verify_postcondition above and would extend this the same way,
/// just not needed for the cases this was built to cover yet.
pub(crate) fn try_elide_overflow_check(
    ctx: &mut LoweringContext<'_, '_>,
    b: &syn::ExprBinary,
    common_ty: &Type,
    local_vars: &HashMap<String, (Type, crate::codegen::context::LocalKind)>,
) -> bool {
    let Some((type_min, type_max)) = type_min_max(ctx, common_ty) else { return false };

    let Ok(lhs_z3) = crate::codegen::expr::translate_to_z3(ctx, &b.left, local_vars) else { return false };
    let Ok(rhs_z3) = crate::codegen::expr::translate_to_z3(ctx, &b.right, local_vars) else { return false };

    let solver = crate::z3_shim::Solver::new(ctx.z3_ctx);
    let mut solver_params = crate::z3_shim::Params::new(ctx.z3_ctx);
    solver_params.set_u32("rlimit", Z3_PROOF_RLIMIT);
    solver.set_params(&solver_params);

    assert_bound_for_type(ctx, &lhs_z3, common_ty, &solver);
    assert_bound_for_type(ctx, &rhs_z3, common_ty, &solver);

    let sym_ctx = SymbolicContext::new(ctx.z3_ctx);
    let caller_pcs = ctx.emission.caller_preconditions.clone();
    for pc in &caller_pcs {
        let Some(actual_pc) = unwrap_contract_expr(pc) else { continue };
        if let Ok(z3_pc) = crate::codegen::expr::translate_bool_to_z3(ctx, actual_pc, local_vars, &sym_ctx) {
            solver.assert(&z3_pc);
        }
    }

    let result = match b.op {
        syn::BinOp::Add(_) => &lhs_z3 + &rhs_z3,
        syn::BinOp::Sub(_) => &lhs_z3 - &rhs_z3,
        syn::BinOp::Mul(_) => &lhs_z3 * &rhs_z3,
        _ => return false,
    };

    // Negation of "no overflow": does a satisfying assignment exist
    // where the operation's true mathematical result falls outside the
    // type's range? UNSAT means no such assignment exists anywhere in
    // the search space -- overflow is impossible, not just untested.
    let out_of_range = crate::z3_shim::ast::Bool::or(ctx.z3_ctx, &[
        &result.lt(&type_min),
        &result.gt(&type_max),
    ]);
    solver.assert(&out_of_range);

    *ctx.total_checks += 1;
    let elided = matches!(solver.check(), crate::z3_shim::SatResult::Unsat);
    if elided {
        *ctx.elided_checks += 1;
    }
    elided
}

/// Emit a runtime assertion for an `ensures` clause that Z3 could not
/// resolve (timeout, or a SAT result attributable to an untracked local
/// rather than a genuine violation -- see the two call sites).
///
/// Mirrors emit_requires_runtime_check exactly, including its tradeoff:
/// return_expr is emitted FRESH here rather than reusing the value already
/// computed at the real return site, so a return expression with side
/// effects is evaluated twice on this path. Accepted for the same reason
/// emit_requires_runtime_check accepts it for arg_exprs -- this only runs
/// when Z3 could not decide, not on every return.
fn emit_ensures_runtime_check(
    ctx: &mut LoweringContext<'_, '_>,
    out: &mut String,
    ens: &syn::Expr,
    return_expr: &syn::Expr,
    local_vars: &HashMap<String, (Type, crate::codegen::context::LocalKind)>,
    return_ty: &Type,
) -> Result<(), String> {
    let mut temp_locals = local_vars.clone();

    // Emit the return expression fresh to get a real `result` value.
    let (result_val, _) = crate::codegen::expr::emit_expr(
        ctx, out, return_expr, &mut temp_locals, Some(return_ty),
    )?;
    temp_locals.insert(
        "result".to_string(),
        (return_ty.clone(), crate::codegen::context::LocalKind::SSA(result_val)),
    );

    // Emit the ensures clause as an MLIR boolean expression.
    let (ens_val, _) = crate::codegen::expr::emit_expr(
        ctx, out, ens, &mut temp_locals, Some(&Type::Bool),
    )?;

    // Emit runtime violation check: scf.if violated { call @__salt_contract_violation() }
    let true_val = format!("%verify_true_{}", ctx.emission.next_id());
    let violated = format!("%verify_violated_{}", ctx.emission.next_id());
    out.push_str(&format!("    {} = arith.constant true\n", true_val));
    out.push_str(&format!("    {} = arith.xori {}, {} : i1\n", violated, ens_val, true_val));
    ctx.ensure_func_declared("__salt_contract_violation", &[], &Type::Unit).ok();
    out.push_str(&format!("    scf.if {} {{\n", violated));
    out.push_str("      func.call @__salt_contract_violation() : () -> ()\n");
    out.push_str("      scf.yield\n");
    out.push_str("    }\n");
    Ok(())
}

/// Emit a runtime assertion for a `requires` clause that Z3 couldn't prove.
/// Evaluates the clause as an MLIR boolean expression and calls
/// `__salt_contract_violation` at runtime when the condition is false.
fn emit_requires_runtime_check(
    ctx: &mut LoweringContext<'_, '_>,
    out: &mut String,
    req: &syn::Expr,
    params: &[String],
    arg_exprs: &[syn::Expr],
    local_vars: &mut HashMap<String, (Type, crate::codegen::context::LocalKind)>,
    param_tys: &[Type],
) -> Result<(), String> {
    // Build temporary local bindings: parameter names -> argument SSA values
    let mut temp_locals = local_vars.clone();
    for (i, p_name) in params.iter().enumerate() {
        if let Some(arg_expr) = arg_exprs.get(i) {
            let (val, ty) = crate::codegen::expr::emit_expr(
                ctx, out, arg_expr, local_vars, param_tys.get(i),
            )?;
            temp_locals.insert(p_name.clone(), (ty, crate::codegen::context::LocalKind::SSA(val)));
        }
    }

    // Emit the requires clause as an MLIR boolean expression
    let (req_val, _) = crate::codegen::expr::emit_expr(
        ctx, out, req, &mut temp_locals, Some(&Type::Bool),
    )?;

    // Emit runtime violation check: scf.if violated { call @__salt_contract_violation() }
    let true_val = format!("%verify_true_{}", ctx.emission.next_id());
    let violated = format!("%verify_violated_{}", ctx.emission.next_id());
    out.push_str(&format!("    {} = arith.constant true\n", true_val));
    out.push_str(&format!("    {} = arith.xori {}, {} : i1\n", violated, req_val, true_val));
    ctx.ensure_func_declared("__salt_contract_violation", &[], &Type::Unit).ok();
    out.push_str(&format!("    scf.if {} {{\n", violated));
    out.push_str("      func.call @__salt_contract_violation() : () -> ()\n");
    out.push_str("      scf.yield\n");
    out.push_str("    }\n");
    Ok(())
}

