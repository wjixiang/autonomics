use async_trait::async_trait;
use datafusion::prelude::SessionContext;

use crate::data_engine::dag::{DagError, DagNode, NodeInput, NodeMeta, graph::NamedDataFrames};

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
pub struct CacheSourceNode {
    meta: NodeMeta,
    file_path: String,
    format: FileFormat,
    ctx: SessionContext,
    cache_table_ident: String,
    cached_hash: Option<blake3::Hash>,
}

impl CacheSourceNode {
    pub fn new(
        meta: NodeMeta,
        file_path: String,
        format: FileFormat,
        ctx: SessionContext,
        cache_table_ident: String,
    ) -> Self {
        Self {
            meta,
            file_path,
            ctx,
            format,
            cache_table_ident,
            cached_hash: None,
        }
    }
}

#[async_trait]
impl DagNode for CacheSourceNode {
    fn meta(&self) -> &NodeMeta {
        &self.meta
    }

    fn clone_box(&self) -> Box<dyn DagNode> {
        Box::new(self.clone())
    }

    async fn execute(&mut self, inputs: &[NodeInput]) -> Result<NamedDataFrames, DagError> {
        // 1. Check if table exists in iceberg
        todo!()
    }
}
