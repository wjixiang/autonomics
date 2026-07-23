//! Read-only verification: per-chromosome row counts + grand total.
use datalake::Datalake;
use datafusion::arrow::array::Int64Array;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ctx = Datalake::new().get_ctx().await?;
    let mut total: i64 = 0;
    for chr in 1..=22u32 {
        let fqn = format!("iceberg.ld_matrix.eur_chr{chr}");
        let batches = ctx
            .sql(format!("SELECT COUNT(*) FROM {fqn}").as_str())
            .await?
            .collect()
            .await?;
        let n = batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .value(0);
        println!("{fqn}: {n}");
        total += n;
    }
    println!("\nTOTAL rows across 22 chromosomes: {total}");
    Ok(())
}
