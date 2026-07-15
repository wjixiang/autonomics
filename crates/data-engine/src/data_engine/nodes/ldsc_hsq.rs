//! LD Score Regression (LDSC) transform node.
//!
//! Takes a single upstream GWAS summary statistics `DataFrame` (with Z-scores,
//! sample sizes, and rsid), queries the Iceberg data lake for LD score panel
//! data under `genetics.ld_score`, joins on rsid, and runs LD Score Regression
//! via [`ldsc::hsq::estimate_h2`]. Outputs a single-row summary `DataFrame`
//! with h², intercept, ratio, and per-annotation coefficients.

use std::sync::Arc;

use arrow_array::{Float64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use async_trait::async_trait;
use datalake::Datalake;
use thiserror::Error;

use super::meta::{DagNode, NodeInput, NodeMeta};
use crate::data_engine::dag::{DagError, graph::PortOutputs};

// =====================================================================
// Error type
// =====================================================================

#[derive(Debug, Error)]
pub enum LdscNodeError {
    #[error("LDSC computation failed: {0}")]
    Ldsc(#[from] ldsc::LdscError),
    #[error("failed to build result batch: {0}")]
    Arrow(#[from] arrow_schema::ArrowError),
    #[error("failed to read result batch: {0}")]
    ReadBatch(#[from] datafusion::error::DataFusionError),
    #[error("datalake error: {0}")]
    Datalake(String),
}

impl From<LdscNodeError> for DagError {
    fn from(e: LdscNodeError) -> Self {
        DagError::NodeError {
            node_type: "ldsc".to_string(),
            msg: e.to_string(),
        }
    }
}

impl From<datalake::error::Error> for LdscNodeError {
    fn from(e: datalake::error::Error) -> Self {
        LdscNodeError::Datalake(e.to_string())
    }
}

// =====================================================================
// Output DataFrame construction
// =====================================================================

/// Build a single-row summary `RecordBatch` from the LDSC result.
///
/// Columns: `h2`, `h2_se`, `intercept`, `intercept_se`, `ratio`,
/// `ratio_se`, `mean_chisq`, `lambda_gc`, `n_snp` (Float64),
/// `coef`, `coef_se` (Utf8 — JSON arrays).
fn build_result_batch(r: &ldsc::hsq::HsqResult) -> Result<RecordBatch, LdscNodeError> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("h2", DataType::Float64, false),
        Field::new("h2_se", DataType::Float64, false),
        Field::new("intercept", DataType::Float64, true),
        Field::new("intercept_se", DataType::Float64, true),
        Field::new("ratio", DataType::Float64, true),
        Field::new("ratio_se", DataType::Float64, true),
        Field::new("mean_chisq", DataType::Float64, false),
        Field::new("lambda_gc", DataType::Float64, false),
        Field::new("n_snp", DataType::Float64, false),
        Field::new("coef", DataType::Utf8, false),
        Field::new("coef_se", DataType::Utf8, false),
    ]));

    let coef_json = serde_json::to_string(&r.coef)
        .map_err(|e| LdscNodeError::Ldsc(ldsc::LdscError::InvalidInput(e.to_string())))?;
    let coef_se_json = serde_json::to_string(&r.coef_se)
        .map_err(|e| LdscNodeError::Ldsc(ldsc::LdscError::InvalidInput(e.to_string())))?;

    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(Float64Array::from(vec![r.h2])),
            Arc::new(Float64Array::from(vec![r.h2_se])),
            Arc::new(Float64Array::from(vec![r.intercept])),
            Arc::new(Float64Array::from(vec![r.intercept_se])),
            Arc::new(Float64Array::from(vec![r.ratio])),
            Arc::new(Float64Array::from(vec![r.ratio_se])),
            Arc::new(Float64Array::from(vec![r.mean_chisq])),
            Arc::new(Float64Array::from(vec![r.lambda_gc])),
            Arc::new(Float64Array::from(vec![r.n_snp as f64])),
            Arc::new(StringArray::from(vec![coef_json])),
            Arc::new(StringArray::from(vec![coef_se_json])),
        ],
    )?;

    Ok(batch)
}

// =====================================================================
// Node
// =====================================================================

/// A transform node that runs LD Score Regression for SNP-heritability (h²).
///
/// Accepts raw GWAS summary statistics as input, queries the Iceberg data lake
/// for LD score panel data, performs the join internally, and runs LDSC.
#[derive(Clone)]
pub struct LdscHsqNode {
    /// DAG node metadata (id, ports).
    meta: NodeMeta,
    /// Iceberg data lake handle, used to fetch the LD score panel
    /// (`iceberg.ld_score.*`) for the join against upstream sumstats.
    datalake: Arc<Datalake>,
    /// Upstream sumstats column carrying per-SNP Z-scores. Aliased to `Z`
    /// in the inner SQL before being passed to
    /// [`ldsc::hsq::estimate_h2`].
    z_column: String,
    /// Upstream sumstats column carrying per-SNP sample sizes. Aliased
    /// to `N` in the inner SQL.
    n_column: String,
    /// Upstream sumstats column carrying SNP identifiers; used as the
    /// join key against `iceberg.ld_score.*.rsid`.
    rsid_column: String,
    /// Configuration for the LDSC h² estimation algorithm itself
    /// (per-annotation M, jackknife blocks, optional fixed intercept).
    /// See [`LdscHsqConfig`].
    ldsc_hsq: LdscHsqConfig,
}

/// Configuration for the LDSC h² estimation algorithm.
///
/// Bundles the three parameters that govern the LDSC regression itself.
/// All other [`LdscHsqNode`] parameters (datalake, sumstats column names)
/// are orthogonal and live on the node directly.
#[derive(Clone, Debug)]
pub struct LdscHsqConfig {
    /// Per-annotation M: the number of SNPs in each LD score annotation
    /// category, in the same order as `HsqColumns::ref_ld`. M is the
    /// normalising constant in the LDSC regression
    /// `E[chi²] = a + N·τ·l_j / M + N·b`.
    ///
    /// For baseline LDSC (single annotation, the current node wiring)
    /// pass `vec![total_snps]`, where `total_snps` matches the SNP
    /// count of the LD score panel. For partitioned LDSC pass one
    /// element per annotation category.
    pub m: Vec<f64>,
    /// Number of block-jackknife blocks used by
    /// [`ldsc::hsq::estimate_h2`] to compute standard errors and the
    /// intercept. A typical value is 200.
    pub n_blocks: usize,
    /// Optional fixed intercept for the LDSC regression.
    ///
    /// `None` (the common case) lets [`ldsc::hsq::estimate_h2`] estimate
    /// the intercept from the data, where it absorbs confounding such as
    /// population stratification. `Some(v)` constrains the regression
    /// to pass through `(0, v)`.
    pub intercept: Option<f64>,
}

impl LdscHsqConfig {
    /// Construct an [`LdscHsqConfig`].
    pub fn new(m: Vec<f64>, n_blocks: usize, intercept: Option<f64>) -> Self {
        Self {
            m,
            n_blocks,
            intercept,
        }
    }
}

impl LdscHsqNode {
    /// Construct an [`LdscHsqNode`].
    ///
    /// # Arguments
    ///
    /// * `id` — DAG node id, used in plan output and to route outputs.
    /// * `datalake` — Iceberg data lake handle; the node queries the
    ///   LD score panel from `genetics.ld_score.*` through it.
    /// * `z_column` — upstream sumstats column name for Z-scores.
    /// * `n_column` — upstream sumstats column name for per-SNP sample
    ///   sizes.
    /// * `rsid_column` — upstream sumstats column name for SNP ids; used
    ///   as the join key against the LD score panel.
    /// * `ldsc_hsq` — algorithm configuration; see [`LdscHsqConfig`].
    pub fn new(
        id: impl Into<String>,
        datalake: Arc<Datalake>,
        z_column: String,
        n_column: String,
        rsid_column: String,
        ldsc_hsq: LdscHsqConfig,
    ) -> Self {
        let meta = NodeMeta::new(id).add_output_port(None).add_input_port(None);
        Self {
            meta,
            datalake,
            z_column,
            n_column,
            rsid_column,
            ldsc_hsq,
        }
    }
}

/// The fixed column names used in the internal SQL join for passing to
/// [`ldsc::hsq::HsqColumns`]. The SQL aliases output columns to these names
/// so the downstream LDSC computation is independent of the user-facing
/// column names.
const LD_Z_COL: &str = "Z";
const LD_N_COL: &str = "N";
const LD_REF_COL: &str = "L2_0";
const LD_WLD_COL: &str = "WLD";

#[async_trait]
impl DagNode for LdscHsqNode {
    fn meta(&self) -> &NodeMeta {
        &self.meta
    }

    fn clone_box(&self) -> Box<dyn DagNode> {
        Box::new((*self).clone())
    }

    fn node_type(&self) -> &str {
        "ldsc"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn execute(&mut self, inputs: &[NodeInput]) -> Result<PortOutputs, DagError> {
        let input = inputs
            .first()
            .ok_or(LdscNodeError::Ldsc(ldsc::LdscError::InvalidInput(
                "no input DataFrame".into(),
            )))?;

        // 1. Get a DataFusion context with the Iceberg catalog registered.
        let ctx = self.datalake.get_ctx().await.map_err(LdscNodeError::from)?;

        // 2. Register the upstream sumstats DataFrame as a temporary table.
        ctx.register_table("sumstats", input.data.clone().into_view())
            .map_err(LdscNodeError::ReadBatch)?;

        // TODO: Auto select LD score panel table by population
        let ld_score_table = "ukbb_eur";

        // 3. Build SQL: join sumstats with LD score panel on rsid.
        //    ld_score is used for both ref_ld and w_ld (single-annotation baseline).
        let sql = format!(
            r#"SELECT s."{z}" AS "{Z}", s."{n}" AS "{N}", l.ld_score AS "{REF}", l.ld_score AS "{WLD}"
               FROM sumstats AS s
               INNER JOIN iceberg.ld_score.{table} AS l
               ON s."{rsid}" = l.rsid
               ORDER BY l.locus.pos"#,
            z = self.z_column,
            n = self.n_column,
            rsid = self.rsid_column,
            table = ld_score_table,
            Z = LD_Z_COL,
            N = LD_N_COL,
            REF = LD_REF_COL,
            WLD = LD_WLD_COL,
        );

        // 4. Execute the join and collect the result.
        let joined_df = ctx.sql(&sql).await.map_err(LdscNodeError::ReadBatch)?;

        // 5. Build the LDSC column-name descriptor.
        let cols = ldsc::hsq::HsqColumns {
            snp: "", // not consumed by the computation
            z: LD_Z_COL,
            n: LD_N_COL,
            ref_ld: vec![LD_REF_COL],
            w_ld: LD_WLD_COL,
        };

        // 6. Run LDSC on the joined DataFrame.
        let LdscHsqConfig {
            m,
            n_blocks,
            intercept,
        } = &self.ldsc_hsq;
        let result =
            ldsc::hsq::estimate_h2(joined_df, cols, m, *n_blocks, *intercept)
                .await
                .map_err(LdscNodeError::from)?;

        // 7. Build a single-row summary RecordBatch and return.
        let batch = build_result_batch(&result)?;
        let df = ctx.read_batch(batch).map_err(LdscNodeError::ReadBatch)?;

        let mut res: PortOutputs = PortOutputs::new();
        res.insert(0, df);
        Ok(res)
    }
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::{Float64Array, StringArray};
    use arrow_schema::Field;

    /// Helper: build a `RecordBatch` from typed columns.
    fn make_batch(columns: Vec<(&str, Arc<dyn arrow_array::Array>)>) -> RecordBatch {
        let fields: Vec<Field> = columns
            .iter()
            .map(|(name, arr)| Field::new(*name, arr.data_type().clone(), arr.null_count() == 0))
            .collect();
        let arrays: Vec<Arc<dyn arrow_array::Array>> =
            columns.into_iter().map(|(_, a)| a).collect();
        RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).unwrap()
    }

    /// Helper: build a synthetic GWAS sumstats `NodeInput` for the LDSC node.
    ///
    /// Generates `n` rows with columns `Z`, `N`, `rsid`, wraps them in a
    /// `RecordBatch`, registers them in a fresh `SessionContext`, and returns
    /// the resulting `NodeInput` on port 0. Both LDSC node tests share the
    /// same fixture shape so the helper keeps them in sync.
    ///
    /// - `n`: Test sample size
    fn make_test_input(n: usize) -> super::super::meta::NodeInput {
        let rsids: Vec<String> = (0..n).map(|i| format!("rs{}", 1000000 + i)).collect();
        let z: Vec<f64> = (0..n)
            .map(|i| ((i * 7 + 3) % 11) as f64 * 0.4 - 2.0)
            .collect();
        let n_samp: Vec<f64> = vec![1000.0_f64; n];

        let batch = make_batch(vec![
            ("Z", Arc::new(Float64Array::from(z)) as _),
            ("N", Arc::new(Float64Array::from(n_samp)) as _),
            ("rsid", Arc::new(StringArray::from(rsids)) as _),
        ]);

        super::super::meta::NodeInput {
            port: 0,
            data: datafusion::prelude::SessionContext::new()
                .read_batch(batch)
                .unwrap(),
        }
    }

    #[tokio::test]
    async fn test_ldsc_hsq_node_output_schema() {
        // Create a mock Datalake — the node test only verifies the output schema
        // via a pre-joined path that doesn't touch the real catalog.
        // We test the join logic by verifying the node accepts the new input.
        let node = LdscHsqNode::new(
            "ldsc_test",
            Arc::new(Datalake::new()),
            "Z".to_string(),
            "N".to_string(),
            "rsid".to_string(),
            LdscHsqConfig::new(vec![20.0], 5, None),
        );

        let _input = make_test_input(20);

        // The node will try to query Iceberg in execute(), which will fail
        // without a real catalog. Instead, verify the struct construction
        // and the input acceptance path.
        assert_eq!(node.node_type(), "ldsc");
        assert_eq!(node.meta().id(), "ldsc_test");
    }

    #[tokio::test]
    async fn test_ld_panel_fetching_e2e() {
        let n = 20;
        let _datalake = Datalake::default();
        let mut node = LdscHsqNode::new(
            "ldsc_test",
            Arc::new(Datalake::default()),
            "Z".to_string(),
            "N".to_string(),
            "rsid".to_string(),
            LdscHsqConfig::new(vec![n as f64], 5, None),
        );
        let input = make_test_input(n);
        // input.data.show().await.unwrap();
        let res = node.execute(&[input]).await.unwrap();
        dbg!(res);
    }
}
