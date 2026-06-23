use datalake::datalake::Datalake;
use iceberg::{
    NamespaceIdent, TableCreation,
    spec::{NestedField, PrimitiveType, Schema, Type},
};
use std::sync::Arc;

/// Integration test: create a table with a simple schema, verify it exists,
/// load it back, and inspect its columns.
///
/// Requires a running Iceberg REST catalog and S3-compatible storage.
/// Set these env vars before running:
///   ICEBERG_REST_URI, ICEBERG_S3_ACCESS_KEY_ID, ICEBERG_S3_SECRET_ACCESS_KEY
#[tokio::test]
async fn test_create_and_load_table() {
    let lake = Datalake::new();

    let ns = NamespaceIdent::from_strs(["tui_test_ns"]).unwrap();

    // Ensure namespace exists (idempotent)
    lake.create_namespace_if_not_exist(&ns)
        .await
        .expect("failed to create namespace");

    // Build schema: id (required long), name (optional string), created_at (optional timestamp)
    let schema = Schema::builder()
        .with_schema_id(0)
        .with_fields(vec![
            Arc::new(
                NestedField::required(1, "id", Type::Primitive(PrimitiveType::Long).into())
                    .with_doc("Primary key".to_string()),
            ),
            Arc::new(
                NestedField::optional(2, "name", Type::Primitive(PrimitiveType::String).into())
                    .with_doc("Display name".to_string()),
            ),
            Arc::new(
                NestedField::optional(
                    3,
                    "created_at",
                    Type::Primitive(PrimitiveType::Timestamp).into(),
                )
                .with_doc("Creation time".to_string()),
            ),
        ])
        .build()
        .expect("failed to build schema");

    let table_name = "tui_test_table";

    // Create table (idempotent: recreate if already exists from a prior run)
    let creation = TableCreation::builder()
        .name(table_name.to_string())
        .schema(schema)
        .build();

    let table = lake
        .create_table_if_not_exist(&ns, creation)
        .await
        .expect("failed to create table");

    // Verify table metadata
    assert_eq!(table.identifier().name, table_name);
    assert_eq!(table.identifier().namespace.to_string(), ns.to_string());

    // Verify schema columns
    let schema_ref = table.current_schema_ref();
    let fields = schema_ref.as_struct().fields();
    assert_eq!(fields.len(), 3, "expected 3 columns");

    assert_eq!(&fields[0].name, "id");
    assert!(fields[0].required);

    assert_eq!(&fields[1].name, "name");

    assert_eq!(&fields[2].name, "created_at");

    // Verify the table appears in list
    let tables = lake
        .list_tables_in_namespace(&ns)
        .await
        .expect("failed to list tables");
    assert!(
        tables.iter().any(|t| t.name == table_name),
        "created table should appear in namespace listing"
    );

    eprintln!(
        "✓ table '{}.{}' created and verified ({} columns)",
        ns.to_string(),
        table_name,
        fields.len(),
    );
}
