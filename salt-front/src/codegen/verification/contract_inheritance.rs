//! Stage-2 contract-inheritance refinement (roadmap item 7).
//!
//! Both clause sets are translated into one symbolic world (fresh consts
//! per method parameter) and checked with Z3:
//!
//! - requires: the override weakens iff some state satisfies every
//!   override clause while violating a given default clause. Checked
//!   per default clause so multi-clause disjunction stays expressible.
//! - ensures: under the default's contract, an override ensured property
//!   whose negation is satisfiable alongside the default's remaining
//!   clauses means the override weakened its postcondition.
//!
//! Solver UNKNOWN and untranslatable clauses defer conservatively
//! (pass), mirroring the house fallback for runtime checks.

use std::collections::HashMap;

use crate::codegen::context::LoweringContext;
use crate::codegen::trait_defaults::OverrideObligation;
use crate::types::Type;

type Locals = HashMap<String, (Type, crate::codegen::context::LocalKind)>;

/// Checks one override obligation semantically. Errors cite the trait
/// and method; solver UNKNOWN or untranslatable clauses defer.
pub fn check_override_conformance(
    ctx: &mut LoweringContext<'_, '_>,
    obligation: &OverrideObligation,
) -> Result<(), String> {
    if ctx.config.no_verify || obligation.params.is_empty() {
        return Ok(());
    }
    let sym_ctx = crate::codegen::verification::SymbolicContext::new(ctx.z3_ctx);
    let locals: Locals = obligation.params.iter()
        .map(|p| (p.clone(), (Type::Unit, crate::codegen::context::LocalKind::SSA(p.clone()))))
        .collect();

    // Requires side: per default clause -- assume every override clause,
    // negate the default clause; SAT means the override admits states
    // the default forbade (weakened precondition).
    for i in 0..obligation.default_requires.len() {
        ctx.z3_solver.push();
        for o in &obligation.ovr_requires {
            if let Ok(b) = crate::codegen::expr::translate_bool_to_z3(ctx, o, &locals, &sym_ctx) {
                ctx.z3_solver.assert(&b);
            }
        }
        if let Some(d) = obligation.default_requires.get(i) {
            if let Ok(b) = crate::codegen::expr::translate_bool_to_z3(ctx, d, &locals, &sym_ctx) {
                ctx.z3_solver.assert(&b.not());
                if ctx.z3_solver.check() == crate::z3_shim::SatResult::Sat {
                    ctx.z3_solver.pop(1);
                    return Err(weaken_error(obligation, "requires", i + 1));
                }
            }
        }
        ctx.z3_solver.pop(1);
    }

    // Ensures side: under the default's full contract, an override
    // ensured property whose negation remains satisfiable means the
    // override no longer delivers what the default promised.
    for i in 0..obligation.ovr_ensures.len() {
        ctx.z3_solver.push();
        for r in &obligation.default_requires {
            if let Ok(b) = crate::codegen::expr::translate_bool_to_z3(ctx, r, &locals, &sym_ctx) {
                ctx.z3_solver.assert(&b);
            }
        }
        for e in &obligation.default_ensures {
            if let Ok(b) = crate::codegen::expr::translate_bool_to_z3(ctx, e, &locals, &sym_ctx) {
                ctx.z3_solver.assert(&b);
            }
        }
        if let Some(o) = obligation.ovr_ensures.get(i) {
            if let Ok(b) = crate::codegen::expr::translate_bool_to_z3(ctx, o, &locals, &sym_ctx) {
                ctx.z3_solver.assert(&b.not());
                if ctx.z3_solver.check() == crate::z3_shim::SatResult::Sat {
                    ctx.z3_solver.pop(1);
                    return Err(weaken_error(obligation, "ensures", i + 1));
                }
            }
        }
        ctx.z3_solver.pop(1);
    }
    Ok(())
}

fn weaken_error(obligation: &OverrideObligation, kind: &str, index: usize) -> String {
    crate::errors::coded(
        "E009",
        format!(
            "override of trait `{}` method `{}` weakens {} clause #{}; \
             overrides must satisfy or strengthen the default contract",
            obligation.trait_name, obligation.method, kind, index
        ),
    )
}
