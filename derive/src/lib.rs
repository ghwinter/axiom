//! # axiom-derive
//!
//! Procedural macros that auto-generate axiom port boilerplate from a single struct declaration.
//!
//! ## `#[ports]` attribute macro
//!
//! Annotate a unit struct with generics using `#[ports]`, declaring ports via fields;
//! the macro auto-generates:
//!
//! - an `Input` enum (all `#[in]` fields)
//! - an `Output` enum (all `#[out]` fields)
//! - `HasPortInfo` impl × 2
//! - `PortSet` impl
//!
//! ### Syntax
//!
//! ```ignore
//! #[axiom::ports]
//! pub struct IdentityPorts<I> {
//!     #[in] input: I,
//!     #[out] output: I,
//! }
//! ```
//!
//! ### Port attributes
//!
//! - `#[in]` — an input port. Optionally specify a FlowKind: `#[in(Control)]` (defaults to `Data`).
//! - `#[out]` — an output port. Same as above.
//! - Port name = field name; enum variant name = the field name PascalCased.
//!
//! ### Generics
//!
//! The macro preserves the struct's generic parameters, propagating them into the generated enums and impls.
//! All impls uniformly add the `Send + Sync + 'static` bound (the `PortSet: Send + Sync + 'static` requirement).
//!
//! ### Empty ports
//!
//! No `#[in]` fields → a zero-variant `Input` enum is generated (uninhabited).
//! No `#[out]` fields → a zero-variant `Output` enum is generated (uninhabited).
//!
//! This matches the builtin `SinkOutput {}` pattern — `ProcessOutput::Yield` cannot be constructed.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, Attribute, DeriveInput, Expr, Fields, GenericParam, Generics, Ident,
    Meta,
};

// ════════════════════════════════════════════════════════════════════════════
// Port-declaration parsing
// ════════════════════════════════════════════════════════════════════════════

/// Declaration info for a single port.
struct PortDecl {
    /// Port name (field name, snake_case).
    name: String,
    /// Enum variant name (field name PascalCased).
    variant: Ident,
    /// Port type.
    ty: syn::Type,
    /// FlowKind, defaults to "Data".
    flow: String,
}

/// Parse `#[in]` / `#[out]` and the optional FlowKind from the field's attributes.
///
/// Returns `Some((dir, flow))` or `None` (the field has no port attribute).
fn parse_port_attr(attrs: &[Attribute]) -> Option<(String, String)> {
    for attr in attrs {
        let path = attr.path();
        if path.is_ident("input") || path.is_ident("output") {
            let dir = if path.is_ident("input") { "in" } else { "out" };
            // Parse the optional FlowKind: #[in(Data)] / #[in(Control)] / #[in(Observe)]
            let flow = match &attr.meta {
                Meta::Path(_) => "Data".to_string(),
                Meta::List(meta_list) => {
                    let tokens = meta_list.tokens.to_string();
                    // tokens look like "Data" / "Control" / "Observe"
                    tokens.trim().to_string()
                }
                Meta::NameValue(nv) => {
                    // #[in = "Data"]
                    if let Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) = &nv.value {
                        s.value()
                    } else {
                        "Data".to_string()
                    }
                }
            };
            return Some((dir.to_string(), flow));
        }
    }
    None
}

/// Convert a snake_case field name into a PascalCase enum variant name.
fn to_pascal_case(s: &str) -> Ident {
    let pascal: String = s
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect();
    format_ident!("{}", pascal)
}

// ════════════════════════════════════════════════════════════════════════════
// Code generation
// ════════════════════════════════════════════════════════════════════════════

/// Extract the ident list of the generic parameters (type parameters only, skipping lifetimes).
fn generic_idents(generics: &Generics) -> Vec<Ident> {
    generics
        .params
        .iter()
        .filter_map(|p| match p {
            GenericParam::Type(tp) => Some(tp.ident.clone()),
            _ => None,
        })
        .collect()
}

/// Generate a uniform where-clause constraint: `T: Send + Sync + 'static` for each type param.
fn where_clause(generics: &Generics) -> TokenStream2 {
    let idents: Vec<Ident> = generic_idents(generics);
    if idents.is_empty() {
        quote! {}
    } else {
        quote! { where #(#idents: Send + Sync + 'static),* }
    }
}

/// Generate the generic parameter list (`<I, O>`).
fn generic_params(generics: &Generics) -> TokenStream2 {
    if generics.params.is_empty() {
        quote! {}
    } else {
        let params = &generics.params;
        quote! { <#params> }
    }
}

/// Generate an enum for a group of port declarations.
///
/// An empty list produces a zero-variant enum (uninhabited).
fn generate_enum(name: &Ident, generics: &Generics, ports: &[PortDecl]) -> TokenStream2 {
    let gp = generic_params(generics);
    let wc = where_clause(generics);

    if ports.is_empty() {
        // Zero-variant enum — uninhabited type.
        quote! {
            #[derive(Debug, Clone, PartialEq)]
            pub enum #name #gp #wc {}
        }
    } else {
        let variants: Vec<TokenStream2> = ports
            .iter()
            .map(|p| {
                let v = &p.variant;
                let ty = &p.ty;
                quote! { #v(#ty) }
            })
            .collect();
        quote! {
            #[derive(Debug, Clone, PartialEq)]
            pub enum #name #gp #wc {
                #(#variants,)*
            }
        }
    }
}

/// Generate a `HasPortInfo` impl for a group of port declarations.
fn generate_has_port_info(
    enum_name: &Ident,
    generics: &Generics,
    ports: &[PortDecl],
) -> TokenStream2 {
    let gp = generic_params(generics);
    let wc = where_clause(generics);

    if ports.is_empty() {
        // Zero-variant enum — every method matches on *self {} (unreachable).
        quote! {
            impl #gp ::axiom::portset::HasPortInfo for #enum_name #gp #wc {
                fn port_name(&self) -> &'static str { match *self {} }
                fn flow_kind(&self) -> ::axiom::flow::FlowKind { match *self {} }
                fn payload_type_id(&self) -> ::core::any::TypeId { match *self {} }
                fn payload_type_name(&self) -> &'static str { match *self {} }
                fn from_port_name(_name: &str, _payload: ::alloc::boxed::Box<dyn ::core::any::Any + ::core::marker::Send>) -> ::core::option::Option<Self> { ::core::option::Option::None }
                fn into_any(self) -> ::alloc::boxed::Box<dyn ::core::any::Any + ::core::marker::Send> { match self {} }
            }
        }
    } else {
        // port_name
        let port_name_arms: Vec<TokenStream2> = ports
            .iter()
            .map(|p| {
                let v = &p.variant;
                let name = &p.name;
                quote! { Self::#v(_) => #name }
            })
            .collect();

        // flow_kind
        let flow_kind_arms: Vec<TokenStream2> = ports
            .iter()
            .map(|p| {
                let v = &p.variant;
                let flow = Ident::new(&p.flow, proc_macro2::Span::call_site());
                quote! { Self::#v(_) => ::axiom::flow::FlowKind::#flow }
            })
            .collect();

        // payload_type_id
        let type_id_arms: Vec<TokenStream2> = ports
            .iter()
            .map(|p| {
                let v = &p.variant;
                let ty = &p.ty;
                quote! { Self::#v(_) => ::core::any::TypeId::of::<#ty>() }
            })
            .collect();

        // payload_type_name
        let type_name_arms: Vec<TokenStream2> = ports
            .iter()
            .map(|p| {
                let v = &p.variant;
                let ty = &p.ty;
                quote! { Self::#v(_) => ::core::any::type_name::<#ty>() }
            })
            .collect();

        // from_port_name
        let from_arms: Vec<TokenStream2> = ports
            .iter()
            .map(|p| {
                let v = &p.variant;
                let name = &p.name;
                let ty = &p.ty;
                quote! {
                    #name => {
                        let v: ::alloc::boxed::Box<#ty> = payload.downcast().ok()?;
                        ::core::option::Option::Some(Self::#v(*v))
                    }
                }
            })
            .collect();

        // into_any
        let into_arms: Vec<TokenStream2> = ports
            .iter()
            .map(|p| {
                let v = &p.variant;
                quote! { Self::#v(v) => ::alloc::boxed::Box::new(v) }
            })
            .collect();

        quote! {
            impl #gp ::axiom::portset::HasPortInfo for #enum_name #gp #wc {
                fn port_name(&self) -> &'static str {
                    match self { #(#port_name_arms),* }
                }
                fn flow_kind(&self) -> ::axiom::flow::FlowKind {
                    match self { #(#flow_kind_arms),* }
                }
                fn payload_type_id(&self) -> ::core::any::TypeId {
                    match self { #(#type_id_arms),* }
                }
                fn payload_type_name(&self) -> &'static str {
                    match self { #(#type_name_arms),* }
                }
                fn from_port_name(name: &str, payload: ::alloc::boxed::Box<dyn ::core::any::Any + ::core::marker::Send>) -> ::core::option::Option<Self> {
                    match name { #(#from_arms),* _ => ::core::option::Option::None }
                }
                fn into_any(self) -> ::alloc::boxed::Box<dyn ::core::any::Any + ::core::marker::Send> {
                    match self { #(#into_arms),* }
                }
            }
        }
    }
}

/// Generate a `PortSet` impl.
fn generate_port_set(
    ports_name: &Ident,
    generics: &Generics,
    input_enum: &Ident,
    output_enum: &Ident,
    inputs: &[PortDecl],
    outputs: &[PortDecl],
) -> TokenStream2 {
    let gp = generic_params(generics);
    let wc = where_clause(generics);

    // PortSchema construction
    let mut schema_calls: Vec<TokenStream2> = Vec::new();
    for p in inputs {
        let name = &p.name;
        let ty = &p.ty;
        let flow = Ident::new(&p.flow, proc_macro2::Span::call_site());
        schema_calls.push(quote! {
            .with(::axiom::port::PortDecl::new::<#ty>(
                #name,
                ::axiom::port::PortDir::In,
                ::axiom::flow::FlowKind::#flow,
            ))
        });
    }
    for p in outputs {
        let name = &p.name;
        let ty = &p.ty;
        let flow = Ident::new(&p.flow, proc_macro2::Span::call_site());
        schema_calls.push(quote! {
            .with(::axiom::port::PortDecl::new::<#ty>(
                #name,
                ::axiom::port::PortDir::Out,
                ::axiom::flow::FlowKind::#flow,
            ))
        });
    }

    quote! {
        impl #gp ::axiom::portset::PortSet for #ports_name #gp #wc {
            type Input = #input_enum #gp;
            type Output = #output_enum #gp;

            fn port_schema() -> ::axiom::port::PortSchema {
                ::axiom::port::PortSchema::new()
                    #(#schema_calls)*
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// #[ports] attribute-macro entry point
// ════════════════════════════════════════════════════════════════════════════

#[proc_macro_attribute]
pub fn ports(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);

    let struct_name = &input.ident;
    let generics = &input.generics;
    let vis = &input.vis;
    let attrs = &input.attrs;

    // Parse fields
    let fields = match &input.data {
        syn::Data::Struct(s) => match &s.fields {
            Fields::Named(named) => &named.named,
            Fields::Unnamed(_) => {
                return syn::Error::new_spanned(
                    &input,
                    "#[ports] requires a struct with named fields",
                )
                .to_compile_error()
                .into();
            }
            Fields::Unit => {
                return syn::Error::new_spanned(
                    &input,
                    "#[ports] requires a struct with named fields (use named fields to declare ports)",
                )
                .to_compile_error()
                .into();
            }
        },
        _ => {
            return syn::Error::new_spanned(&input, "#[ports] can only be applied to structs")
                .to_compile_error()
                .into();
        }
    };

    let mut inputs: Vec<PortDecl> = Vec::new();
    let mut outputs: Vec<PortDecl> = Vec::new();

    for field in fields {
        let field_name = field.ident.as_ref().unwrap().to_string();
        let variant = to_pascal_case(&field_name);
        let ty = field.ty.clone();

        match parse_port_attr(&field.attrs) {
            Some((dir, flow)) => {
                let decl = PortDecl {
                    name: field_name,
                    variant,
                    ty,
                    flow,
                };
                if dir == "in" {
                    inputs.push(decl);
                } else {
                    outputs.push(decl);
                }
            }
            None => {
                return syn::Error::new_spanned(
                    field,
                    "every field in #[ports] struct must have #[in] or #[out] attribute",
                )
                .to_compile_error()
                .into();
            }
        }
    }

    // Generate the enum names: strip the "Ports" suffix + "Input" / "Output"
    // e.g. IdentityPorts → IdentityInput / IdentityOutput
    let base_name = struct_name.to_string();
    let base_name = base_name
        .strip_suffix("Ports")
        .unwrap_or(&base_name);
    let input_enum_name = format_ident!("{}Input", base_name);
    let output_enum_name = format_ident!("{}Output", base_name);

    let gp = generic_params(generics);

    // Generate each part
    let input_enum = generate_enum(&input_enum_name, generics, &inputs);
    let output_enum = generate_enum(&output_enum_name, generics, &outputs);
    let input_impl = generate_has_port_info(&input_enum_name, generics, &inputs);
    let output_impl = generate_has_port_info(&output_enum_name, generics, &outputs);
    let port_set_impl = generate_port_set(
        struct_name,
        generics,
        &input_enum_name,
        &output_enum_name,
        &inputs,
        &outputs,
    );

    // Turn the original struct into a PhantomData unit struct (preserving generics and visibility)
    let gen_idents = generic_idents(generics);
    let phantom = if gen_idents.is_empty() {
        quote! { (()) }
    } else {
        quote! { ( ::core::marker::PhantomData <(#(#gen_idents),*)> ) }
    };

    let wc = where_clause(generics);

    let expanded = quote! {
        #(#attrs)*
        #vis struct #struct_name #gp #phantom #wc;

        #input_enum
        #output_enum
        #input_impl
        #output_impl
        #port_set_impl
    };

    expanded.into()
}
