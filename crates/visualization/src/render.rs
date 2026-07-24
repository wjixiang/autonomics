//! Render Arrow `RecordBatch`es to a PNG via R's ggplot2.
//!
//! The renderer hands data to R through the **Arrow IPC stream** format (a
//! stable, version-tolerant wire format): the batches are serialized in Rust,
//! written next to a generated R script in a tempdir, and an `Rscript`
//! subprocess reads them back with `arrow::read_ipc_stream` into a
//! `data.frame`, runs the caller-supplied ggplot2 code, and saves the PNG.
//!
//! Why a subprocess instead of in-process [`extendr_api`][crate]?  Linking
//! `libR` would force the *entire* data-engine to depend on R being present at
//! build- and runtime. Visualization is an optional capability, so the core
//! pipeline must keep working without R. Subprocess invocation keeps that
//! boundary clean: the node returns a clear [`VizError::RscriptNotFound`] when
//! R is unavailable, and the rest of the DAG is unaffected.

use std::path::{Path, PathBuf};
use std::time::Duration;

use arrow::ipc::writer::StreamWriter;
use arrow::record_batch::RecordBatch;

use crate::error::{Result, VizError};

/// Default figure dimensions (inches) and resolution.
const DEFAULT_WIDTH: f64 = 8.0;
const DEFAULT_HEIGHT: f64 = 6.0;
const DEFAULT_DPI: f64 = 150.0;
/// Hard cap on a single render before the subprocess is killed.
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Render `batches` to a PNG and return the **bytes**, using the supplied
/// ggplot2 code. The PNG is produced in a private tempdir and never touches a
/// caller-chosen path — use this when you want to forward the bytes elsewhere
/// (e.g. upload into a virtualized object store) rather than write to disk.
///
/// See [`render_png`] for the `r_code` contract (`df` bound, must assign `p`).
pub async fn render_png_bytes(
    batches: &[RecordBatch],
    r_code: &str,
    width: Option<f64>,
    height: Option<f64>,
    dpi: Option<f64>,
) -> Result<Vec<u8>> {
    let (ipc, width, height, dpi) = prepare(batches, r_code, width, height, dpi)?;
    run_rscript(&ipc, r_code, width, height, dpi).await
}

/// Render `batches` to a PNG at `output_path` using the supplied ggplot2 code.
///
/// # The `r_code` contract
///
/// The caller's `r_code` runs in an environment where a `data.frame` named
/// **`df`** is already bound to the input data. The code must build a ggplot
/// object and assign it to a variable named **`p`** — e.g.
///
/// ```r
/// p <- ggplot(df, aes(x = bp, y = pval)) + geom_point()
/// ```
///
/// The wrapper then calls `ggsave(output_path, plot = p, ...)` with the given
/// `width`/`height`/`dpi`.
///
/// `output_path`'s parent directory must exist and be writable. This is a
/// thin wrapper over [`render_png_bytes`] that writes the returned bytes to
/// `output_path`.
pub async fn render_png(
    batches: &[RecordBatch],
    r_code: &str,
    output_path: &Path,
    width: Option<f64>,
    height: Option<f64>,
    dpi: Option<f64>,
) -> Result<()> {
    let bytes = render_png_bytes(batches, r_code, width, height, dpi).await?;
    std::fs::write(output_path, bytes)?;
    Ok(())
}

/// Shared front-end: validate input, derive the schema, serialize the IPC
/// stream, and resolve default dimensions.
fn prepare(
    batches: &[RecordBatch],
    r_code: &str,
    width: Option<f64>,
    height: Option<f64>,
    dpi: Option<f64>,
) -> Result<(Vec<u8>, f64, f64, f64)> {
    let r_code = r_code.trim();
    if r_code.is_empty() {
        return Err(VizError::InvalidPlotCode("plot code is empty".to_string()));
    }

    // All batches share one schema; take it from the first non-empty batch,
    // falling back to an empty schema for a zero-row render.
    let schema = batches
        .iter()
        .map(|b| b.schema())
        .next()
        .unwrap_or_else(|| arrow::datatypes::SchemaRef::new(arrow::datatypes::Schema::empty()));

    let ipc = batches_to_ipc_stream(&schema, batches)?;
    let width = width.unwrap_or(DEFAULT_WIDTH);
    let height = height.unwrap_or(DEFAULT_HEIGHT);
    let dpi = dpi.unwrap_or(DEFAULT_DPI);
    Ok((ipc, width, height, dpi))
}

/// Serialize `RecordBatch`es (sharing one schema) into Arrow **IPC stream**
/// bytes — the format R's `arrow::read_ipc_stream` consumes.
fn batches_to_ipc_stream(
    schema: &arrow::datatypes::SchemaRef,
    batches: &[RecordBatch],
) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut writer = StreamWriter::try_new(&mut buf, schema)?;
    for b in batches {
        writer.write(b)?;
    }
    writer.finish()?;
    drop(writer);
    Ok(buf)
}

/// Resolve `Rscript` on `PATH`, returning [`VizError::RscriptNotFound`] if it
/// is missing. Split out so the error is distinct from a real subprocess
/// failure.
fn resolve_rscript() -> Result<PathBuf> {
    let candidate = std::env::var_os("VISUALIZATION_RSCRIPT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("Rscript"));
    Ok(candidate)
}

/// Write the IPC bytes + a generated R script to a fresh tempdir, invoke
/// `Rscript`, and return the rendered PNG bytes. The output PNG lives inside
/// the tempdir (a private scratch path), so the caller's filesystem layout is
/// never touched — only the returned bytes leave this function.
async fn run_rscript(
    ipc: &[u8],
    r_code: &str,
    width: f64,
    height: f64,
    dpi: f64,
) -> Result<Vec<u8>> {
    let rscript = resolve_rscript()?;

    let tmp = tempfile::tempdir()?;
    let data_path = tmp.path().join("data.arrow_stream");
    let out_path = tmp.path().join("out.png");
    let script_path = tmp.path().join("plot.R");

    std::fs::write(&data_path, ipc)?;

    // The R wrapper: load libs, read the IPC stream into `df`, run the
    // caller's code (which must assign `p`), then save. Paths are injected as
    // quoted literals to avoid shell/quote injection.
    let data_lit = r_escape(&data_path.to_string_lossy());
    let out_lit = r_escape(&out_path.to_string_lossy());
    // `{{` / `}}` are literal braces inside the format string.
    let script = format!(
        r#"library(arrow)
library(ggplot2)
df <- as.data.frame(arrow::read_ipc_stream("{data_lit}"))
{r_code}
if (!exists("p")) {{
  stop("plot code must assign the ggplot object to a variable named `p`")
}}
ggsave("{out_lit}", plot = p, device = png,
       width = {width}, height = {height}, dpi = {dpi}, units = "in")
"#
    );
    std::fs::write(&script_path, script)?;

    let mut cmd = tokio::process::Command::new(&rscript);
    cmd.arg(&script_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(VizError::RscriptNotFound);
        }
        Err(e) => return Err(e.into()),
    };

    let output = match tokio::time::timeout(
        Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        child.wait_with_output(),
    )
    .await
    {
        Ok(o) => o?,
        Err(_) => {
            return Err(VizError::RscriptTimeout(DEFAULT_TIMEOUT_SECS));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(VizError::RscriptFailed {
            code: output.status.code().unwrap_or(-1),
            stderr,
        });
    }

    // Read the rendered PNG back as bytes.
    let bytes = std::fs::read(&out_path)?;
    if bytes.is_empty() {
        return Err(VizError::InvalidPlotCode(
            "ggsave produced an empty file".to_string(),
        ));
    }

    // The tempdir (IPC + script + PNG) is removed when `tmp` drops.
    drop(tmp);
    Ok(bytes)
}

/// Escape a path for safe interpolation into a double-quoted R string literal:
/// escape backslash and double-quote. Combined with the surrounding `"..."` in
/// the format string, this keeps arbitrary paths out of R's parser.
fn r_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{Float64Array, Int32Array};
    use arrow::datatypes::{DataType, Field, Schema};
    use std::sync::Arc;

    fn sample_batches() -> Vec<RecordBatch> {
        let schema = Arc::new(Schema::new(vec![
            Field::new("x", DataType::Int32, false),
            Field::new("y", DataType::Float64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int32Array::from(vec![1, 2, 3, 4, 5])),
                Arc::new(Float64Array::from(vec![1.0, 4.0, 9.0, 16.0, 25.0])),
            ],
        )
        .expect("build batch");
        vec![batch]
    }

    /// End-to-end: serialize batches, run ggplot2 via Rscript, check the PNG.
    /// Requires R + the `arrow`/`ggplot2` packages on PATH (the r45 conda env).
    #[tokio::test]
    async fn test_render_png_via_rscript() {
        let out = tempfile::NamedTempFile::new().unwrap().keep().unwrap().1;
        let code = "p <- ggplot(df, aes(x = x, y = y)) + geom_point() + geom_line()";

        render_png(
            &sample_batches(),
            code,
            &out,
            Some(6.0),
            Some(4.0),
            Some(100.0),
        )
        .await
        .expect("render should succeed");

        let bytes = std::fs::read(&out).expect("read output png");
        assert!(bytes.len() > 100, "PNG too small");
        assert_eq!(
            &bytes[0..4],
            &[0x89, b'P', b'N', b'G'],
            "not a PNG signature"
        );
        eprintln!("render_png OK: {} bytes", bytes.len());
        let _ = std::fs::remove_file(&out);
    }

    /// Empty plot code is rejected before touching R.
    #[tokio::test]
    async fn test_empty_plot_code_rejected() {
        let out = tempfile::NamedTempFile::new().unwrap().keep().unwrap().1;
        let err = render_png(&sample_batches(), "   ", &out, None, None, None)
            .await
            .expect_err("empty code should fail");
        assert!(matches!(err, VizError::InvalidPlotCode(_)));
        let _ = std::fs::remove_file(&out);
    }

    /// Plot code that does not assign `p` surfaces an RscriptFailed error.
    #[tokio::test]
    async fn test_missing_p_assignment_fails() {
        let out = tempfile::NamedTempFile::new().unwrap().keep().unwrap().1;
        let err = render_png(
            &sample_batches(),
            "ggplot(df, aes(x = x, y = y))",
            &out,
            None,
            None,
            None,
        )
        .await
        .expect_err("missing p should fail");
        assert!(matches!(err, VizError::RscriptFailed { .. }));
        let _ = std::fs::remove_file(&out);
    }
}
