//! Compile-time wiring macros for the axiom constitution layer.
//!
//! `wire!` declares a causal flow `Source => Sink` (or a chain
//! `A => B => C ...`) and expands to an inline driver closure — the exact
//! code a hand-written composition would contain (T7 zero-cost), plus a
//! per-edge pairing witness (`Conforms<Wire<A, B>>`) checked at compile time.
//!
//! **Contract face unchanged**: the macro generates only existing core
//! vocabulary (`PortCell::step`, `Conforms`, `Wire`). It is syntax sugar,
//! not a second vocabulary source.
//!
//! **Readable diagnostics**: each edge's pairing witness is spanned at the
//! user's `=>` arrow (via `quote_spanned`), so a type mismatch (`B::In !=
//! A::Out`) is reported at that exact arrow in the caller's code instead of
//! inside opaque macro expansion.
//!
//! **Path contract**: expansion refers to `::axiom::` paths, which resolve in
//! the caller's extern prelude — the caller's dependency must appear under
//! the literal name `axiom`. The core itself enables this internally via
//! `extern crate self as axiom;`.

// The macro crate never links downstream; it exists only inside rustc.
#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{Token, Type};

/// One parsed stage: a port-body type plus the span of the `=>` arrow that
/// precedes the *next* stage (used to span that edge's pairing witness).
struct Stage {
    ty: Type,
    /// Span of the `=>` that follows this type; `None` for the last stage.
    arrow: Option<Span>,
}

/// Grammar: `Type (=> Type)*` — at least one stage, arrows between stages.
struct Wiring {
    stages: Vec<Stage>,
}

impl Parse for Wiring {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut stages = Vec::new();
        loop {
            let ty: Type = input.parse()?;
            if input.peek(Token![=>]) {
                let arrow = input.parse::<Token![=>]>()?;
                stages.push(Stage { ty, arrow: Some(arrow.span()) });
            } else {
                stages.push(Stage { ty, arrow: None });
                break;
            }
        }
        Ok(Wiring { stages })
    }
}

/// Wire a causal flow through one or more port bodies.
///
/// - `wire!(A => B)` expands to `Fn(&mut A::State, &mut B::State, A::In) -> B::Out`
///   (identical to the former declarative macro; states in chain order).
/// - `wire!(A => B => C)` chains further: `Fn(&mut A::State, &mut B::State,
///   &mut C::State, A::In) -> C::Out`.
///
/// Each edge carries a compile-time pairing witness (`B::In == A::Out`,
/// via the unified `Conforms` criterion); a violation is reported at the
/// offending `=>` arrow in the caller's source.
#[proc_macro]
pub fn wire(input: TokenStream) -> TokenStream {
    let wiring = syn::parse_macro_input!(input as Wiring);
    let n = wiring.stages.len();
    let states: Vec<syn::Ident> = (0..n)
        .map(|i| quote::format_ident!("state{i}"))
        .collect();
    let mids: Vec<syn::Ident> = (0..n.saturating_sub(1))
        .map(|i| quote::format_ident!("mid{i}"))
        .collect();

    // Closure parameters: one `&mut State` per stage, then the input value.
    let params = wiring.stages.iter().zip(&states).map(|(s, id)| {
        let ty = &s.ty;
        quote! { #id: &mut <#ty as ::axiom::cell_core::PortCell>::State, }
    });
    let first = &wiring.stages[0].ty;
    let last = &wiring.stages[n - 1].ty;
    let input_param = quote! { input: <#first as ::axiom::cell_core::PortCell>::In };

    // Per-edge pairing witness, spanned at the caller's arrow so a mismatch
    // is reported there. Runs at monomorphization: the witness is a const
    // `bool` binding and compiles to nothing.
    let witnesses = wiring.stages[..n - 1].iter().enumerate().map(|(i, s)| {
        let from = &s.ty;
        let to = &wiring.stages[i + 1].ty;
        let arrow = s.arrow.unwrap_or_else(Span::call_site);
        quote_spanned! { arrow =>
            let _: bool = <() as ::axiom::cell_core::Conforms<
                ::axiom::cell_core::Wire<#from, #to>
            >>::OK;
        }
    });

    // Inline step chain: `v0 = A0::step(state0, input)`, …, tail expression
    // `An::step(stateN, vN-1)`. Exactly the hand-written composition (T7).
    let steps = wiring.stages.iter().enumerate().map(|(i, s)| {
        let ty = &s.ty;
        let state = &states[i];
        if i == 0 {
            let out = &mids[0];
            quote! { let #out: <#ty as ::axiom::cell_core::PortCell>::Out =
                <#ty as ::axiom::cell_core::PortCell>::step(#state, input); }
        } else if i < n - 1 {
            let src = &mids[i - 1];
            let out = &mids[i];
            quote! { let #out: <#ty as ::axiom::cell_core::PortCell>::Out =
                <#ty as ::axiom::cell_core::PortCell>::step(#state, #src); }
        } else {
            let src = &mids[i - 1];
            quote! { <#ty as ::axiom::cell_core::PortCell>::step(#state, #src) }
        }
    });

    let expanded = quote! {{
        | #(#params)* #input_param| -> <#last as ::axiom::cell_core::PortCell>::Out {
            #(#witnesses)*
            #(#steps)*
        }
    }};
    expanded.into()
}
