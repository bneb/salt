// Constant folder for Z3 contract expressions.
//
// Before a requires/ensures expression is translated to Z3, this pass
// evaluates sub-expressions using the compiler's built-in Evaluator.
// The Evaluator handles integer/float/bool literals, binary ops,
// comparisons, and path lookups via a constant table.
//
// This pass adds MethodCall resolution (.length() → int) which the
// Evaluator does not handle, then delegates everything else.

use std::collections::HashMap;
use crate::evaluator::{ConstValue, Evaluator};

/// Resolve an expression to a concrete ConstValue, or return None
/// if it depends on symbolic (runtime) values.
pub fn try_eval(
    expr: &syn::Expr,
    known_lengths: &HashMap<String, i64>,
    params: &[String],
    arg_exprs: &[syn::Expr],
) -> Option<ConstValue> {
    // Build constant table from known argument values
    let mut constant_table: HashMap<String, ConstValue> = HashMap::new();
    for (param, &len) in known_lengths {
        constant_table.insert(param.clone(), ConstValue::Integer(len));
    }

    // Substitute parameters with compile-time-known argument values
    let mut substituted = expr.clone();
    for (i, param) in params.iter().enumerate() {
        if i < arg_exprs.len() {
            if let Some(value) = arg_to_const(&arg_exprs[i]) {
                substituted = substitute_param(&substituted, param, &arg_exprs[i]);
                // Also insert into constant table for Evaluator path lookups
                if let ConstValue::Integer(n) = value {
                    constant_table.insert(param.clone(), ConstValue::Integer(n));
                }
            }
        }
    }

    let evaluator = Evaluator {
        depth_limit: 100,
        constant_table,
    };

    // Resolve .length() and string content methods to literals
    let resolved = resolve_methods(&substituted, known_lengths);

    // Resolve .len field accesses to known lengths (e.g., self.len → 10)
    let resolved = resolve_fields(&resolved, known_lengths);

    // Resolve calls to known std.contracts.bounds predicates when
    // all arguments are compile-time integer constants.
    if let Some(result) = try_eval_contract_predicate(&resolved) {
        return Some(result);
    }

    // Use the Evaluator for everything else
    evaluator.eval_expr(&resolved).ok()
}

/// Convert an argument expression to a ConstValue if it's a literal.
fn arg_to_const(expr: &syn::Expr) -> Option<ConstValue> {
    match expr {
        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(li), .. }) => {
            Some(ConstValue::Integer(li.base10_parse::<i64>().ok()?))
        }
        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => {
            Some(ConstValue::String(s.value()))
        }
        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Bool(b), .. }) => {
            Some(ConstValue::Bool(b.value))
        }
        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Float(lf), .. }) => {
            Some(ConstValue::Float(lf.base10_parse::<f64>().ok()?))
        }
        _ => None,
    }
}

/// Walk an expression tree, calling `f` on each node.
///
/// If `f` returns `Some(expr)`, that expr replaces the node and walk stops.
/// If `f` returns `None`, the walker recurses into children and reconstructs.
fn walk_expr(expr: &syn::Expr, f: &mut impl FnMut(&syn::Expr) -> Option<syn::Expr>) -> syn::Expr {
    if let Some(replacement) = f(expr) {
        return replacement;
    }
    match expr {
        syn::Expr::Binary(b) => syn::Expr::Binary(syn::ExprBinary {
            attrs: b.attrs.clone(),
            left: Box::new(walk_expr(&b.left, f)),
            op: b.op,
            right: Box::new(walk_expr(&b.right, f)),
        }),
        syn::Expr::Paren(p) => syn::Expr::Paren(syn::ExprParen {
            attrs: p.attrs.clone(),
            paren_token: p.paren_token,
            expr: Box::new(walk_expr(&p.expr, f)),
        }),
        syn::Expr::Group(g) => syn::Expr::Group(syn::ExprGroup {
            attrs: g.attrs.clone(),
            group_token: g.group_token,
            expr: Box::new(walk_expr(&g.expr, f)),
        }),
        syn::Expr::Unary(u) => syn::Expr::Unary(syn::ExprUnary {
            attrs: u.attrs.clone(),
            op: u.op,
            expr: Box::new(walk_expr(&u.expr, f)),
        }),
        syn::Expr::Block(block) => {
            if let Some(syn::Stmt::Expr(inner, semi)) = block.block.stmts.first() {
                let folded = walk_expr(inner, f);
                let mut new_block = block.clone();
                new_block.block.stmts[0] = syn::Stmt::Expr(folded, *semi);
                syn::Expr::Block(new_block)
            } else {
                expr.clone()
            }
        }
        syn::Expr::Let(let_expr) => syn::Expr::Let(syn::ExprLet {
            attrs: let_expr.attrs.clone(),
            let_token: let_expr.let_token,
            pat: let_expr.pat.clone(),
            eq_token: let_expr.eq_token,
            expr: Box::new(walk_expr(&let_expr.expr, f)),
        }),
        syn::Expr::MethodCall(mc) => syn::Expr::MethodCall(syn::ExprMethodCall {
            attrs: mc.attrs.clone(),
            receiver: Box::new(walk_expr(&mc.receiver, f)),
            dot_token: mc.dot_token,
            method: mc.method.clone(),
            turbofish: mc.turbofish.clone(),
            paren_token: mc.paren_token,
            args: mc.args.iter().map(|a| walk_expr(a, f)).collect(),
        }),
        syn::Expr::Field(field) => syn::Expr::Field(syn::ExprField {
            attrs: field.attrs.clone(),
            base: Box::new(walk_expr(&field.base, f)),
            dot_token: field.dot_token,
            member: field.member.clone(),
        }),
        syn::Expr::Call(call) => syn::Expr::Call(syn::ExprCall {
            attrs: call.attrs.clone(),
            func: Box::new(walk_expr(&call.func, f)),
            paren_token: call.paren_token,
            args: call.args.iter().map(|a| walk_expr(a, f)).collect(),
        }),
        _ => expr.clone(),
    }
}

/// Substitute a parameter reference with its argument expression in the AST.
fn substitute_param(expr: &syn::Expr, param: &str, arg: &syn::Expr) -> syn::Expr {
    walk_expr(expr, &mut |e| {
        if let syn::Expr::Path(p) = e {
            if p.path.get_ident().is_some_and(|i| i == param) {
                return Some(arg.clone());
            }
        }
        None
    })
}

/// Extract a string literal value from an expression, if possible.
fn string_literal_value(expr: &syn::Expr) -> Option<String> {
    if let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) = expr {
        Some(s.value())
    } else {
        None
    }
}

/// Resolve .length(), .len(), .contains(), .starts_with(), .ends_with()
/// method calls to integer or boolean literals.
fn resolve_methods(expr: &syn::Expr, known_lengths: &HashMap<String, i64>) -> syn::Expr {
    walk_expr(expr, &mut |e| {
        if let syn::Expr::MethodCall(mc) = e {
            if let Some(v) = try_resolve_len(&mc.receiver, &mc.method, known_lengths) {
                return Some(v);
            }
            if let Some(v) = try_resolve_content(&mc.receiver, &mc.method, &mc.args) {
                return Some(v);
            }
            if let Some(v) = try_resolve_regex(&mc.receiver, &mc.method, &mc.args) {
                return Some(v);
            }
        }
        None
    })
}

fn try_resolve_len(
    receiver: &syn::Expr,
    method: &syn::Ident,
    known_lengths: &HashMap<String, i64>,
) -> Option<syn::Expr> {
    let method = method.to_string();
    if method != "length" && method != "len" {
        return None;
    }
    if let Some(s) = string_literal_value(receiver) {
        return Some(make_int_literal(s.len() as i64));
    }
    if let syn::Expr::Path(p) = receiver {
        if let Some(ident) = p.path.get_ident() {
            if let Some(&len) = known_lengths.get(&ident.to_string()) {
                return Some(make_int_literal(len));
            }
        }
    }
    None
}

fn try_resolve_content(
    receiver: &syn::Expr,
    method: &syn::Ident,
    args: &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
) -> Option<syn::Expr> {
    let method = method.to_string();
    if method != "contains" && method != "starts_with" && method != "ends_with" {
        return None;
    }
    if args.len() != 1 {
        return None;
    }
    if let (Some(receiver), Some(arg)) = (
        string_literal_value(receiver),
        string_literal_value(&args[0]),
    ) {
        let result = match method.as_str() {
            "contains" => receiver.contains(&arg),
            "starts_with" => receiver.starts_with(&arg),
            "ends_with" => receiver.ends_with(&arg),
            _ => unreachable!(),
        };
        return Some(make_bool_literal(result));
    }
    None
}

fn try_resolve_regex(
    receiver: &syn::Expr,
    method: &syn::Ident,
    args: &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
) -> Option<syn::Expr> {
    let method = method.to_string();
    if method != "matches" {
        return None;
    }
    if args.len() != 1 {
        return None;
    }
    if let (Some(text), Some(pattern)) = (
        string_literal_value(receiver),
        string_literal_value(&args[0]),
    ) {
        let result = regex::Regex::new(&pattern)
            .map(|re| re.is_match(&text))
            .unwrap_or(false);
        return Some(make_bool_literal(result));
    }
    None
}

/// Resolve .len field accesses (e.g., self.len) to integer literals
/// using the known_lengths map. This is the field-access counterpart to
/// resolve_methods, which handles .len() method calls.
fn resolve_fields(expr: &syn::Expr, known_lengths: &HashMap<String, i64>) -> syn::Expr {
    walk_expr(expr, &mut |e| {
        if let syn::Expr::Field(f) = e {
            if let syn::Member::Named(id) = &f.member {
                if id == "len" {
                    if let syn::Expr::Path(p) = &*f.base {
                        if let Some(ident) = p.path.get_ident() {
                            if let Some(&len) = known_lengths.get(&ident.to_string()) {
                                return Some(make_int_literal(len));
                            }
                        }
                    }
                }
            }
        }
        None
    })
}

fn make_int_literal(val: i64) -> syn::Expr {
    syn::Expr::Lit(syn::ExprLit {
        attrs: vec![],
        lit: syn::Lit::Int(syn::LitInt::new(&val.to_string(), proc_macro2::Span::call_site())),
    })
}

/// Evaluate calls to known contract library predicates when all arguments
/// are compile-time integer constants. Returns `None` if the expression is
/// not a contract predicate call or if any argument is symbolic.
///
/// Currently supports `std.contracts.bounds.{in_bounds, in_range, positive, non_negative}`.
fn try_eval_contract_predicate(expr: &syn::Expr) -> Option<ConstValue> {
    // Unwrap parentheses and groups
    let inner = match expr {
        syn::Expr::Paren(p) => &p.expr,
        syn::Expr::Group(g) => &g.expr,
        other => other,
    };
    match inner {
        syn::Expr::Call(call) => {
            let fn_name = if let syn::Expr::Path(p) = &*call.func {
                p.path.segments.last()?.ident.to_string()
            } else {
                return None;
            };
            // Extract all arguments as integer literals
            let mut args: Vec<i64> = Vec::new();
            for arg in &call.args {
                if let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(li), .. }) = arg {
                    args.push(li.base10_parse::<i64>().ok()?);
                } else {
                    return None; // non-constant argument
                }
            }
            match fn_name.as_str() {
                "in_range" if args.len() == 3 => {
                    Some(ConstValue::Bool(args[1] <= args[0] && args[0] < args[2]))
                }
                "in_bounds" if args.len() == 2 => {
                    Some(ConstValue::Bool(0 <= args[0] && args[0] < args[1]))
                }
                "positive" if args.len() == 1 => {
                    Some(ConstValue::Bool(args[0] > 0))
                }
                "non_negative" if args.len() == 1 => {
                    Some(ConstValue::Bool(args[0] >= 0))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn make_bool_literal(val: bool) -> syn::Expr {
    syn::Expr::Lit(syn::ExprLit {
        attrs: vec![],
        lit: syn::Lit::Bool(syn::LitBool::new(val, proc_macro2::Span::call_site())),
    })
}
