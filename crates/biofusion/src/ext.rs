//! Single extension trait adding bioinformatics file readers to DataFusion's
//! [`SessionContext`].
//!
//! Previously each format module declared its own `DataFusionReadExt`. They are
//! merged here into one trait so there is a single, coherent surface for
//! `read_vcf` / `read_bcf` (and any future formats).

use std::sync::Arc;

use async_trait::async_trait;
use datafusion::common::{DataFusionError, Result};
use datafusion::datasource::listing::{ListingTable, ListingTableConfig, ListingTableUrl};
use datafusion::execution::context::DataFilePaths;
use datafusion::prelude::*;

use crate::datasource_bcf::BcfReadOptions;
use crate::datasource_vcf::VcfReadOptions;

/// Extension trait that adds typed readers (`read_vcf`, `read_bcf`, ...) to a
/// [`SessionContext`], mirroring DataFusion's own `read_csv` / `read_parquet`.
#[async_trait]
pub trait DataFusionReadExt {
    /// Register VCF file(s) as a queryable [`DataFrame`].
    async fn read_vcf<'a, P: DataFilePaths + Send>(
        &self,
        table_paths: P,
        options: VcfReadOptions<'a>,
    ) -> Result<DataFrame>;

    /// Register BCF file(s) as a queryable [`DataFrame`].
    async fn read_bcf<'a, P: DataFilePaths + Send>(
        &self,
        table_paths: P,
        options: BcfReadOptions<'a>,
    ) -> Result<DataFrame>;
}

/// Build a [`DataFrame`] from already-resolved URLs + [`ReadOptions`],
/// mirroring DataFusion's private `_read_type` helper (which external crates
/// cannot call directly): turn the options into `ListingOptions`, resolve the
/// schema, and wrap a [`ListingTable`] via [`SessionContext::read_table`].
///
/// [`ReadOptions`]: datafusion::execution::options::ReadOptions
async fn read_listing_type<'a, O>(
    ctx: &SessionContext,
    table_paths: Vec<ListingTableUrl>,
    options: O,
) -> Result<DataFrame>
where
    O: datafusion::execution::options::ReadOptions<'a>,
{
    let session_config = ctx.copied_config();
    let listing_options = options.to_listing_options(&session_config, ctx.copied_table_options());

    let resolved_schema = options
        .get_resolved_schema(&session_config, ctx.state(), table_paths[0].clone())
        .await?;

    let config = ListingTableConfig::new_with_multi_paths(table_paths)
        .with_listing_options(listing_options)
        .with_schema(resolved_schema);
    let provider = ListingTable::try_new(config)?;
    ctx.read_table(Arc::new(provider))
}

/// Derive the ListingTable file extension for a format from the input URL.
///
/// ListingTable filters discovered files by this extension, so it must reflect
/// the actual file (including a `.gz` suffix) — otherwise a `foo.vcf.gz` input
/// is silently dropped and schema inference reports "no objects". If the user
/// already set an explicit extension we keep it; otherwise we infer
/// `<base_ext>` or `<base_ext>.gz` from the path.
fn infer_file_extension(url: &ListingTableUrl, explicit: Option<&str>, base_ext: &str) -> String {
    if let Some(ext) = explicit {
        return ext.to_string();
    }
    if url.as_str().ends_with(".gz") {
        format!("{base_ext}.gz")
    } else {
        base_ext.to_string()
    }
}

#[async_trait]
impl DataFusionReadExt for SessionContext {
    async fn read_vcf<'a, P: DataFilePaths + Send>(
        &self,
        table_paths: P,
        mut options: VcfReadOptions<'a>,
    ) -> Result<DataFrame> {
        let urls = table_paths.to_urls()?;
        let url = urls
            .first()
            .ok_or_else(|| DataFusionError::Plan("read_vcf: no table path provided".into()))?;
        // Auto-detect the file extension (incl. `.gz`) so ListingTable matches
        // compressed inputs when the caller didn't set one explicitly.
        let ext = infer_file_extension(url, options.file_extension(), "vcf");
        options = options.with_file_extension(ext);
        read_listing_type(self, urls, options).await
    }

    async fn read_bcf<'a, P: DataFilePaths + Send>(
        &self,
        table_paths: P,
        mut options: BcfReadOptions<'a>,
    ) -> Result<DataFrame> {
        let urls = table_paths.to_urls()?;
        let url = urls
            .first()
            .ok_or_else(|| DataFusionError::Plan("read_bcf: no table path provided".into()))?;
        let ext = infer_file_extension(url, options.file_extension(), "bcf");
        options = options.with_file_extension(ext);
        read_listing_type(self, urls, options).await
    }
}
