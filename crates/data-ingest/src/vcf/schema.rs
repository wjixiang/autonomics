//! Arrow schema definition for VCF fixed columns + dynamic sample columns.

use std::sync::Arc;

use arrow_schema::{DataType, Field, Schema, SchemaRef};

/// Build the VCF Arrow schema for the 8 fixed columns.
///
/// | Column       | Arrow Type | Nullable | Notes                          |
/// |--------------|-------------|----------|--------------------------------|
/// | chrom        | Utf8        | false    | Reference sequence name        |
/// | pos          | Int64       | false    | 1-based variant position        |
/// | id           | Utf8        | true     | `.` maps to null                |
/// | ref_allele   | Utf8        | false    | Reference base(s)              |
/// | alt          | Utf8        | true     | Comma-separated, `.` -> null   |
/// | qual         | Utf8        | true     | `.` maps to null                |
/// | filter       | Utf8        | false    | PASS / `.` / semicolon-list     |
/// | info         | Utf8        | true     | Raw INFO string                 |
pub fn vcf_fixed_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("chrom", DataType::Utf8, false),
        Field::new("pos", DataType::Int64, false),
        Field::new("id", DataType::Utf8, true),
        Field::new("ref_allele", DataType::Utf8, false),
        Field::new("alt", DataType::Utf8, true),
        Field::new("qual", DataType::Utf8, true),
        Field::new("filter", DataType::Utf8, false),
        Field::new("info", DataType::Utf8, true),
    ]))
}

/// Build a VCF schema that includes dynamic sample columns.
///
/// Sample column names and types are derived from the `VcfParseResult`.
/// This produces a schema with the 8 fixed VCF columns followed by the
/// per-sample FORMAT columns in the order of the keys.
pub fn vcf_arrow_schema(sample_keys: &[String], sample_fields: &[(String, DataType)]) -> SchemaRef {
    let mut fields = vec![
        Field::new("chrom", DataType::Utf8, false),
        Field::new("pos", DataType::Int64, false),
        Field::new("id", DataType::Utf8, true),
        Field::new("ref_allele", DataType::Utf8, false),
        Field::new("alt", DataType::Utf8, true),
        Field::new("qual", DataType::Utf8, true),
        Field::new("filter", DataType::Utf8, false),
        Field::new("info", DataType::Utf8, true),
    ];

    // Append sample columns.
    for (key, dt) in sample_fields {
        // Avoid name collision with fixed columns.
        let field_name = if fields.iter().any(|f| f.name() == key.as_str()) {
            format!("sample_{key}")
        } else {
            key.clone()
        };
        fields.push(Field::new(&field_name, dt.clone(), true));
    }

    let _ = sample_keys; // available for future per-sample column naming
    Arc::new(Schema::new(fields))
}
