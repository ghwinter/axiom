//! # axiom-derive
//!
//! 过程宏，从单一 struct 声明自动生成 axiom 端口样板代码。
//!
//! ## `#[ports]` 属性宏
//!
//! 在一个带泛型的 unit struct 上标注 `#[ports]`，用字段声明端口，
//! 宏自动生成：
//!
//! - `Input` 枚举（所有 `#[in]` 字段）
//! - `Output` 枚举（所有 `#[out]` 字段）
//! - `HasPortInfo` impl × 2
//! - `PortSet` impl
//!
//! ### 语法
//!
//! ```ignore
//! #[axiom::ports]
//! pub struct IdentityPorts<I> {
//!     #[in] input: I,
//!     #[out] output: I,
//! }
//! ```
//!
//! ### 端口属性
//!
//! - `#[in]` — 输入端口。可选指定 FlowKind：`#[in(Control)]`（默认 `Data`）。
//! - `#[out]` — 输出端口。同上。
//! - 端口名 = 字段名；枚举变体名 = 字段名 PascalCase 化。
//!
//! ### 泛型
//!
//! 宏保留 struct 的泛型参数，传播到生成的枚举和 impl。所有 impl 统一
//! 添加 `Send + Sync + 'static` 约束（`PortSet: Send + Sync + 'static` 的要求）。
//!
//! ### 空端口
//!
//! 无 `#[in]` 字段 → 生成零变体 `Input` 枚举（uninhabited）。
//! 无 `#[out]` 字段 → 生成零变体 `Output` 枚举（uninhabited）。
//!
//! 这与 builtin 的 `SinkOutput {}` 模式一致——`ProcessOutput::Yield` 无法构造。

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, Attribute, DeriveInput, Expr, Fields, GenericParam, Generics, Ident,
    Meta,
};

// ════════════════════════════════════════════════════════════════════════════
// 端口声明解析
// ════════════════════════════════════════════════════════════════════════════

/// 一个端口的声明信息。
struct PortDecl {
    /// 端口名（字段名，snake_case）。
    name: String,
    /// 枚举变体名（字段名 PascalCase 化）。
    variant: Ident,
    /// 端口类型。
    ty: syn::Type,
    /// FlowKind，默认 "Data"。
    flow: String,
}

/// 从字段属性解析 `#[in]` / `#[out]` 及可选的 FlowKind。
///
/// 返回 `Some((dir, flow))` 或 `None`（字段无端口属性）。
fn parse_port_attr(attrs: &[Attribute]) -> Option<(String, String)> {
    for attr in attrs {
        let path = attr.path();
        if path.is_ident("input") || path.is_ident("output") {
            let dir = if path.is_ident("input") { "in" } else { "out" };
            // 解析可选的 FlowKind：#[in(Data)] / #[in(Control)] / #[in(Observe)]
            let flow = match &attr.meta {
                Meta::Path(_) => "Data".to_string(),
                Meta::List(meta_list) => {
                    let tokens = meta_list.tokens.to_string();
                    // tokens 形如 "Data" / "Control" / "Observe"
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

/// 把 snake_case 字段名转为 PascalCase 枚举变体名。
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
// 代码生成
// ════════════════════════════════════════════════════════════════════════════

/// 提取泛型参数的 ident 列表（仅类型参数，跳过生命周期）。
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

/// 生成统一的 where 子句约束：`T: Send + Sync + 'static` for each type param.
fn where_clause(generics: &Generics) -> TokenStream2 {
    let idents: Vec<Ident> = generic_idents(generics);
    if idents.is_empty() {
        quote! {}
    } else {
        quote! { where #(#idents: Send + Sync + 'static),* }
    }
}

/// 生成泛型参数列表（`<I, O>`）。
fn generic_params(generics: &Generics) -> TokenStream2 {
    if generics.params.is_empty() {
        quote! {}
    } else {
        let params = &generics.params;
        quote! { <#params> }
    }
}

/// 为一组端口声明生成枚举。
///
/// 空列表生成零变体枚举（uninhabited）。
fn generate_enum(name: &Ident, generics: &Generics, ports: &[PortDecl]) -> TokenStream2 {
    let gp = generic_params(generics);
    let wc = where_clause(generics);

    if ports.is_empty() {
        // 零变体枚举——uninhabited type。
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

/// 为一组端口声明生成 `HasPortInfo` impl。
fn generate_has_port_info(
    enum_name: &Ident,
    generics: &Generics,
    ports: &[PortDecl],
) -> TokenStream2 {
    let gp = generic_params(generics);
    let wc = where_clause(generics);

    if ports.is_empty() {
        // 零变体枚举——所有方法 match *self {}（unreachable）。
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

/// 生成 `PortSet` impl。
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

    // PortSchema 构建
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
// #[ports] 属性宏入口
// ════════════════════════════════════════════════════════════════════════════

#[proc_macro_attribute]
pub fn ports(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);

    let struct_name = &input.ident;
    let generics = &input.generics;
    let vis = &input.vis;
    let attrs = &input.attrs;

    // 解析字段
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

    // 生成枚举名：去掉 "Ports" 后缀 + "Input" / "Output"
    // 例如 IdentityPorts → IdentityInput / IdentityOutput
    let base_name = struct_name.to_string();
    let base_name = base_name
        .strip_suffix("Ports")
        .unwrap_or(&base_name);
    let input_enum_name = format_ident!("{}Input", base_name);
    let output_enum_name = format_ident!("{}Output", base_name);

    let gp = generic_params(generics);

    // 生成各部分
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

    // 原始 struct 改为 PhantomData unit struct（保留泛型和可见性）
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
