#![allow(dead_code)]
//! Shared test helpers for eutils E2E tests.
//!
//! # Concurrency model
//!
//! `cargo test` runs each test *binary* as a separate process and executes the
//! binaries concurrently. NCBI rate-limits to ~3 requests/second per IP, so
//! uncoordinated parallel binaries trigger 429s and truncated responses.
//!
//! We defend at two layers:
//! 1. `#[serial]` (from `serial_test`) — serializes tests *within* a binary.
//! 2. `rate_limit()` — a cross-process gate so *all* binaries coordinate.
//!
//! `rate_limit()` binds a fixed localhost TCP port as an exclusive mutex: only
//! one test process across all binaries holds it at a time. It holds the port
//! for the spacing interval, then releases it. This spaces request initiations
//! globally to ~2 req/s, safely under NCBI's limit, with no extra dependencies.

use std::net::TcpListener;

/// Create a test client with identifiable tool/email for NCBI.
pub fn test_client() -> eutils::EutilsClient {
    eutils::EutilsClient::new("eutils-rs-tests", "eutils-rs-tests@localhost", None)
}

/// Fixed localhost port used as a cross-process mutex. Chosen from the
/// ephemeral range but unlikely to collide with real services on a dev machine.
const RATE_LOCK_ADDR: &str = "127.0.0.1:38437";

/// Wait between API calls to respect NCBI's ~3 req/s rate limit (without API
/// key), coordinating across all concurrently-running test binaries via a
/// shared TCP-port lock so the aggregate request rate stays under the limit.
pub fn rate_limit() {
    // Spin until we win the bind (acquire the cross-process lock).
    let listener = loop {
        match TcpListener::bind(RATE_LOCK_ADDR) {
            Ok(l) => break l,
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(40)),
        }
    };
    // Hold the lock for the spacing interval, then drop to release it for the
    // next waiting process. ~450 ms => ~2 req/s globally.
    std::thread::sleep(std::time::Duration::from_millis(450));
    drop(listener);
}

// Re-export the serial macro for convenience.
pub use serial_test::serial;

/// Whether NCBI's ESpell endpoint is currently usable. NCBI intermittently
/// breaks ESpell (HTTP 500 with an empty body); when it is down we skip the
/// ESpell tests rather than report a failure for an NCBI-side outage.
pub async fn espell_available() -> bool {
    match test_client().espell("pubmed", "CRISPR").await {
        Ok(_) => true,
        Err(e) => {
            eprintln!("note: skipping ESpell test — NCBI endpoint unavailable: {e}");
            false
        }
    }
}

/// Whether NCBI's EGQuery endpoint is currently usable. NCBI intermittently
/// redirects EGQuery (301) to a dead host or returns HTTP 500.
pub async fn egquery_available() -> bool {
    match test_client().egquery("CRISPR").await {
        Ok(_) => true,
        Err(e) => {
            eprintln!("note: skipping EGQuery test — NCBI endpoint unavailable: {e}");
            false
        }
    }
}

// Well-known PMIDs used across tests (unlikely to disappear from PubMed).
pub const PMID_CRISPR: &str = "29474904"; // First CRISPR in human embryos
pub const PMID_BRCA1: &str = "20109048"; // BRCA1 review article
pub const PMID_RNA_SEQ: &str = "31819223"; // RNA-seq benchmarking
