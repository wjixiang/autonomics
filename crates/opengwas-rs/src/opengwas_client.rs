use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::Result;
use reqwest::{Client, header, multipart};
use rusqlite::Connection;
use serde_json::Value;

use file_base::OpendalFileStorage;

use crate::types::*;

const BASE_URL: &str = "https://api.opengwas.io/api";

// ---------------------------------------------------------------------------
// SQLite helper: Row → GwasInfo
// ---------------------------------------------------------------------------

fn row_to_gwasinfo(row: &rusqlite::Row<'_>) -> rusqlite::Result<GwasInfo> {
    Ok(GwasInfo {
        id: row.get(0)?,
        trait_: row.get(1)?,
        build: row.get(2)?,
        group_name: row.get(3)?,
        category: row.get(4)?,
        subcategory: row.get(5)?,
        population: row.get(6)?,
        sex: row.get(7)?,
        author: row.get(8)?,
        nsnp: row.get(9)?,
        sample_size: row.get(10)?,
        year: row.get(11)?,
        ontology: row.get(12)?,
        unit: row.get(13)?,
        ncase: row.get(14)?,
        ncontrol: row.get(15)?,
        study_design: row.get(16)?,
        covariates: row.get(17)?,
        coverage: row.get(18)?,
        qc_prior_to_upload: row.get(19)?,
        imputation_panel: row.get(20)?,
        beta_transformation: row.get(21)?,
        doi: row.get(22)?,
        consortium: row.get(23)?,
        pmid: row.get(24)?,
        sd: row.get(25)?,
        mr: row.get(26)?,
        priority: row.get(27)?,
        note: row.get(28)?,
        is_nc: row.get(29)?,
    })
}

const SELECT_COLUMNS: &str = "
    id, trait_, build, group_name, category, subcategory,
    population, sex, author, nsnp, sample_size, year,
    ontology, unit, ncase, ncontrol, study_design,
    covariates, coverage, qc_prior_to_upload,
    imputation_panel, beta_transformation, doi,
    consortium, pmid, sd, mr, priority, note, is_nc
";

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct OpengwasClient {
    client: Client,
    /// In-memory SQLite cache for gwasinfo metadata.
    /// Initialised once on first access via [`ensure_db_loaded`](Self::ensure_db_loaded).
    db: OnceLock<Arc<Mutex<Connection>>>,
}

impl OpengwasClient {
    /// Create a new client, reading the token from:
    /// 1. the provided `token` argument (if `Some`);
    /// 2. the `OPENGWAS_TOKEN` environment variable (if set).
    ///
    /// The token should **not** include the `Bearer` prefix — it is
    /// automatically prepended.
    ///
    /// Panics if no token can be resolved.
    pub fn new<S: Into<String>>(token: Option<S>) -> Self {
        let token = match token {
            Some(t) => Some(t.into()),
            None => std::env::var("OPENGWAS_TOKEN").ok(),
        };
        let full = format!(
            "Bearer {}",
            token.expect("no token provided and OPENGWAS_TOKEN env var not set")
        );
        let mut headers = header::HeaderMap::new();
        let mut auth = header::HeaderValue::from_str(&full).expect("invalid token");
        auth.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth);
        let client = Client::builder()
            .default_headers(headers)
            .build()
            .expect("failed to build HTTP client");
        Self {
            client,
            db: OnceLock::new(),
        }
    }

    /// Create a client without any authentication header.
    ///
    /// Only public endpoints (`/status`, `/batches`) will work.
    pub fn new_no_auth() -> Self {
        let client = Client::builder()
            .build()
            .expect("failed to build HTTP client");
        Self {
            client,
            db: OnceLock::new(),
        }
    }

    // =======================================================================
    // SQLite helpers (private)
    // =======================================================================

    /// Create the in-memory SQLite database and the `gwasinfo` table.
    fn init_db() -> Result<Arc<Mutex<Connection>>> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS gwasinfo (
                id                  TEXT PRIMARY KEY,
                trait_              TEXT,
                build               TEXT,
                group_name          TEXT,
                category            TEXT,
                subcategory         TEXT,
                population          TEXT,
                sex                 TEXT,
                author              TEXT,
                nsnp                INTEGER,
                sample_size         INTEGER,
                year                INTEGER,
                ontology            TEXT,
                unit                TEXT,
                ncase               INTEGER,
                ncontrol            INTEGER,
                study_design        TEXT,
                covariates          TEXT,
                coverage            TEXT,
                qc_prior_to_upload  TEXT,
                imputation_panel    TEXT,
                beta_transformation TEXT,
                doi                 TEXT,
                consortium          TEXT,
                pmid                INTEGER,
                sd                  REAL,
                mr                  INTEGER,
                priority            INTEGER,
                note                TEXT,
                is_nc               INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_gwasinfo_trait
                ON gwasinfo(trait_);
            CREATE INDEX IF NOT EXISTS idx_gwasinfo_author
                ON gwasinfo(author);
            CREATE INDEX IF NOT EXISTS idx_gwasinfo_population
                ON gwasinfo(population);",
        )?;
        Ok(Arc::new(Mutex::new(conn)))
    }

    /// Insert a batch of [`GwasInfo`] rows into the in-memory SQLite cache.
    fn bulk_insert(conn: &Connection, infos: &[GwasInfo]) -> Result<()> {
        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM gwasinfo", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO gwasinfo (
                    id, trait_, build, group_name, category, subcategory,
                    population, sex, author, nsnp, sample_size, year,
                    ontology, unit, ncase, ncontrol, study_design,
                    covariates, coverage, qc_prior_to_upload,
                    imputation_panel, beta_transformation, doi,
                    consortium, pmid, sd, mr, priority, note, is_nc
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                    ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22,
                    ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30
                 )",
            )?;
            for info in infos {
                stmt.execute(rusqlite::params![
                    info.id,
                    info.trait_,
                    info.build,
                    info.group_name,
                    info.category,
                    info.subcategory,
                    info.population,
                    info.sex,
                    info.author,
                    info.nsnp,
                    info.sample_size,
                    info.year,
                    info.ontology,
                    info.unit,
                    info.ncase,
                    info.ncontrol,
                    info.study_design,
                    info.covariates,
                    info.coverage,
                    info.qc_prior_to_upload,
                    info.imputation_panel,
                    info.beta_transformation,
                    info.doi,
                    info.consortium,
                    info.pmid,
                    info.sd,
                    info.mr,
                    info.priority,
                    info.note,
                    info.is_nc,
                ])?;
            }
        } // stmt dropped here, releasing borrow on tx
        tx.commit()?;
        Ok(())
    }

    /// Ensure the in-memory SQLite cache is populated.
    ///
    /// On the first call this fetches all gwasinfo metadata from the remote
    /// API and inserts it into the cache. Subsequent calls return immediately.
    async fn ensure_db_loaded(&self) -> Result<()> {
        // Fast path: already initialised.
        if self.db.get().is_some() {
            return Ok(());
        }

        // Fetch from remote API.
        let raw: Value = self.get("/gwasinfo").await?;
        // API returns a map keyed by GWAS ID: { "ieu-a-2": { ... }, ... }
        let infos: Vec<GwasInfo> = raw
            .as_object()
            .map(|m| {
                m.values()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            })
            .ok_or_else(|| {
                anyhow::anyhow!("failed to deserialize gwasinfo response: expected a map")
            })?;

        let db = Self::init_db()?;

        // Bulk insert inside spawn_blocking.
        {
            let db_clone = Arc::clone(&db);
            tokio::task::spawn_blocking(move || {
                let guard = db_clone.lock().unwrap();
                Self::bulk_insert(&guard, &infos)
            })
            .await
            .expect("spawn_blocking task panicked")?;
        }

        // Store in OnceLock. If another task raced us its value wins; our
        // data is harmlessly dropped.
        let _ = self.db.set(db);
        Ok(())
    }

    // =======================================================================
    // HTTP helpers (private)
    // =======================================================================

    async fn get(&self, path: &str) -> Result<Value> {
        let url = format!("{BASE_URL}{path}");
        let resp = self.client.get(&url).send().await?;
        resp.error_for_status_ref()?;
        Ok(resp.json().await?)
    }

    async fn get_raw(&self, path: &str) -> Result<String> {
        let url = format!("{BASE_URL}{path}");
        let resp = self.client.get(&url).send().await?;
        resp.error_for_status_ref()?;
        Ok(resp.text().await?)
    }

    async fn post_json<T: serde::Serialize + ?Sized>(&self, path: &str, body: &T) -> Result<Value> {
        let url = format!("{BASE_URL}{path}");
        let resp = self.client.post(&url).json(body).send().await?;
        resp.error_for_status_ref()?;
        Ok(resp.json().await?)
    }

    async fn delete(&self, path: &str) -> Result<Value> {
        let url = format!("{BASE_URL}{path}");
        let resp = self.client.delete(&url).send().await?;
        resp.error_for_status_ref()?;
        Ok(resp.json().await?)
    }

    // =======================================================================
    // Status
    // =======================================================================

    /// `GET /status` — check that API services are running (no auth required).
    pub async fn status(&self) -> Result<Value> {
        self.get("/status").await
    }

    // =======================================================================
    // Batches
    // =======================================================================

    /// `GET /batches` — list existing data batches (no auth required).
    pub async fn batches(&self) -> Result<Value> {
        self.get("/batches").await
    }

    // =======================================================================
    // User
    // =======================================================================

    /// `GET /user` — get information about the authenticated user.
    pub async fn user(&self) -> Result<Value> {
        self.get("/user").await
    }

    // =======================================================================
    // GwasInfo (SQLite-cached)
    // =======================================================================

    /// Get metadata for **all** GWAS datasets accessible to you.
    ///
    /// Results are cached in an in-memory SQLite database after the first
    /// call. Subsequent calls read from the cache (no network request).
    pub async fn gwasinfo_all(&self) -> Result<Vec<GwasInfo>> {
        self.ensure_db_loaded().await?;

        let db = self.db.get().expect("db initialised by ensure_db_loaded");
        let db_clone = Arc::clone(db);
        tokio::task::spawn_blocking(move || {
            let guard = db_clone.lock().unwrap();
            let mut stmt = guard.prepare(&format!("SELECT {SELECT_COLUMNS} FROM gwasinfo"))?;
            let rows = stmt.query_map([], row_to_gwasinfo)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .await
        .expect("spawn_blocking task panicked")
        .map_err(Into::into)
    }

    /// Get metadata for specific GWAS datasets by ID.
    ///
    /// Results are served from the in-memory cache (populated on first use).
    /// If any requested IDs are not in the cache, they are simply absent
    /// from the returned vector.
    pub async fn gwasinfo(&self, req: &GwasInfoRequest) -> Result<Vec<GwasInfo>> {
        if req.id.is_empty() {
            return Ok(vec![]);
        }

        self.ensure_db_loaded().await?;

        let db = self.db.get().expect("db initialised by ensure_db_loaded");
        let ids = req.id.clone();
        let db_clone = Arc::clone(db);
        tokio::task::spawn_blocking(move || {
            let guard = db_clone.lock().unwrap();
            let placeholders: String = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!("SELECT {SELECT_COLUMNS} FROM gwasinfo WHERE id IN ({placeholders})");
            let mut stmt = guard.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::types::ToSql> = ids
                .iter()
                .map(|s| s as &dyn rusqlite::types::ToSql)
                .collect();
            let rows = stmt.query_map(params.as_slice(), row_to_gwasinfo)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .await
        .expect("spawn_blocking task panicked")
        .map_err(Into::into)
    }

    /// Force a refresh of the in-memory gwasinfo cache by re-fetching
    /// all metadata from the remote API.
    pub async fn gwasinfo_refresh(&self) -> Result<Vec<GwasInfo>> {
        self.ensure_db_loaded().await?;

        let db = self.db.get().expect("db initialised by ensure_db_loaded");

        // Fetch fresh data from remote.
        let raw: Value = self.get("/gwasinfo").await?;
        let infos: Vec<GwasInfo> = raw
            .as_object()
            .map(|m| {
                m.values()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            })
            .ok_or_else(|| {
                anyhow::anyhow!("failed to deserialize gwasinfo response: expected a map")
            })?;

        // Replace table contents inside spawn_blocking.
        let db_clone = Arc::clone(db);
        tokio::task::spawn_blocking(move || {
            let guard = db_clone.lock().unwrap();
            Self::bulk_insert(&guard, &infos)?;
            Ok::<_, anyhow::Error>(infos)
        })
        .await
        .expect("spawn_blocking task panicked")
    }

    /// Return the number of cached GWAS datasets.
    pub async fn gwasinfo_count(&self) -> Result<i64> {
        self.ensure_db_loaded().await?;

        let db = self.db.get().expect("db initialised by ensure_db_loaded");
        let db_clone = Arc::clone(db);
        tokio::task::spawn_blocking(move || {
            let guard = db_clone.lock().unwrap();
            guard.query_row("SELECT COUNT(*) FROM gwasinfo", [], |row| row.get(0))
        })
        .await
        .expect("spawn_blocking task panicked")
        .map_err(Into::into)
    }

    /// Search cached GWAS datasets by keyword using a SQL LIKE query on the
    /// given indexed column (`trait_`, `author`, or `population`).
    ///
    /// Returns at most `limit` results.
    pub async fn gwasinfo_search(
        &self,
        keyword: &str,
        field: &str,
        limit: i64,
    ) -> Result<Vec<GwasInfo>> {
        const ALLOWED: &[&str] = &["trait_", "author", "population"];
        if !ALLOWED.contains(&field) {
            anyhow::bail!("invalid search field: {field} (allowed: trait, author, population)");
        }

        self.ensure_db_loaded().await?;

        let db = self.db.get().expect("db initialised by ensure_db_loaded");
        let pattern = format!("%{}%", keyword.to_lowercase());
        let field = field.to_string();
        let db_clone = Arc::clone(db);
        tokio::task::spawn_blocking(move || {
            let guard = db_clone.lock().unwrap();
            let sql =
                format!("SELECT {SELECT_COLUMNS} FROM gwasinfo WHERE LOWER(\"{field}\") LIKE ?1 LIMIT ?2");
            let mut stmt = guard.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params![pattern, limit], row_to_gwasinfo)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .await
        .expect("spawn_blocking task panicked")
        .map_err(Into::into)
    }

    /// `POST /gwasinfo/files` — get download URLs for dataset files
    /// (`.vcf.gz`, `.vcf.gz.tbi`, `_report.html`). URLs expire in 2 hours.
    ///
    /// This method always hits the remote API; it is **not** cached.
    pub async fn gwasinfo_files(&self, req: &GwasInfoFilesRequest) -> Result<Value> {
        self.post_json("/gwasinfo/files", req).await
    }

    // =======================================================================
    // Associations
    // =======================================================================

    /// `POST /associations` — get specific variant associations from specific
    /// GWAS datasets.
    pub async fn associations(&self, req: &AssociationsRequest) -> Result<Value> {
        self.post_json("/associations", req).await
    }

    // =======================================================================
    // Tophits
    // =======================================================================

    /// `POST /tophits` — extract top hits based on a p-value threshold
    /// from a GWAS dataset.
    pub async fn tophits(&self, req: &TophitsRequest) -> Result<Value> {
        self.post_json("/tophits", req).await
    }

    // =======================================================================
    // PheWAS
    // =======================================================================

    /// `POST /phewas` — perform PheWAS of specified variants across all
    /// available GWAS datasets (only p ≤ 0.01).
    pub async fn phewas(&self, req: &PhewasRequest) -> Result<Value> {
        self.post_json("/phewas", req).await
    }

    // =======================================================================
    // Variants
    // =======================================================================

    /// `POST /variants/rsid` — obtain variant information by rs IDs.
    pub async fn variants_rsid(&self, req: &VariantsRsidRequest) -> Result<Value> {
        self.post_json("/variants/rsid", req).await
    }

    /// `POST /variants/chrpos` — obtain variant information by chr:pos.
    pub async fn variants_chrpos(&self, req: &VariantsChrposRequest) -> Result<Value> {
        self.post_json("/variants/chrpos", req).await
    }

    /// `GET /variants/gene/{gene}` — obtain variant information for a gene
    /// (Ensembl or Entrez ID).
    pub async fn variants_gene(&self, gene: &str, radius: Option<i32>) -> Result<Value> {
        let mut path = format!("/variants/gene/{gene}");
        if let Some(r) = radius {
            path = format!("{path}?radius={r}");
        }
        self.get(&path).await
    }

    /// `POST /variants/afl2` — obtain allele frequency and LD scores for
    /// variants.
    pub async fn variants_afl2(&self, req: &VariantsAfl2Request) -> Result<Value> {
        self.post_json("/variants/afl2", req).await
    }

    /// `GET /variants/afl2/snplist` — get list of rsids variable across
    /// populations for ancestry analyses.
    pub async fn variants_afl2_snplist(&self) -> Result<Value> {
        self.get("/variants/afl2/snplist").await
    }

    // =======================================================================
    // LD
    // =======================================================================

    /// `POST /ld/clump` — perform LD clumping on a set of rs IDs using 1000
    /// Genomes reference data.
    pub async fn ld_clump(&self, req: &LdClumpRequest) -> Result<Value> {
        self.post_json("/ld/clump", req).await
    }

    /// `POST /ld/matrix` — for a list of SNPs get the LD R values.
    pub async fn ld_matrix(&self, req: &LdMatrixRequest) -> Result<Value> {
        self.post_json("/ld/matrix", req).await
    }

    /// `POST /ld/reflookup` — lookup whether rsids are present in the LD
    /// reference panel.
    pub async fn ld_reflookup(&self, req: &LdReflookupRequest) -> Result<Value> {
        self.post_json("/ld/reflookup", req).await
    }

    // =======================================================================
    // Edit — metadata
    // =======================================================================

    /// `POST /edit/add` — add new GWAS metadata.
    pub async fn edit_add(&self, req: &EditAddRequest) -> Result<Value> {
        self.post_json("/edit/add", req).await
    }

    /// `POST /edit/edit` — edit existing GWAS metadata.
    pub async fn edit_edit(&self, req: &EditEditRequest) -> Result<Value> {
        self.post_json("/edit/edit", req).await
    }

    /// `GET /edit/list` — list datasets added by the user. Supports filtering
    /// by state (`draft` / `released`) and pagination for released datasets.
    pub async fn edit_list(&self, query: &EditListQuery) -> Result<Value> {
        let q = serde_urlencoded::to_string(query)?;
        self.get(&format!("/edit/list?{q}")).await
    }

    /// `GET /edit/check/{gwas_id}` — get metadata about a specific GWAS
    /// dataset.
    pub async fn edit_check(&self, gwas_id: &str) -> Result<Value> {
        self.get(&format!("/edit/check/{gwas_id}")).await
    }

    /// `GET /edit/state/{gwas_id}` — check DAG runs and task instances
    /// related to the dataset.
    pub async fn edit_state(&self, gwas_id: &str) -> Result<Value> {
        self.get(&format!("/edit/state/{gwas_id}")).await
    }

    /// `DELETE /edit/delete/draft/{gwas_id}` — force the QC pipeline to
    /// fail, delete uploaded files and QC products, and optionally delete
    /// metadata. Available until the dataset is submitted for approval.
    pub async fn edit_delete_draft(&self, gwas_id: &str) -> Result<Value> {
        self.delete(&format!("/edit/delete/draft/{gwas_id}")).await
    }

    // =======================================================================
    // Edit — upload
    // =======================================================================

    /// `POST /edit/upload` — upload a GWAS summary stats file.
    ///
    /// `file_path` is the local path to the text file (can be gzipped).
    /// The `id` must be the identifier the summary stats belong to.
    pub async fn edit_upload(
        &self,
        id: &str,
        file_path: &Path,
        opts: &EditUploadOptions,
    ) -> Result<Value> {
        let file_name = file_path
            .file_name()
            .expect("file has no name")
            .to_string_lossy()
            .to_string();

        let path = file_path.to_owned();
        let file_bytes = tokio::task::spawn_blocking(move || std::fs::read(path)).await??;
        let file_part = multipart::Part::bytes(file_bytes)
            .file_name(file_name)
            .mime_str("application/octet-stream")?;

        let mut form = multipart::Form::new()
            .text("id", id.to_string())
            .text("delimiter", opts.delimiter.clone())
            .text("header", opts.header.clone())
            .text("gzipped", opts.gzipped.clone())
            .text("chr_col", opts.chr_col.to_string())
            .text("pos_col", opts.pos_col.to_string())
            .text("ea_col", opts.ea_col.to_string())
            .text("oa_col", opts.oa_col.to_string())
            .text("beta_col", opts.beta_col.to_string())
            .text("se_col", opts.se_col.to_string())
            .text("pval_col", opts.pval_col.to_string())
            .part("gwas_file", file_part);

        if let Some(v) = opts.snp_col {
            form = form.text("snp_col", v.to_string());
        }
        if let Some(v) = opts.eaf_col {
            form = form.text("eaf_col", v.to_string());
        }
        if let Some(v) = opts.oaf_col {
            form = form.text("oaf_col", v.to_string());
        }
        if let Some(v) = opts.imp_z_col {
            form = form.text("imp_z_col", v.to_string());
        }
        if let Some(v) = opts.imp_info_col {
            form = form.text("imp_info_col", v.to_string());
        }
        if let Some(v) = opts.ncase_col {
            form = form.text("ncase_col", v.to_string());
        }
        if let Some(v) = opts.ncontrol_col {
            form = form.text("ncontrol_col", v.to_string());
        }
        if let Some(ref v) = opts.md5 {
            form = form.text("md5", v.clone());
        }
        if let Some(v) = opts.nsnp {
            form = form.text("nsnp", v.to_string());
        }

        let url = format!("{BASE_URL}/edit/upload");
        let resp = self.client.post(&url).multipart(form).send().await?;
        resp.error_for_status_ref()?;
        Ok(resp.json().await?)
    }

    // =======================================================================
    // Quality Control
    // =======================================================================

    /// `GET /quality_control/list` — return all GWAS datasets requiring QC.
    pub async fn qc_list(&self) -> Result<Value> {
        self.get("/quality_control/list").await
    }

    /// `GET /quality_control/check/{id}` — view files generated for a dataset.
    pub async fn qc_check(&self, id: &str) -> Result<Value> {
        self.get(&format!("/quality_control/check/{id}")).await
    }

    /// `GET /quality_control/report/{gwas_id}` — view the HTML QC report.
    pub async fn qc_report(&self, gwas_id: &str) -> Result<String> {
        self.get_raw(&format!("/quality_control/report/{gwas_id}"))
            .await
    }

    /// `GET /quality_control/submit/{gwas_id}` — submit a dataset for
    /// approval.
    pub async fn qc_submit(&self, gwas_id: &str) -> Result<Value> {
        self.get(&format!("/quality_control/submit/{gwas_id}"))
            .await
    }

    /// `POST /quality_control/release` — release data from the QC process.
    pub async fn qc_release(&self, req: &QcReleaseRequest) -> Result<Value> {
        self.post_json("/quality_control/release", req).await
    }

    /// `DELETE /quality_control/delete/{id}` — delete a QC relationship
    /// (does not delete metadata or data files).
    pub async fn qc_delete(&self, id: &str) -> Result<Value> {
        self.delete(&format!("/quality_control/delete/{id}")).await
    }

    // =======================================================================
    // Download
    // =======================================================================

    /// Download a file from a URL and store it in [`OpendalFileStorage`].
    ///
    /// Uses the existing `Client` (with auth headers) to fetch the file,
    /// then writes the entire body to OpenDAL at the given path.
    pub async fn download_file_to_storage(
        &self,
        url: &str,
        storage: &OpendalFileStorage,
        path: &str,
    ) -> Result<u64> {
        let resp = self.client.get(url).send().await?;
        resp.error_for_status_ref()?;

        let bytes = resp.bytes().await?;
        let size = bytes.len() as u64;

        storage
            .op
            .write(path, bytes.to_vec())
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        Ok(size)
    }
}

// ---------------------------------------------------------------------------
// Upload options
// ---------------------------------------------------------------------------

/// Options for [`OpengwasClient::edit_upload`].
#[derive(Debug, Clone)]
pub struct EditUploadOptions {
    pub chr_col: i32,
    pub pos_col: i32,
    pub ea_col: i32,
    pub oa_col: i32,
    pub beta_col: i32,
    pub se_col: i32,
    pub pval_col: i32,
    pub delimiter: String,
    pub header: String,
    pub gzipped: String,
    pub snp_col: Option<i32>,
    pub eaf_col: Option<i32>,
    pub oaf_col: Option<i32>,
    pub imp_z_col: Option<i32>,
    pub imp_info_col: Option<i32>,
    pub ncase_col: Option<i32>,
    pub ncontrol_col: Option<i32>,
    pub md5: Option<String>,
    pub nsnp: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn get_client() -> OpengwasClient {
        OpengwasClient::new::<String>(None)
    }

    #[tokio::test]
    async fn test_search_by_trait() {
        let client = get_client();

        let result = client
            .gwasinfo_search("tension", "trait_", 10)
            .await
            .unwrap();

        dbg!(result);
    }
}
