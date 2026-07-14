//! DataFrame ingest — extract the per-SNP vectors LDSC needs from an
//! already-joined DataFusion [`DataFrame`] into plain `Vec<f64>` / faer [`Mat`].
//!
//! The caller supplies a single `DataFrame` joining GWAS summary statistics,
//! per-SNP LD Scores, and per-SNP weight LD Scores on the SNP key. `M` (the
//! L2-summed per-annotation count) is *not* per-SNP, so it is passed separately
//! to [`crate::hsq::estimate_h2`].
//!
//! Column extraction reuses the downcast-ladder pattern from
//! `crates/data-engine/.../linear_regression.rs`: `df.collect()` →
//! `Vec<RecordBatch>` → cast each numeric array through Int/UInt/Float variants
//! → `Vec<f64>`, with nulls carried as `NaN`.

use arrow_array::{
    Array, Float32Array, Float64Array, Int8Array, Int16Array, Int32Array, Int64Array, UInt8Array,
    UInt16Array, UInt32Array, UInt64Array,
};
use arrow_schema::DataType;
use datafusion::prelude::DataFrame;
use faer::Mat;

use crate::hsq::HsqColumns;
use crate::{LdscError, Result};

/// The per-SNP arrays extracted from the joined DataFrame.
pub struct HsqArrays {
    /// Z-scores (length `n`).
    pub z: Vec<f64>,
    /// Per-SNP sample sizes (length `n`).
    pub n: Vec<f64>,
    /// Per-annotation LD Scores (`n × n_annot`).
    pub ref_ld: Mat<f64>,
    /// Per-SNP weight LD Scores (length `n`).
    pub w_ld: Vec<f64>,
}

/// Pull `z`, `n`, each `ref_ld` column, and `w_ld` out of the DataFrame.
pub async fn to_arrays(df: DataFrame, cols: &HsqColumns<'_>) -> Result<HsqArrays> {
    let batches = df.collect().await?;
    if batches.is_empty() {
        return Err(LdscError::InvalidInput("estimate_h2: empty DataFrame (no record batches)".into()));
    }
    let z = extract_column(&batches, cols.z)?;
    let n = extract_column(&batches, cols.n)?;
    let w_ld = extract_column(&batches, cols.w_ld)?;
    let nrows = z.len();
    if n.len() != nrows || w_ld.len() != nrows {
        return Err(LdscError::DimensionMismatch(format!(
            "estimate_h2: column length mismatch (z={}, n={}, w_ld={})",
            nrows,
            n.len(),
            w_ld.len()
        )));
    }
    if cols.ref_ld.is_empty() {
        return Err(LdscError::InvalidInput("estimate_h2: at least one ref_ld column is required".into()));
    }

    let mut flat: Vec<f64> = Vec::with_capacity(nrows * cols.ref_ld.len());
    for name in &cols.ref_ld {
        let col = extract_column(&batches, name)?;
        if col.len() != nrows {
            return Err(LdscError::DimensionMismatch(format!(
                "estimate_h2: ref_ld column '{name}' has length {}, expected {nrows}",
                col.len()
            )));
        }
        flat.extend(col);
    }
    let n_annot = cols.ref_ld.len();
    // column-major: annotation k occupies flat[k*nrows .. (k+1)*nrows].
    let ref_ld = Mat::from_fn(nrows, n_annot, |i, k| flat[k * nrows + i]);

    Ok(HsqArrays { z, n, ref_ld, w_ld })
}

/// Extract a named numeric column from a sequence of record batches into a
/// flat `Vec<f64>`, casting nulls to `NaN`.
fn extract_column(batches: &[arrow_array::RecordBatch], name: &str) -> Result<Vec<f64>> {
    let schema = batches
        .first()
        .ok_or_else(|| LdscError::InvalidInput("extract_column: no batches".into()))?
        .schema();
    let idx = schema
        .index_of(name)
        .map_err(|_| LdscError::InvalidInput(format!("missing column '{name}'")))?;
    let dtype = schema.field(idx).data_type().clone();
    let is_numeric = matches!(
        dtype,
        DataType::Float16
            | DataType::Float32
            | DataType::Float64
            | DataType::Int8
            | DataType::Int16
            | DataType::Int32
            | DataType::Int64
            | DataType::UInt8
            | DataType::UInt16
            | DataType::UInt32
            | DataType::UInt64
    );
    if !is_numeric {
        return Err(LdscError::InvalidInput(format!(
            "column '{name}' is not numeric (got {dtype})"
        )));
    }
    let mut out = Vec::new();
    for batch in batches {
        push_numeric(batch.column(idx), &mut out);
    }
    Ok(out)
}

fn push_numeric(col: &dyn Array, out: &mut Vec<f64>) {
    macro_rules! cast {
        ($arr:expr, $t:ty) => {
            if let Some(a) = $arr.as_any().downcast_ref::<$t>() {
                for v in a.iter() {
                    out.push(match v {
                        Some(val) => val as f64,
                        None => f64::NAN,
                    });
                }
                return;
            }
        };
    }
    cast!(col, Int8Array);
    cast!(col, Int16Array);
    cast!(col, Int32Array);
    cast!(col, Int64Array);
    cast!(col, UInt8Array);
    cast!(col, UInt16Array);
    cast!(col, UInt32Array);
    cast!(col, UInt64Array);
    cast!(col, Float32Array);
    cast!(col, Float64Array);
}
