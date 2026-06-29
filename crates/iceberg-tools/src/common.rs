//! Shared helpers for the Iceberg tools: identifier parsing, schema
//! construction, and error mapping.
//!
//! Tools here intentionally operate on plain dotted-string identifiers so the
//! model can pass them as single parameters (e.g. `"warehouse.analytics"`,
//! `"analytics.events"`). Splitting happens at the Rust boundary.

use agentik_core::tools::ToolError;
use anyhow::{Result, anyhow};
use iceberg::{
    NamespaceIdent, TableIdent,
    spec::{NestedField, NestedFieldRef, PrimitiveType, Schema, Type},
};

/// Parse a dotted namespace path (`"a.b.c"`) into a [`NamespaceIdent`].
///
/// Empty segments are dropped; an entirely empty path is rejected.
pub fn parse_namespace(path: &str) -> Result<NamespaceIdent> {
    let parts: Vec<String> = path
        .split('.')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        return Err(anyhow!("namespace identifier '{path}' is empty"));
    }
    NamespaceIdent::from_vec(parts).map_err(Into::into)
}

/// Build a [`TableIdent`] from separate namespace path and table name.
pub fn table_ident(namespace: &str, table: &str) -> Result<TableIdent> {
    let table = table.trim();
    if table.is_empty() {
        return Err(anyhow!("table name is empty"));
    }
    Ok(TableIdent::new(parse_namespace(namespace)?, table.to_string()))
}

/// Map any error (anyhow, iceberg, datafusion, …) into a tool execution error.
pub fn err<E>(source: E) -> ToolError
where
    E: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
{
    ToolError::ExecutionFailed {
        source: source.into(),
    }
}

/// A column spec parsed from a compact `"name:type"` (or `"name:type!"` for
/// required) string, used by the create-table tool.
pub struct ColumnSpec {
    pub name: String,
    pub field_type: Type,
    pub required: bool,
}

/// Parse a list of compact column specs into typed [`ColumnSpec`]s.
///
/// Supported syntax per entry: `name:type` or `name:type!` (required).
/// Recognised type aliases mirror Iceberg's primitive type names:
/// `boolean`, `int`, `long`, `float`, `double`, `date`, `time`,
/// `timestamp`, `timestamptz`, `string`, `uuid`, `binary`.
pub fn parse_columns(specs: &[String]) -> Result<Vec<ColumnSpec>> {
    let mut out = Vec::with_capacity(specs.len());
    for raw in specs {
        let raw = raw.trim();
        let required = raw.ends_with('!');
        let body = raw.trim_end_matches('!');
        let (name, type_str) = body
            .split_once(':')
            .ok_or_else(|| anyhow!("invalid column spec '{raw}', expected 'name:type'"))?;
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(anyhow!("column spec '{raw}' has empty name"));
        }
        let field_type = parse_primitive_type(type_str.trim())?;
        out.push(ColumnSpec {
            name,
            field_type: Type::Primitive(field_type),
            required,
        });
    }
    if out.is_empty() {
        return Err(anyhow!("at least one column is required"));
    }
    Ok(out)
}

fn parse_primitive_type(s: &str) -> Result<PrimitiveType> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "boolean" | "bool" => PrimitiveType::Boolean,
        "int" | "integer" => PrimitiveType::Int,
        "long" | "bigint" => PrimitiveType::Long,
        "float" => PrimitiveType::Float,
        "double" => PrimitiveType::Double,
        "date" => PrimitiveType::Date,
        "time" => PrimitiveType::Time,
        "timestamp" => PrimitiveType::Timestamp,
        "timestamptz" | "timestamp_tz" => PrimitiveType::Timestamptz,
        "string" | "str" => PrimitiveType::String,
        "uuid" => PrimitiveType::Uuid,
        "binary" | "bytes" => PrimitiveType::Binary,
        other => return Err(anyhow!("unsupported column type '{other}'")),
    })
}

/// Build an Iceberg [`Schema`] from parsed column specs, assigning
/// monotonically increasing field ids starting at 1.
pub fn build_schema(columns: &[ColumnSpec]) -> Result<Schema> {
    let fields: Vec<NestedFieldRef> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let field = if c.required {
                NestedField::required((i + 1) as i32, c.name.clone(), c.field_type.clone())
            } else {
                NestedField::optional((i + 1) as i32, c.name.clone(), c.field_type.clone())
            };
            std::sync::Arc::new(field)
        })
        .collect();
    Schema::builder()
        .with_fields(fields)
        .build()
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_namespace_splits_dotted_path() {
        let ns = parse_namespace(" warehouse.analytics ").unwrap();
        assert_eq!(ns.as_ref(), &["warehouse".to_string(), "analytics".to_string()]);
    }

    #[test]
    fn parse_namespace_rejects_empty() {
        assert!(parse_namespace("").is_err());
        assert!(parse_namespace(" . . ").is_err());
    }

    #[test]
    fn table_ident_splits_namespace_and_name() {
        let ident = table_ident("ns.sub", "events").unwrap();
        assert_eq!(ident.name, "events");
        assert_eq!(ident.namespace.as_ref(), &["ns".to_string(), "sub".to_string()]);
    }

    #[test]
    fn parse_columns_handles_types_and_required() {
        let cols = parse_columns(&[
            "id:long!".to_string(),
            "event:string".to_string(),
            " ts : timestamp ".to_string(),
        ])
        .unwrap();
        assert_eq!(cols.len(), 3);
        assert!(cols[0].required);
        assert!(!cols[1].required);
        assert!(matches!(cols[0].field_type, Type::Primitive(PrimitiveType::Long)));
        assert!(matches!(cols[1].field_type, Type::Primitive(PrimitiveType::String)));
        assert!(matches!(cols[2].field_type, Type::Primitive(PrimitiveType::Timestamp)));
        assert_eq!(cols[2].name, "ts");
    }

    #[test]
    fn parse_columns_rejects_bad_spec() {
        assert!(parse_columns(&["no_type".to_string()]).is_err());
        assert!(parse_columns(&[":long".to_string()]).is_err());
        assert!(parse_columns(&["id:widget".to_string()]).is_err());
        assert!(parse_columns(&[]).is_err());
    }

    #[test]
    fn build_schema_assigns_incrementing_ids() {
        let cols = parse_columns(&["a:int!".to_string(), "b:string".to_string()]).unwrap();
        let schema = build_schema(&cols).unwrap();
        let fields = schema.as_struct().fields();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].id, 1);
        assert!(fields[0].required);
        assert_eq!(fields[1].id, 2);
        assert!(!fields[1].required);
    }
}
