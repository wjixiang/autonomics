//! Unified source node: brings external data into the DAG as a `DataFrame`.
//!
//! A [`SourceNode`] has no inputs and produces exactly one output. The concrete
//! origin is described by [`Source`]: a file on the local filesystem or a
//! registered object store (with the format auto-detected from the extension,
//! or explicitly given), or an Iceberg table by identifier. Bioinformatics
//! formats (VCF, BAM, BED, …) are read through `biofusion`, which already
//! exposes them as DataFusion tables.

use async_trait::async_trait;
use biofusion::datasource::BioReadOptions;
use biofusion::ext::DataFusionReadExt;
use datafusion::{
    common::HashMap,
    prelude::{CsvReadOptions, DataFrame, ParquetReadOptions, SessionContext},
};
use datalake::Datalake;
use thiserror::Error;

use super::meta::{DagNode, NodeInput, NodeMeta};
use crate::data_engine::dag::{DagError, graph::PortOutputs};

/// Where a [`SourceNode`] reads from.
#[derive(Debug, Clone)]
pub enum Source {
    /// A file path or URL. When `format` is `None`, it is inferred from the
    /// extension (`.vcf.gz` → Vcf, `.bam` → Bam, `.csv` → Csv, …).
    File {
        path: String,
        format: Option<FileFormat>,
    },
    /// An Iceberg table identifier (`namespace.table`), resolved through the
    /// `iceberg` catalog registered on the engine context.
    Iceberg { ident: String },
}

/// Supported file formats. Tabular formats go through DataFusion natively;
/// bioinformatics formats go through `biofusion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    // DataFusion native
    Csv,
    Parquet,
    // biofusion bioinformatics
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

impl FileFormat {
    /// Infer a format from a path's extension. Handles `.gz`-compressed
    /// bioinformatics files (`.vcf.gz`, `.bed.gz`, …).
    pub fn from_path(path: &str) -> Option<Self> {
        let lower = path.to_lowercase();
        // Order matters: longer/compound suffixes first.
        let suffixes: &[(&str, FileFormat)] = &[
            (".vcf.gz", FileFormat::Vcf),
            (".vcf", FileFormat::Vcf),
            (".bcf", FileFormat::Bcf),
            (".fasta.gz", FileFormat::Fasta),
            (".fasta", FileFormat::Fasta),
            (".fa.gz", FileFormat::Fasta),
            (".fa", FileFormat::Fasta),
            (".fastq.gz", FileFormat::Fastq),
            (".fastq", FileFormat::Fastq),
            (".fq.gz", FileFormat::Fastq),
            (".fq", FileFormat::Fastq),
            (".bed.gz", FileFormat::Bed),
            (".bed", FileFormat::Bed),
            (".gtf.gz", FileFormat::Gtf),
            (".gtf", FileFormat::Gtf),
            (".gff3.gz", FileFormat::Gff),
            (".gff3", FileFormat::Gff),
            (".gff.gz", FileFormat::Gff),
            (".gff", FileFormat::Gff),
            (".sam.gz", FileFormat::Sam),
            (".sam", FileFormat::Sam),
            (".bam", FileFormat::Bam),
            (".cram", FileFormat::Cram),
            (".bw", FileFormat::BigWig),
            (".bigwig", FileFormat::BigWig),
            (".bb", FileFormat::BigBed),
            (".bigbed", FileFormat::BigBed),
            (".csv", FileFormat::Csv),
            (".parquet", FileFormat::Parquet),
        ];
        suffixes
            .iter()
            .find(|(s, _)| lower.ends_with(s))
            .map(|(_, f)| *f)
    }
}

/// Errors specific to [`SourceNode`].
#[derive(Debug, Error)]
pub enum SourceError {
    #[error("cannot infer file format from path: {0}")]
    UnknownFormat(String),
    #[error("read source '{path}' failed")]
    Read {
        path: String,
        #[source]
        source: datafusion::error::DataFusionError,
    },
}

impl From<SourceError> for DagError {
    fn from(e: SourceError) -> Self {
        match e {
            SourceError::Read { source, .. } => DagError::DataFusion(source),
            SourceError::UnknownFormat(msg) => DagError::Schedule(msg),
        }
    }
}

#[derive(Clone)]
pub struct SourceNode {
    meta: NodeMeta,
    source: Source,
    ctx: SessionContext,
}

impl SourceNode {
    pub fn new(id: impl Into<String>, source: Source, ctx: SessionContext) -> Self {
        // A source has no inputs and a single output port.
        let meta = NodeMeta::new(id.into());
        let meta = meta.add_output_port(None);
        Self { meta, source, ctx }
    }
}

pub fn normalize_path(path: &str) -> String {
    let trimmed = path
        .trim_matches('/')
        .strip_prefix("./")
        .unwrap_or(path.trim_matches('/'));
    let trimmed = trimmed.strip_prefix('.').unwrap_or(trimmed);
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        format!("/{trimmed}")
    }
}

#[async_trait]
impl DagNode for SourceNode {
    fn meta(&self) -> &NodeMeta {
        &self.meta
    }

    fn clone_box(&self) -> Box<dyn DagNode> {
        Box::new((*self).clone())
    }

    fn node_type(&self) -> &str {
        "source"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn execute(&mut self, _inputs: &[NodeInput]) -> Result<PortOutputs, DagError> {
        let ctx = self.ctx.clone();
        let df = match &self.source {
            Source::File { path, format } => {
                let path = normalize_path(path);
                let fmt = format
                    .or_else(|| FileFormat::from_path(&path))
                    .ok_or_else(|| SourceError::UnknownFormat(path.clone()))?;
                read_file(&ctx, &path, fmt).await?
            }
            Source::Iceberg { ident } => {
                // The iceberg catalog is registered under "iceberg"; qualify
                // the identifier so DataFusion resolves it through that catalog.
                ctx.sql(&format!("SELECT * FROM iceberg.{ident}")).await?
            }
        };
        let mut res: PortOutputs = HashMap::new();
        res.insert(0, df);
        Ok(res)
    }
}

async fn read_file(
    ctx: &SessionContext,
    path: &str,
    fmt: FileFormat,
) -> Result<DataFrame, DagError> {
    use FileFormat::*;
    let df = match fmt {
        Csv => ctx.read_csv(path, CsvReadOptions::default()).await,
        Parquet => ctx.read_parquet(path, ParquetReadOptions::default()).await,
        Vcf => ctx.read_vcf(path, BioReadOptions::default()).await,
        Bcf => ctx.read_bcf(path, BioReadOptions::default()).await,
        Fasta => ctx.read_fasta(path, BioReadOptions::default()).await,
        Fastq => ctx.read_fastq(path, BioReadOptions::default()).await,
        Bed => ctx.read_bed(path, BioReadOptions::default()).await,
        Gtf => ctx.read_gtf(path, BioReadOptions::default()).await,
        Gff => ctx.read_gff(path, BioReadOptions::default()).await,
        Sam => ctx.read_sam(path, BioReadOptions::default()).await,
        Bam => ctx.read_bam(path, BioReadOptions::default()).await,
        Cram => ctx.read_cram(path, BioReadOptions::default()).await,
        BigWig => ctx.read_bigwig(path, BioReadOptions::default()).await,
        BigBed => ctx.read_bigbed(path, BioReadOptions::default()).await,
    };
    df.map_err(|e| {
        SourceError::Read {
            path: path.to_string(),
            source: e,
        }
        .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use datalake::Datalake;
    use fs::OpendalFileStorage;

    #[tokio::test]
    async fn test_load_vcf() {
        let (ctx, fs) = OpendalFileStorage::new_temp().register_to_ctx();
        let test_vcf = std::fs::read("test_datasets/sample.vcf").unwrap();
        fs.op.write("/sample.vcf", test_vcf).await.unwrap();

        let res = ctx
            .read_vcf("/sample.vcf", BioReadOptions::default())
            .await
            .unwrap();

        // res.show().await.unwrap();

        let schema = res.schema();
        dbg!(schema);
    }

    #[tokio::test]
    async fn test_load_vcf_gz() {
        let (ctx, fs) = OpendalFileStorage::new_temp().register_to_ctx();
        dbg!("start copy data");
        let test_vcf_gz = std::fs::read("test_datasets/sample.vcf.gz").unwrap();
        fs.op.write("/sample.vcf.gz", test_vcf_gz).await.unwrap();
        dbg!("copy data finished");

        let res = ctx
            .read_vcf("/sample.vcf.gz", BioReadOptions::default())
            .await
            .unwrap();

        res.show().await.unwrap();

        // let schema = res.schema();
        // dbg!(schema);
    }

    #[tokio::test]
    async fn test_load_from_iceberg() {
        let ctx = Datalake::default().get_ctx().await.unwrap();
        let source = Source::Iceberg {
            ident: "gwas.gwas_study".to_string(),
        };
        let mut node = SourceNode::new("test_id", source, ctx);
        let res = node.execute(&[]).await.unwrap();
        let df = res.get(&0).unwrap().clone();
        df.limit(0, Some(10)).unwrap().show().await.unwrap();
    }
}
