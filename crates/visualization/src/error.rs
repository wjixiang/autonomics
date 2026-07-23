use thiserror::Error;

/// Errors produced by the visualization renderer.
#[derive(Debug, Error)]
pub enum VizError {
    /// A column referenced by the spec was not found in the source data.
    #[error("column not found: {0}")]
    ColumnNotFound(String),

    /// A column exists but its Arrow type cannot be coerced to the required
    /// shape (numeric or text).
    #[error("column `{column}` has unsupported type for this mapping: {ty}")]
    UnsupportedType { column: String, ty: String },

    /// The system has no `Rscript` on PATH.
    #[error("Rscript not found on PATH — is R installed?")]
    RscriptNotFound,

    /// `Rscript` ran but exited with a non-zero status. Carries the captured
    /// stderr so the caller can see the R-side error.
    #[error("Rscript failed (exit code {code}):\n{stderr}")]
    RscriptFailed { code: i32, stderr: String },

    /// `Rscript` was killed because it exceeded the render timeout.
    #[error("Rscript timed out after {0} seconds")]
    RscriptTimeout(u64),

    /// The supplied plot code is empty or not valid R.
    #[error("invalid plot code: {0}")]
    InvalidPlotCode(String),

    /// Arrow serialization error (writing the IPC stream for R to read).
    #[error(transparent)]
    Arrow(#[from] arrow::error::ArrowError),

    /// I/O error while writing temp files or the output artifact.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = VizError> = std::result::Result<T, E>;
