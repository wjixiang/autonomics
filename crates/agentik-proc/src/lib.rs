use std::default;

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Expr, ExprLit, Fields, Lit, Meta, Type, TypePath, parse_macro_input};

/// 解析 `#[tool(name = "...", description = "...")]` 属性，
/// 返回 (name, description)。
fn parse_tool_attr(input: &DeriveInput) -> syn::Result<(String, String)> {
    let mut tool_name = None;
    let mut tool_desc = None;

    for attr in &input.attrs {
        if !attr.path().is_ident("tool") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                let value = meta.value()?;
                let lit: Lit = value.parse()?;
                if let Lit::Str(s) = lit {
                    tool_name = Some(s.value());
                }
            } else if meta.path.is_ident("description") {
                let value = meta.value()?;
                let lit: Lit = value.parse()?;
                if let Lit::Str(s) = lit {
                    tool_desc = Some(s.value());
                }
            } else {
                return Err(meta.error("expected `name` or `description`"));
            }
            Ok(())
        })?;
    }

    let name = tool_name
        .ok_or_else(|| syn::Error::new_spanned(input, "missing `#[tool(name = \"...\")]`"))?;
    let desc = tool_desc.ok_or_else(|| {
        syn::Error::new_spanned(input, "missing `#[tool(description = \"...\")]`")
    })?;

    Ok((name, desc))
}

/// 一个字段对应的信息
struct FieldInfo {
    name: String,        // 字段名（JSON 参数名）
    type_str: String,    // JSON Schema 类型："string", "integer", "boolean", "array"
    is_required: bool,   // 非 Option<T> → true
    description: String, // 从 #[doc] 提取
    has_default: bool,
    default_tokens: Option<proc_macro2::TokenStream>,
}

/// 判断类型是否为 `Option<T>`，若是则返回内部 T；否则返回 None。
fn extract_option_inner(ty: &Type) -> Option<&Type> {
    if let Type::Path(TypePath { path, .. }) = ty {
        let seg = path.segments.last()?;
        if seg.ident == "Option" {
            if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                    return Some(inner);
                }
            }
        }
    }
    None
}

/// 从 Rust 类型映射到 JSON Schema 类型字符串
fn rust_type_to_schema(ty: &syn::Type) -> String {
    // Option<T> → 取内部 T 的类型
    if let Some(inner) = extract_option_inner(ty) {
        return rust_type_to_schema(inner);
    }

    let type_str = quote!(#ty).to_string();
    rust_type_to_schema_str(&type_str)
}

fn rust_type_to_schema_str(ty: &str) -> String {
    match ty {
        "String" | "& str" | "str" => "string".to_string(),
        "bool" => "boolean".to_string(),
        "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize" => {
            "integer".to_string()
        }
        "f32" | "f64" => "number".to_string(),
        t if t.starts_with("Vec <") || t.starts_with("Vec<") => "array".to_string(),
        _ => "string".to_string(),
    }
}

/// 从 `#[desc = "..."]` 属性中提取参数描述
fn extract_desc(attrs: &[syn::Attribute]) -> String {
    for attr in attrs {
        if attr.path().is_ident("desc") {
            if let Meta::NameValue(nv) = &attr.meta {
                if let Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) = &nv.value
                {
                    return s.value();
                }
            }
        }
    }
    String::new()
}

fn extract_default(attrs: &[syn::Attribute]) -> Option<proc_macro2::TokenStream> {
    for attr in attrs {
        if attr.path().is_ident("default") {
            let value: proc_macro2::TokenStream = attr.parse_args().ok()?;
            return Some(value);
        }
    }
    None
}

/// 遍历 struct 的命名字段，收集每个字段的 FieldInfo
fn parse_fields(input: &DeriveInput) -> syn::Result<Vec<FieldInfo>> {
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    input,
                    "only named fields are supported",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                input,
                "ToolInput can only be derived on structs",
            ));
        }
    };

    let mut infos = Vec::new();
    for field in fields {
        let name = field.ident.as_ref().unwrap().to_string();
        let type_str = rust_type_to_schema(&field.ty);

        let description = extract_desc(&field.attrs);

        let (has_default, default_tokens) = match extract_default(&field.attrs) {
            Some(tokens) => (true, Some(tokens)),
            None => (false, None),
        };

        // Option<T> 或有默认值 → 不是 required
        let is_required = extract_option_inner(&field.ty).is_none() && !has_default;

        infos.push(FieldInfo {
            name,
            type_str,
            is_required,
            description,
            has_default,
            default_tokens,
        });
    }

    Ok(infos)
}

/// Derive macro for ToolInput.
///
/// Must be paired with `#[tool(name = "...", description = "...")]` attribute.
/// Each field optionally takes `#[desc = "..."]` for its parameter description.
///
/// ```ignore
/// #[derive(ToolInput)]
/// #[tool(name = "bash", description = "Run shell command")]
/// pub struct BashInput {
///     #[desc = "The command to execute"]
///     pub command: String,
/// }
/// ```
#[proc_macro_derive(ToolInput, attributes(tool, desc, default))]
pub fn derive_tool_input(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match tool_input_impl(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Core logic: extract info from AST and generate `impl ToolInput`.
fn tool_input_impl(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let (tool_name, tool_desc) = parse_tool_attr(&input)?;
    let fields = parse_fields(&input)?;
    let struct_name = &input.ident;

    // 用 quote! 构建 .parameter().required() 链
    let parameters: Vec<proc_macro2::TokenStream> = fields
        .iter()
        .map(|f| {
            let name = &f.name;
            let type_str = &f.type_str;
            let desc = &f.description;

            if let Some(default) = &f.default_tokens {
                quote! {
                    .parameter(#name, #type_str, #desc)
                    .default(#name, serde_json::json!(#default))
                }
            } else {
                quote! { .parameter(#name, #type_str, #desc) }
            }
        })
        .collect();

    let requireds: Vec<&str> = fields
        .iter()
        .filter(|f| f.is_required)
        .map(|f| f.name.as_str())
        .collect();

    // 生成最终的 impl
    Ok(quote! {
        impl ::agentik_sdk::types::ToolInput for #struct_name {
            fn definition() -> ::agentik_sdk::types::ToolDefinition {
                ::agentik_sdk::types::ToolBuilder::new(#tool_name, #tool_desc)
                    #(#parameters)*
                    #(.required(#requireds))*
                    .build()
            }
        }
    })
}
