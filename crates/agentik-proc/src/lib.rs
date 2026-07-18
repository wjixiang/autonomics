use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use syn::{
    Expr, ExprLit, Fields, Ident, ItemStruct, Lit, LitStr, Meta, Token, parse::Parse,
};

/// Per-field metadata collected at macro expansion time.
struct FieldInfo {
    name: String,
    /// From `#[desc = "..."]`. Injected into the generated schema as the
    /// property's `description`. Fields without `#[desc]` fall back to any
    /// doc-comment description that `schemars` derives automatically.
    description: String,
    /// From `#[default = ...]` (or `#[default(...)]`). Injected into the
    /// generated schema as the property's `default`.
    default_tokens: Option<proc_macro2::TokenStream>,
}

/// Extract a doc/`#[desc]` description for a field.
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

/// Extract the value of a `#[default = EXPR]` (NameValue) or `#[default(EXPR)]`
/// (List) attribute. Returns the raw token stream of the value.
fn extract_default(attrs: &[syn::Attribute]) -> Option<proc_macro2::TokenStream> {
    for attr in attrs {
        if !attr.path().is_ident("default") {
            continue;
        }
        match &attr.meta {
            // `#[default = 120]`
            Meta::NameValue(nv) => return Some(nv.value.to_token_stream()),
            // `#[default(120)]`
            Meta::List(_) => return attr.parse_args().ok(),
            // bare `#[default]` — no value to record
            Meta::Path(_) => return None,
        }
    }
    None
}

/// Collect [`FieldInfo`] for each named field of the struct.
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
        let description = extract_desc(&field.attrs);
        let default_tokens = extract_default(&field.attrs);

        infos.push(FieldInfo {
            name,
            description,
            default_tokens,
        });
    }

    Ok(infos)
}

/// Attribute macro: `#[tool(name = "...", description = "...")]` on a struct.
///
/// Automatically injects `#[derive(serde::Serialize, serde::Deserialize)]` and
/// `#[derive(schemars::JsonSchema)]`, then generates `impl ToolInput`. The tool
/// definition's JSON Schema is produced by `schemars` (via
/// [`agentik_types::tool_definition_from_schema`]) rather than a hand-rolled
/// type mapping, so nested structs, enums, `Option<T>`, `Vec<T>`, and
/// `serde_json::Value` are all handled correctly.
///
/// Each field may carry `#[desc = "..."]` and/or `#[default = ...]`; these are
/// stripped before deriving and re-applied to the generated schema as the
/// property's `description` / `default`.
#[proc_macro_attribute]
pub fn tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    let tool_attr = match syn::parse::<ToolAttr>(attr) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error().into(),
    };

    // 确认属性宏所标注的对象必须是 struct, 并拿到匹配结果
    let mut input: syn::ItemStruct = match syn::parse(item) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error().into(),
    };

    // 收集字段信息 —— 必须在剥离 #[desc]/#[default] 之前完成，
    // 否则 extract_desc/extract_default 拿不到这些属性。
    let fields = match parse_fields_from_struct(&input) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error().into(),
    };

    // 剥离 struct 级别的 #[tool] 属性
    input.attrs.retain(|a| !a.path().is_ident("tool"));
    // 剥离字段上的 #[desc]、#[default]（serde/schemars 的派生宏不认识这些
    // attribute，留着会触发 unknown attribute 警告）
    let field_known_attrs = ["desc", "default"];
    if let syn::Fields::Named(ref mut fields) = input.fields {
        for field in &mut fields.named {
            field
                .attrs
                .retain(|a| !field_known_attrs.iter().any(|k| a.path().is_ident(k)));
        }
    }

    // 注入 Serialize + Deserialize（若尚不存在）
    let derives_str = input
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("derive"))
        .map(|a| a.meta.to_token_stream().to_string())
        .collect::<String>();
    if !derives_str.contains("Serialize") {
        input
            .attrs
            .insert(0, syn::parse_quote!(#[derive(serde::Serialize, serde::Deserialize)]));
    }
    // 注入 JsonSchema（若尚不存在）
    if !derives_str.contains("JsonSchema") {
        input.attrs.insert(0, syn::parse_quote!(#[derive(schemars::JsonSchema)]));
    }

    // 生成 impl ToolInput
    let struct_name = &input.ident;
    let tool_name = tool_attr.name;
    let tool_desc = tool_attr.descrption;

    // Per-field override literals.
    let overrides: Vec<proc_macro2::TokenStream> = fields
        .iter()
        .map(|f| {
            let name = &f.name;
            let desc_expr = if f.description.is_empty() {
                quote! { ::core::option::Option::None }
            } else {
                let d = &f.description;
                quote! { ::core::option::Option::Some(#d.to_string()) }
            };
            let default_expr = match &f.default_tokens {
                Some(tokens) => quote! { ::core::option::Option::Some(serde_json::json!(#tokens)) },
                None => quote! { ::core::option::Option::None },
            };
            quote! {
                ::agentik_sdk::types::FieldOverride {
                    name: #name,
                    description: #desc_expr,
                    default: #default_expr,
                }
            }
        })
        .collect();

    let impl_block = quote! {
        impl ::agentik_sdk::types::ToolInput for #struct_name {
            fn definition() -> ::agentik_sdk::types::ToolDefinition {
                ::agentik_sdk::types::tool_definition_from_schema::<#struct_name>(
                    #tool_name,
                    #tool_desc,
                    &[#(#overrides),*],
                )
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
