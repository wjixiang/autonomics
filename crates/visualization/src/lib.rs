pub mod error;
pub mod render;

#[cfg(test)]
use extendr_api::prelude::*;

/// Smoke test: render a ggplot2 figure to a PNG on disk and read it back.
#[test]
fn test_ggplot() {
    extendr_engine::with_r(|| -> extendr_api::Result<()> {
        eval_string("library(ggplot2)")?;

        let path = tempfile::NamedTempFile::new()
            .map_err(|e| extendr_api::Error::from(e.to_string()))?
            .keep()
            .map_err(|e| extendr_api::Error::from(e.to_string()))?
            .1;
        let path_str = path.to_string_lossy().into_owned();

        let code = format!(
            r#"
            df <- data.frame(x = 1:10, y = (1:10)^2)
            p <- ggplot(df, aes(x = x, y = y)) + geom_point() + geom_line()
            ggsave("{path_str}", plot = p, device = png, width = 6, height = 4, dpi = 100)
            "#
        );
        eval_string(&code)?;

        let len = std::fs::metadata(&path)
            .map_err(|e| extendr_api::Error::from(e.to_string()))?
            .len();
        assert!(len > 0, "ggplot2 produced an empty PNG file");
        eprintln!("ggplot2 (file) OK: {} bytes", len);

        let _ = std::fs::remove_file(&path);
        Ok(())
    })
    .expect("ggplot2 test failed");
}

/// Render a ggplot2 figure and return the PNG bytes **through memory** via
/// extendr (no file ever leaves R's side / reaches Rust's filesystem API).
/// The tempfile is created, written, read, and unlinked inside a single R
/// call; only the resulting raw vector crosses the extendr boundary.
#[test]
fn test_ggplot_to_bytes() {
    let bytes: Vec<u8> = extendr_engine::with_r(|| -> extendr_api::Result<Vec<u8>> {
        // The whole render pipeline runs in R; the last expression is the
        // raw vector of PNG bytes, which eval_string returns as an Robj.
        let robj = eval_string(
            r#"
            library(ggplot2)
            df <- data.frame(x = 1:10, y = (1:10)^2)
            p <- ggplot(df, aes(x = x, y = y)) + geom_point() + geom_line()
            tf <- tempfile(fileext = ".png")
            ggsave(tf, plot = p, device = png, width = 6, height = 4, dpi = 100)
            bytes <- readBin(tf, "raw", n = file.info(tf)$size)
            unlink(tf)
            bytes
            "#,
        )?;
        let slice = robj
            .as_raw_slice()
            .ok_or_else(|| extendr_api::Error::from("expected a raw vector from R"))?;
        Ok(slice.to_vec())
    })
    .expect("ggplot2 in-memory test failed");

    // PNG magic bytes — proves we got real raster data back through memory.
    assert!(bytes.len() > 100, "PNG byte vector too small");
    assert_eq!(
        &bytes[0..4],
        &[0x89, b'P', b'N', b'G'],
        "not a PNG signature"
    );
    eprintln!("ggplot2 (memory) OK: {} PNG bytes via extendr", bytes.len());
}

#[cfg(test)]
mod df_to_r_tests {
    use super::*;
    use std::sync::Arc;

    use arrow::array::{Float64Array, Int32Array};
    use arrow::datatypes::{DataType, Field, Schema};
    use arrow::ipc::writer::StreamWriter;
    use arrow::record_batch::RecordBatch;
    use datafusion::prelude::*;

    /// Serialize `RecordBatch`es (sharing one schema) into Arrow **IPC stream**
    /// bytes — a stable, version-tolerant on-the-wire format that R's `arrow`
    /// package can read back into a Table / data.frame.
    fn batches_to_ipc_stream(
        schema: &arrow::datatypes::SchemaRef,
        batches: &[RecordBatch],
    ) -> extendr_api::Result<Vec<u8>> {
        let map_err = |e: arrow::error::ArrowError| extendr_api::Error::from(e.to_string());
        let mut buf = Vec::new();
        let mut writer = StreamWriter::try_new(&mut buf, schema).map_err(map_err)?;
        for b in batches {
            writer.write(b).map_err(map_err)?;
        }
        writer.finish().map_err(map_err)?;
        drop(writer);
        Ok(buf)
    }

    /// Build a small DataFusion DataFrame (one Int32 + one Float64 column, plus
    /// a computed column) and collect it to Arrow `RecordBatch`es.
    fn build_batches() -> (arrow::datatypes::SchemaRef, Vec<RecordBatch>) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        rt.block_on(async {
            let ctx = SessionContext::new();
            let schema = Arc::new(Schema::new(vec![
                Field::new("x", DataType::Int32, false),
                Field::new("y", DataType::Float64, false),
            ]));
            let batch = RecordBatch::try_new(
                schema.clone(),
                vec![
                    Arc::new(Int32Array::from(vec![1, 2, 3])),
                    Arc::new(Float64Array::from(vec![1.0, 4.0, 9.0])),
                ],
            )
            .expect("build batch");
            ctx.register_batch("t", batch).expect("register_batch");
            let df = ctx
                .sql("SELECT x, y, y * y AS y2 FROM t ORDER BY x")
                .await
                .expect("sql");
            let batches = df.collect().await.expect("collect");
            let schema = batches[0].schema();
            (schema, batches)
        })
    }

    /// End-to-end: DataFusion DataFrame → IPC stream bytes → R `arrow` reads it
    /// → R `data.frame`. Verifies row count and a computed column's values.
    #[test]
    fn test_datafusion_df_to_r() {
        let (schema, batches) = build_batches();
        let ipc = batches_to_ipc_stream(&schema, &batches).expect("serialize IPC");

        extendr_engine::with_r(|| -> extendr_api::Result<()> {
            let raw = r!(Raw::from_bytes(&ipc));
            // rawConnection wraps the raw vector as a readable stream for arrow.
            let df_obj = eval_string_with_params(
                "as.data.frame(arrow::read_ipc_stream(rawConnection(param.0)))",
                &[&raw],
            )?;

            let nrow = eval_string_with_params("nrow(param.0)", &[&df_obj])?
                .as_integer()
                .ok_or("nrow not int")?;
            assert_eq!(nrow, 3, "expected 3 rows in R data.frame");

            let y2 = eval_string_with_params("param.0$y2", &[&df_obj])?;
            let vals: Vec<f64> = y2
                .as_real_slice()
                .ok_or("y2 not a numeric vector")?
                .to_vec();
            assert_eq!(vals, vec![1.0, 16.0, 81.0]);
            eprintln!("DataFusion -> R data.frame OK: {nrow} rows, y2 = {vals:?}");
            Ok(())
        })
        .expect("df->r test failed");
    }
}
