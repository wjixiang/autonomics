use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use syn::{
    Expr, ExprLit, Fields, Ident, ItemStruct, Lit, LitStr, Meta, Token, Type, TypePath,
    parse::Parse,
};

/// 字段信息
struct FieldInfo {
    name: String,
    type_str: String,
    is_required: bool,
    description: String,
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
    if let Some(inner) = extract_option_inner(ty) {
        return rust_type_to_schema(inner);
    }

    let type_str = quote!(#ty).to_string();
    match type_str.as_str() {
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

/// 遍历 struct 的命名字段，收集每个字段的 FieldInfo。
fn parse_fields_from_struct(input: &ItemStruct) -> syn::Result<Vec<FieldInfo>> {
    let fields = match &input.fields {
        Fields::Named(fields) => &fields.named,
        _ => {
            return Err(syn::Error::new_spanned(
                &input.ident,
                "only named fields are supported",
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

/// Attribute macro: `#[tool(name = "...", description = "...")]` on a struct.
///
/// Automatically injects `#[derive(serde::Serialize, serde::Deserialize)]` and
/// generates `impl ToolInput`. Each field may carry `#[desc = "..."]` and
/// `#[default = ...]`.
#[proc_macro_attribute]
pub fn tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    let tool_attr = match syn::parse::<ToolAttr>(attr) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error().into(),
    };
    let mut input: syn::ItemStruct = match syn::parse(item) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error().into(),
    };

    // 剥离 struct 级别的 #[tool] 属性（#[desc]、#[default] 只出现在字段上）
    input.attrs.retain(|a| !a.path().is_ident("tool"));
    let field_known_attrs = ["desc", "default"];
    if let syn::Fields::Named(ref mut fields) = input.fields {
        for field in &mut fields.named {
            field
                .attrs
                .retain(|a| !field_known_attrs.iter().any(|k| a.path().is_ident(k)));
        }
    }

    // 注入 Serialize + Deserialize（若尚不存在）
    let needs_serde = !input.attrs.iter().any(|a| {
        a.path().is_ident("derive") && a.meta.to_token_stream().to_string().contains("Serialize")
    });
    if needs_serde {
        input.attrs.insert(
            0,
            syn::parse_quote!(#[derive(serde::Serialize, serde::Deserialize)]),
        );
    }

    // 收集字段信息
    let fields = match parse_fields_from_struct(&input) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error().into(),
    };

    // 生成 impl ToolInput
    let struct_name = &input.ident;
    let tool_name = tool_attr.name;
    let tool_desc = tool_attr.descrption;

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

    let impl_block = quote! {
        impl ::agentik_sdk::types::ToolInput for #struct_name {
            fn definition() -> ::agentik_sdk::types::ToolDefinition {
                ::agentik_sdk::types::ToolBuilder::new(#tool_name, #tool_desc)
                    #(#parameters)*
                    #(.required(#requireds))*
                    .build()
            }
        }
    };

    quote! { #input #impl_block }.into()
}

struct ToolAttr {
    name: LitStr,
    descrption: LitStr,
}

impl Parse for ToolAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut name = None;
        let mut description = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            let _eq: Token![=] = input.parse()?;
            let value: LitStr = input.parse()?;

            match key.to_string().as_str() {
                "name" => name = Some(value),
                "description" => description = Some(value),
                other => {
                    return Err(input.error(format!(
                        "unknown key `{other}`, expected `name` or `description`"
                    )));
                }
            }

            if input.is_empty() {
                break;
            }
            let _comma: Token![,] = input.parse()?;
        }

        Ok(Self {
            name: name.ok_or_else(|| input.error("missing `name`"))?,
            descrption: description.ok_or_else(|| input.error("missing `description`"))?,
        })
    }
}
