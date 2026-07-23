//! Read-only, metadata-only: does each ld_matrix table have committed data?
//! Fast — inspects snapshots, does NOT scan data files.
use datalake::Datalake;
use iceberg::{Catalog, NamespaceIdent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let dl = Datalake::new();
    let catalog = dl.get_catalog().await?;
    let ns = NamespaceIdent::from_vec(vec!["ld_matrix".to_string()])?;

    let mut with_data = 0usize;
    let mut empty = 0usize;
    let mut total_records: i64 = 0;
    let mut tables = catalog.list_tables(&ns).await?;
    tables.sort_by(|a, b| a.name.cmp(&b.name));
    for t in &tables {
        let table = catalog.load_table(t).await?;
        let md = table.metadata();
        let n_snaps = md.snapshots().count();
        // Row count straight from the snapshot summary — no data scan.
        let rows = md
            .current_snapshot()
            .and_then(|s| s.summary().additional_properties.get("total-records"))
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(-1);
        let state = if n_snaps > 0 {
            with_data += 1;
            if rows > 0 {
                total_records += rows;
            }
            "HAS DATA"
        } else {
            empty += 1;
            "EMPTY"
        };
        println!(
            "ld_matrix.{:<11} snapshots={n_snaps}  rows={rows:>12}  {state}",
            t.name
        );
    }
    println!(
        "\n{} tables: {with_data} with data, {empty} empty | TOTAL rows = {total_records}",
        tables.len()
    );
    Ok(())
}
