pub mod error;

use async_trait::async_trait;
use datafusion::object_store::ObjectStoreExt;
use datafusion::{execution::object_store::ObjectStoreUrl, object_store::path::Path, prelude::*};
use error::{Error, Result};
use oxbow::variant::scanner::vcf::Scanner;
use oxbow::{CoordSystem, Select};
use std::io::Cursor;

#[async_trait]
pub trait DataFusionReadExt {
    async fn read_file(&self, path: &str) -> Result<DataFrame>;
    async fn read_vcf(&self, path: &str) -> Result<DataFrame>;
}

#[async_trait]
impl DataFusionReadExt for SessionContext {
    async fn read_file(&self, path: &str) -> Result<DataFrame> {
        todo!()
    }
    async fn read_vcf(&self, path: &str) -> Result<DataFrame> {
        let url = ObjectStoreUrl::parse("file://")?;
        let store = self.runtime_env().object_store(url)?;

        let path = Path::parse(path)?;
        let vcf_file = store.get(&path).await?;

        // Currently read all data into RAM. This will be fixed when OxBow support async stream
        let bytes = vcf_file.bytes().await?;
        let cursor = Cursor::new(bytes);
        let mut fmt_reader = noodles::vcf::io::Reader::new(cursor);
        let header = fmt_reader.read_header()?;
        let scanner = Scanner::new(
            header,
            Select::All,
            Select::All,
            Select::All,
            None,
            Select::All,
            None,
            CoordSystem::OneClosed,
        )
        .unwrap();
        let batches = scanner.scan(fmt_reader, None, None, Some(1000))?;

        // let vcf = vcf::reader::VcfReader::convert_from_gz_bytes(bytes)?;
        // let pared_data = vcf.parse_into_arrow()?;
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use datafusion::prelude::SessionContext;

    use super::DataFusionReadExt;

    fn test_data_path(file: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_datasets")
            .join(file)
    }

    // --- read_vcf ---

    #[tokio::test]
    #[ignore = "read_vcf conversion to DataFrame not yet implemented"]
    async fn test_read_vcf_gz() {
        let ctx = SessionContext::new();
        let path = test_data_path("test.vcf.gz");
        let df = ctx.read_vcf(path.to_str().unwrap()).await.unwrap();

        let row_count = df.clone().count().await.unwrap();
        assert!(row_count > 0, "expected at least one row from test.vcf.gz");

        let columns: Vec<&str> = df
            .schema()
            .fields()
            .iter()
            .map(|f| f.name().as_str())
            .collect();
        assert!(columns.contains(&"chrom"));
        assert!(columns.contains(&"pos"));
    }

    #[tokio::test]
    #[ignore = "read_vcf conversion to DataFrame not yet implemented"]
    async fn test_read_vcf_plain() {
        let ctx = SessionContext::new();
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fixtures")
            .join("sample.vcf");
        let df = ctx.read_vcf(path.to_str().unwrap()).await.unwrap();

        let row_count = df.clone().count().await.unwrap();
        assert!(row_count > 0, "expected at least one row from sample.vcf");
    }

    #[tokio::test]
    async fn test_read_vcf_file_not_found() {
        let ctx = SessionContext::new();
        let result = ctx.read_vcf("/tmp/nonexistent_file.vcf").await;
        assert!(result.is_err());
    }

    // --- read_file ---

    #[tokio::test]
    #[ignore = "read_file not yet implemented"]
    async fn test_read_file_csv() {
        let ctx = SessionContext::new();
        let path = test_data_path("Iris.csv");
        let df = ctx.read_file(path.to_str().unwrap()).await.unwrap();

        let row_count = df.clone().count().await.unwrap();
        assert!(row_count > 0);
    }
}
