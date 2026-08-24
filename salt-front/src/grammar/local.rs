//! Mutable local declarations written with the Salt 'var' keyword.
//!
//! At item level 'var' declares a global (see GlobalDef); inside a function
//! body it declares a MUTABLE local. The declaration is desugared into an
//! ordinary 'let mut' statement so lowering sees a familiar Rust local.

use quote::quote;
use syn::{Ident, Token};
use syn::parse::ParseStream;

use super::{Stmt, SynType, parse_user_ident};
use crate::keywords::var;

/// Parse one 'var' local declaration:
///
///   var total = a + b;
///   var pos: i64 = 0;
pub(crate) fn parse_var_stmt(input: ParseStream) -> syn::Result<Stmt> {
    input.parse::<var>()?;
    let name: Ident = parse_user_ident(input)?;
    let ty: Option<SynType> = if input.peek(Token![:]) {
        input.parse::<Token![:]>()?;
        Some(input.parse()?)
    } else {
        None
    };
    input.parse::<Token![=]>()?;
    let init = input.parse::<syn::Expr>()?;
    input.parse::<Token![;]>()?;
    Ok(Stmt::Syn(desugar_to_local(&name, ty.as_ref(), &init, &name)?))
}

/// Build 'let mut name(: Ty)? = init;' as a native syn statement.
fn desugar_to_local(
    name: &Ident,
    ty: Option<&SynType>,
    init: &syn::Expr,
    span: &Ident,
) -> syn::Result<syn::Stmt> {
    let ty_tokens = match ty {
        Some(t) => quote!(: #t),
        None => quote!(),
    };
    syn::parse2(quote!(let mut #name #ty_tokens = #init;))
        .map_err(|e| syn::Error::new(span.span(), format!(
            "invalid var declaration: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::{Item, SaltFile};

    /// Returns (is_mut, has_type_annotation) of the first stmt of fn f.
    fn first_var_shape(src: &str) -> (bool, bool) {
        let file: SaltFile = syn::parse_str(src).expect("parse");
        let stmt = match &file.items[0] {
            Item::Fn(f) => f.body.stmts[0].clone(),
            other => panic!("expected fn item at top level, got {other:?}"),
        };
        let local = match stmt {
            Stmt::Syn(syn::Stmt::Local(local)) => local,
            other => panic!("expected Local stmt, got {other:?}"),
        };
        match &local.pat {
            syn::Pat::Type(pt) => match &*pt.pat {
                syn::Pat::Ident(pi) => (pi.mutability.is_some(), true),
                p => panic!("expected ident under Pat::Type, got {p:?}"),
            },
            syn::Pat::Ident(pi) => (pi.mutability.is_some(), false),
            p => panic!("expected ident pattern, got {p:?}"),
        }
    }

    #[test]
    fn typed_var_becomes_mutable_local() {
        let (is_mut, has_ty) = first_var_shape("fn f() { var pos: i64 = 0; }");
        assert!(is_mut, "var must imply mut");
        assert!(has_ty, "type annotation must survive");
    }

    #[test]
    fn untyped_var_gets_inferred_type() {
        let (is_mut, has_ty) = first_var_shape("fn f() { var step = 3; }");
        assert!(is_mut);
        assert!(!has_ty);
    }

    #[test]
    fn var_without_initializer_is_rejected() {
        let result = syn::parse_str::<SaltFile>("fn f() { var x; }");
        assert!(result.is_err(), "var requires an initializer");
    }
}
