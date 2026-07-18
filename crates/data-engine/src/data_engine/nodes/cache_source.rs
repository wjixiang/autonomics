use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read, Seek},
    sync::Arc,
};

use async_trait::async_trait;
use biofusion::{datasource::BioReadOptions, ext::DataFusionReadExt};
use blake3::Hash;
use datafusion::{
    common::DataFusionError,
    execution::object_store::ObjectStoreUrl,
    object_store::{GetOptions, path::Path},
    prelude::{DataFrame, SessionContext},
};
use derive_more::From;
use iceberg::{
    Catalog, NamespaceIdent, TableCreation, TableIdent,
    arrow::arrow_schema_to_schema_auto_assign_ids,
};

use super::source::normalize_path;
use crate::data_engine::dag::{DagError, DagNode, NodeInput, NodeMeta, graph::PortOutputs};

#[derive(Debug, From)]
enum Error {
    #[from]
    Custom(String),

    #[from]
    Io(io::Error),

    #[from]
    Iceberg(iceberg::Error),

    #[from]
    DataFusion(DataFusionError),
}

impl From<Error> for DagError {
    fn from(value: Error) -> Self {
        match value {
            Error::Custom(msg) => DagError::NodeError {
                node_type: "cache_source_node".to_string(),
                msg,
            },
            Error::Io(error) => DagError::NodeError {
                node_type: "cache_source_node".to_string(),
                msg: error.to_string(),
            },
            Error::Iceberg(error) => DagError::NodeError {
                node_type: "cache_source_node".to_string(),
                msg: error.to_string(),
            },
            Error::DataFusion(error) => DagError::NodeError {
                node_type: "cache_source_node".to_string(),
                msg: error.to_string(),
            },
        }
    }
}

/// Supported file formats. Tabular formats go through DataFusion natively;
/// bioinformatics formats go through `biofusion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    Vcf,
    Bcf,
    Fasta,
    Fastq,
    Bed,
    Gtf,
    Gff,
    Sam,
    Bam,
    Cram,
    BigWig,
    BigBed,
}

#[derive(Clone)]
#[allow(dead_code)] // TODO: remove as fields are used
pub struct CacheSourceNode<R: Catalog> {
    meta: NodeMeta,
    file_path: String,
    format: FileFormat,
    ctx: SessionContext,
    df_name: String,
    cached_hash: Option<blake3::Hash>,
    catalog: Arc<R>,
}

impl<R: Catalog> CacheSourceNode<R> {
    pub fn new(
        file_path: String,
        format: FileFormat,
        ctx: SessionContext,
        cache_table_ident: String,
        catalog: Arc<R>,
    ) -> Self {
        let meta = NodeMeta::new().add_output_port(None);
        Self {
            meta,
            file_path,
            ctx,
            format,
            df_name: cache_table_ident,
            cached_hash: None,
            catalog,
        }
    }

    async fn load_file(&self) -> Result<File, Error> {
        // 1. Check if table exists in iceberg

        let object_url = ObjectStoreUrl::parse("file://").map_err(|e| e.to_string())?;
        let store = self
            .ctx
            .runtime_env()
            .object_store(object_url)
            .map_err(|e| e.to_string())?;
        let normalized = normalize_path(&self.file_path);
        let path = Path::parse(&normalized)
            .map_err(|e| DagError::Schedule(format!("cannot parse path '{}': {e}", self.file_path)))
            .map_err(|e| e.to_string())?;
        let file = store
            .get_opts(&path, GetOptions::new())
            .await
            .map_err(|e| e.to_string())?;
        match file.payload {
            datafusion::object_store::GetResultPayload::File(file, _path_buf) => Ok(file),
            datafusion::object_store::GetResultPayload::Stream(_pin) => {
                todo!("not support streaming")
            }
        }
    }

    fn stream_hash(&self, file: &mut File) -> Result<Hash, Error> {
        let mut hasher = blake3::Hasher::new();
        let mut buf = [0u8; 8192];

        loop {
            let n = file.read(&mut buf)?;

            if n == 0 {
                break;
            }

            hasher.update(&buf[..n]);
        }
        let hash = hasher.finalize();
        file.rewind()?;
        Ok(hash)
    }

    /// Read the source file into a [`DataFrame`] via the biofusion reader
    /// matching [`Self::format`]. Mirrors the dispatch in `SourceNode`.
    async fn read_source_df(&self) -> Result<DataFrame, Error> {
        let path = normalize_path(&self.file_path);
        let opts = BioReadOptions::default();
        let df = match self.format {
            FileFormat::Vcf => self.ctx.read_vcf(path, opts).await?,
            FileFormat::Bcf => self.ctx.read_bcf(path, opts).await?,
            FileFormat::Fasta => self.ctx.read_fasta(path, opts).await?,
            FileFormat::Fastq => self.ctx.read_fastq(path, opts).await?,
            FileFormat::Bed => self.ctx.read_bed(path, opts).await?,
            FileFormat::Gtf => self.ctx.read_gtf(path, opts).await?,
            FileFormat::Gff => self.ctx.read_gff(path, opts).await?,
            FileFormat::Sam => self.ctx.read_sam(path, opts).await?,
            FileFormat::Bam => self.ctx.read_bam(path, opts).await?,
            FileFormat::Cram => self.ctx.read_cram(path, opts).await?,
            FileFormat::BigWig => self.ctx.read_bigwig(path, opts).await?,
            FileFormat::BigBed => self.ctx.read_bigbed(path, opts).await?,
        };
        Ok(df)
    }

    /// Cache miss path: materialize the source file into an Iceberg table
    /// keyed by the content hash. After this returns, the table is queryable
    /// as `iceberg.cache.<hash>`.
    ///
    /// Steps:
    /// 1. Read the bio file into a `DataFrame`.
    /// 2. Convert its Arrow schema into an Iceberg schema.
    /// 3. Ensure the `cache` namespace exists, then create the (empty) table.
    /// 4. `INSERT INTO iceberg.cache.<hash> SELECT * FROM <source>` to write
    ///    the data through the registered `IcebergTableProvider`, which handles
    ///    parquet writing and the append commit.
    async fn build_df_cache(&self) -> Result<(), Error> {
        let hash = self
            .cached_hash
            .ok_or_else(|| Error::Custom("cannot build cache before hashing".into()))?
            .to_string();

        // 1. Read source + 2. derive iceberg schema from the arrow schema.
        let df = self.read_source_df().await?;
        let arrow_schema = df.schema();
        let iceberg_schema = arrow_schema_to_schema_auto_assign_ids(arrow_schema.as_ref())?;

        // 3. Ensure namespace + create table.
        let namespace = NamespaceIdent::from_strs(["cache"])?;
        if !self.catalog.namespace_exists(&namespace).await? {
            self.catalog
                .create_namespace(&namespace, HashMap::new())
                .await?;
        }

        let creation = TableCreation::builder()
            .name(hash.clone())
            .schema(iceberg_schema)
            .build();
        self.catalog.create_table(&namespace, creation).await?;

        // 4. Write the rows through DataFusion. The `iceberg` catalog is
        //    registered on the context and yields write-capable table
        //    providers, so a plain INSERT works.
        let src_name = format!("__cache_src_{hash}");
        let _ = self.ctx.deregister_table(&src_name);
        let view = df.into_view();
        self.ctx.register_table(&src_name, view)?;

        let sql = format!("INSERT INTO iceberg.cache.{hash} SELECT * FROM {src_name}");
        self.ctx.sql(&sql).await?.collect().await?;

        self.ctx.deregister_table(&src_name)?;
        Ok(())
    }
}

#[async_trait]
impl<R: Catalog + Clone + 'static> DagNode for CacheSourceNode<R> {
    fn meta(&self) -> &NodeMeta {
        &self.meta
    }

    async fn execute(&mut self, _inputs: &[NodeInput]) -> Result<PortOutputs, DagError> {
        // Content-address the source file once.
        let mut file = self.load_file().await?;
        if self.cached_hash.is_none() {
            let file_hash = self.stream_hash(&mut file)?;
            self.cached_hash = Some(file_hash);
        }

        let hash = self
            .cached_hash
            .ok_or_else(|| Error::Custom("hash not computed".into()))?
            .to_string();

        // Cache miss: materialize the file into `iceberg.cache.<hash>`.
        let ident = TableIdent::from_strs(["cache", &hash]).map_err(Error::from)?;
        if !self
            .catalog
            .table_exists(&ident)
            .await
            .map_err(Error::from)?
        {
            self.build_df_cache().await?;
        }

        // Load the (now-guaranteed) cache table back as a DataFrame.
        let df = self
            .ctx
            .sql(&format!("SELECT * FROM iceberg.cache.{hash}"))
            .await?;

        let mut res = PortOutputs::new();
        res.insert(0, df);
        Ok(res)
    }

    fn clone_box(&self) -> Box<dyn DagNode> {
        Box::new(self.clone())
    }

    fn node_type(&self) -> &str {
        "cache_source"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
