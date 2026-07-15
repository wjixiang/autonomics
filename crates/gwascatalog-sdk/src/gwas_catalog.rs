//! GWAS Catalog Summary Statistics Database API client.
//!
//! <https://www.ebi.ac.uk/gwas/summary-statistics/docs/>
//!
//! All endpoints are read-only (`GET`). Responses use HAL format with
//! `_links` (pagination: `first`, `next`) and `_embedded` (data).

use serde::Deserialize;
use thiserror::Error;

const BASE_URL: &str = "https://www.ebi.ac.uk/gwas/summary-statistics/api";

#[derive(Debug, Error)]
pub enum GwasCatalogError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API returned error: {status} {message}")]
    Api { status: u16, message: String },
}

// ── Data types ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct HalLink {
    pub href: String,
}

/// HAL `_links` object. Fields appear depending on resource type.
#[derive(Debug, Default, Deserialize)]
pub struct HalLinks {
    #[serde(default, rename = "self")]
    pub self_: Option<HalLink>,
    #[serde(default, rename = "first")]
    pub first: Option<HalLink>,
    #[serde(default, rename = "next")]
    pub next: Option<HalLink>,
    #[serde(default, rename = "associations")]
    pub associations: Option<HalLink>,
    #[serde(default, rename = "studies")]
    pub studies: Option<HalLink>,
    #[serde(default, rename = "traits")]
    pub traits: Option<HalLink>,
    #[serde(default, rename = "chromosomes")]
    pub chromosomes: Option<HalLink>,
    #[serde(
        default,
        rename = "trait",
        deserialize_with = "single_or_vec::deserialize"
    )]
    pub trait_: Option<Vec<HalLink>>,
    #[serde(default, rename = "variant")]
    pub variant: Option<HalLink>,
    #[serde(default, rename = "study")]
    pub study: Option<HalLink>,
    #[serde(default, rename = "ols")]
    pub ols: Option<HalLink>,
    #[serde(default, rename = "gwas_catalog")]
    pub gwas_catalog: Option<HalLink>,
}

/// A single variant–study association record.
///
/// | Field | Type | Description |
/// |---|---|---|
/// | `variant_id` | String | rsid of the variant |
/// | `chromosome` | u32 | Chromosome number (X=23, Y=24, MT=25) |
/// | `base_pair_location` | u64 | Base pair location |
/// | `study_accession` | String | Study accession (e.g. GCST005038) |
/// | `trait_` | Vec\<String\> | EFO trait URI(s) |
/// | `p_value` | f64 | P-value of the association |
/// | `code` | Option\<u32\> | Harmonisation outcome code |
/// | `effect_allele` | Option\<String\> | Effect allele |
/// | `other_allele` | Option\<String\> | Other allele |
/// | `effect_allele_frequency` | Option\<f64\> | Effect allele frequency |
/// | `odds_ratio` | Option\<f64\> | Odds ratio |
/// | `ci_lower` | Option\<f64\> | CI lower bound |
/// | `ci_upper` | Option\<f64\> | CI upper bound |
/// | `beta` | Option\<f64\> | Beta coefficient |
/// | `se` | Option\<f64\> | Standard error of beta |
///
/// Default values are harmonised; use `reveal=raw` or `reveal=all` to access
/// original data (harmonised fields get `hm_` prefix with `reveal=all`).
#[derive(Debug, Deserialize)]
pub struct Association {
    pub variant_id: String,
    pub chromosome: u32,
    pub base_pair_location: u64,
    pub study_accession: String,
    #[serde(rename = "trait")]
    pub trait_: Vec<String>,
    #[serde(with = "p_value_serde")]
    pub p_value: f64,
    pub code: Option<u32>,
    pub effect_allele: Option<String>,
    pub other_allele: Option<String>,
    pub effect_allele_frequency: Option<f64>,
    pub odds_ratio: Option<f64>,
    pub ci_lower: Option<f64>,
    pub ci_upper: Option<f64>,
    pub beta: Option<f64>,
    pub se: Option<f64>,
    #[serde(default)]
    pub _links: HalLinks,
}

mod single_or_vec {
    use super::HalLink;
    use serde::de::{self, Deserialize, Deserializer, IntoDeserializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<HalLink>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = Option::<serde_json::Value>::deserialize(deserializer)?;
        match opt {
            None => Ok(None),
            Some(serde_json::Value::Array(arr)) => {
                let de = arr.into_deserializer();
                Vec::<HalLink>::deserialize(de)
                    .map(Some)
                    .map_err(de::Error::custom)
            }
            Some(other) => {
                let de = other.into_deserializer();
                let link = HalLink::deserialize(de).map_err(de::Error::custom)?;
                Ok(Some(vec![link]))
            }
        }
    }
}

mod p_value_serde {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<f64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = serde_json::Value::deserialize(deserializer)?;
        match v {
            serde_json::Value::String(s) => s
                .replace(' ', "")
                .parse::<f64>()
                .map_err(serde::de::Error::custom),
            serde_json::Value::Number(n) => n
                .as_f64()
                .ok_or_else(|| serde::de::Error::custom("invalid p-value number")),
            _ => Err(serde::de::Error::custom(
                "expected string or number for p_value",
            )),
        }
    }
}

/// Chromosome resource.
#[derive(Debug, Deserialize)]
pub struct Chromosome {
    pub chromosome: String,
    #[serde(default)]
    pub _links: HalLinks,
}

/// EFO trait resource.
#[derive(Debug, Deserialize)]
pub struct Trait {
    #[serde(rename = "trait")]
    pub trait_: String,
    #[serde(default)]
    pub _links: HalLinks,
}

/// Study resource.
#[derive(Debug, Deserialize)]
pub struct Study {
    pub study_accession: String,
    #[serde(default)]
    pub _links: HalLinks,
}

/// `_embedded` wrapper for association lists ( keyed by index string ).
#[derive(Debug, Default, Deserialize)]
pub struct EmbeddedAssociations {
    #[serde(default)]
    pub associations: std::collections::HashMap<String, Association>,
}

/// `_embedded` wrapper for chromosome lists.
#[derive(Debug, Default, Deserialize)]
pub struct EmbeddedChromosomes {
    #[serde(default)]
    pub chromosomes: Vec<Chromosome>,
}

/// `_embedded` wrapper for trait lists.
#[derive(Debug, Default, Deserialize)]
pub struct EmbeddedTraits {
    #[serde(default, rename = "trait")]
    pub traits: Vec<Trait>,
}

/// `_embedded` wrapper for study lists (returned by `/studies`).
#[derive(Debug, Default, Deserialize)]
pub struct EmbeddedStudies {
    #[serde(default)]
    pub studies: Vec<Vec<Study>>,
}

/// `_embedded` wrapper for trait-specific study lists (returned by `/traits/{trait}/studies`).
#[derive(Debug, Default, Deserialize)]
pub struct EmbeddedTraitStudies {
    #[serde(default)]
    pub studies: Vec<Study>,
}

/// Generic HAL paginated response.
///
/// Use `_links.next` to fetch the next page; the `start` offset in the next link
/// may not equal `previous_start + size` when filtering by p-value or base-pair range.
#[derive(Debug, Deserialize)]
#[serde(bound = "")]
pub struct PaginatedResponse<T: serde::de::DeserializeOwned> {
    #[serde(default, bound = "")]
    pub _embedded: Option<T>,
    #[serde(default)]
    pub _links: HalLinks,
}

// ── Query builders ────────────────────────────────────────────────────────

/// Query parameters for association endpoints.
///
/// | Param | Type | Description |
/// |---|---|---|
/// | `start` | usize | Offset number (default 0) |
/// | `size` | usize | Items per page (default 20) |
/// | `reveal` | RevealMode | `"raw"` for original data, `"all"` for harmonised + raw (`hm_` prefix) |
/// | `p_lower` | f64 | Lower p-value threshold (e.g. `1e-5`) |
/// | `p_upper` | f64 | Upper p-value threshold |
/// | `study_accession` | String | Filter by study accession |
#[derive(Debug, Default, Clone)]
pub struct AssociationQuery {
    pub start: Option<usize>,
    pub size: Option<usize>,
    pub reveal: Option<RevealMode>,
    pub p_lower: Option<f64>,
    pub p_upper: Option<f64>,
    pub study_accession: Option<String>,
}

/// Query parameters for chromosome-specific association endpoints.
///
/// Extends `AssociationQuery` with base-pair location filtering (only works on
/// `/chromosomes/{chr}/associations`).
///
/// | Extra Param | Type | Description |
/// |---|---|---|
/// | `bp_lower` | u64 | Lower base-pair limit |
/// | `bp_upper` | u64 | Upper base-pair limit |
/// | `trait_` | String | Filter by trait ID |
#[derive(Debug, Default, Clone)]
pub struct ChromosomeAssociationQuery {
    pub start: Option<usize>,
    pub size: Option<usize>,
    pub reveal: Option<RevealMode>,
    pub p_lower: Option<f64>,
    pub p_upper: Option<f64>,
    pub bp_lower: Option<u64>,
    pub bp_upper: Option<u64>,
    pub study_accession: Option<String>,
    pub trait_: Option<String>,
}

/// Controls what data `reveal` returns.
/// - `Raw` — original/unharmonised values only
/// - `All` — both harmonised (default) and raw; raw fields get `hm_` prefix
#[derive(Debug, Clone, Copy)]
pub enum RevealMode {
    Raw,
    All,
}

impl std::fmt::Display for RevealMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RevealMode::Raw => write!(f, "raw"),
            RevealMode::All => write!(f, "all"),
        }
    }
}

/// Basic pagination query (`start` + `size`).
#[derive(Debug, Default, Clone)]
pub struct PaginationQuery {
    pub start: Option<usize>,
    pub size: Option<usize>,
}

// ── API client ────────────────────────────────────────────────────────────

/// Async client for the [GWAS Catalog Summary Statistics API].
///
/// [GWAS Catalog Summary Statistics API]: https://www.ebi.ac.uk/gwas/summary-statistics/docs/
///
/// # Example
///
/// ```ignore
/// let api = GwasCatalogApi::new();
/// let resp = api.list_associations(&AssociationQuery::default()).await?;
/// if let Some(emb) = resp._embedded {
///     for (_, assoc) in emb.associations {
///         println!("{} p={}", assoc.variant_id, assoc.p_value);
///     }
/// }
/// ```
pub struct GwasCatalogApi {
    client: reqwest::Client,
    base_url: String,
}

impl GwasCatalogApi {
    /// Create a client pointing to the production API.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: BASE_URL.to_string(),
        }
    }

    /// Create a client with a custom base URL (useful for testing or proxies).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
        }
    }

    // ── Associations ───────────────────────────────────────────────────

    /// `GET /associations`
    ///
    /// Lists all available associations. Supports p-value filtering via query params.
    pub async fn list_associations(
        &self,
        query: &AssociationQuery,
    ) -> Result<PaginatedResponse<EmbeddedAssociations>, GwasCatalogError> {
        self.get("/associations", query).await
    }

    /// `GET /associations/{variant_id}`
    ///
    /// Lists all associations for a variant (valid rsid). Returns 404 if not found.
    /// Add `study_accession` to the query to get a single association.
    pub async fn get_variant_associations(
        &self,
        variant_id: &str,
        query: &AssociationQuery,
    ) -> Result<PaginatedResponse<EmbeddedAssociations>, GwasCatalogError> {
        self.get(&format!("/associations/{variant_id}"), query)
            .await
    }

    // ── Chromosomes ────────────────────────────────────────────────────

    /// `GET /chromosomes`
    ///
    /// Lists all chromosome resources. X=23, Y=24, MT=25.
    pub async fn list_chromosomes(
        &self,
        query: &PaginationQuery,
    ) -> Result<PaginatedResponse<EmbeddedChromosomes>, GwasCatalogError> {
        self.get("/chromosomes", query).await
    }

    /// `GET /chromosomes/{chromosome}`
    ///
    /// Returns a specific chromosome resource. Returns 404 if invalid.
    pub async fn get_chromosome(&self, chromosome: &str) -> Result<Chromosome, GwasCatalogError> {
        self.get(&format!("/chromosomes/{chromosome}"), &()).await
    }

    /// `GET /chromosomes/{chromosome}/associations`
    ///
    /// Returns associations for a specific chromosome. Supports base-pair range
    /// filtering via `bp_lower`/`bp_upper`. Returns 404 if chromosome doesn't exist.
    pub async fn list_chromosome_associations(
        &self,
        chromosome: &str,
        query: &ChromosomeAssociationQuery,
    ) -> Result<PaginatedResponse<EmbeddedAssociations>, GwasCatalogError> {
        self.get(&format!("/chromosomes/{chromosome}/associations"), query)
            .await
    }

    /// `GET /chromosomes/{chromosome}/associations/{variant_id}`
    ///
    /// Faster than `get_variant_associations` if you know the chromosome.
    /// Returns 404 if variant not found.
    pub async fn get_variant_on_chromosome(
        &self,
        chromosome: &str,
        variant_id: &str,
        query: &AssociationQuery,
    ) -> Result<PaginatedResponse<EmbeddedAssociations>, GwasCatalogError> {
        self.get(
            &format!("/chromosomes/{chromosome}/associations/{variant_id}"),
            query,
        )
        .await
    }

    // ── Traits ──────────────────────────────────────────────────────────

    /// `GET /traits`
    ///
    /// Lists all existing EFO trait resources.
    pub async fn list_traits(
        &self,
        query: &PaginationQuery,
    ) -> Result<PaginatedResponse<EmbeddedTraits>, GwasCatalogError> {
        self.get("/traits", query).await
    }

    /// `GET /traits/{trait}`
    ///
    /// Returns a specific trait resource. Returns 404 if not found.
    pub async fn get_trait(&self, trait_id: &str) -> Result<Trait, GwasCatalogError> {
        self.get(&format!("/traits/{trait_id}"), &()).await
    }

    /// `GET /traits/{trait}/associations`
    ///
    /// Lists associations for a specific trait. Returns 404 if trait not found.
    pub async fn list_trait_associations(
        &self,
        trait_id: &str,
        query: &AssociationQuery,
    ) -> Result<PaginatedResponse<EmbeddedAssociations>, GwasCatalogError> {
        self.get(&format!("/traits/{trait_id}/associations"), query)
            .await
    }

    /// `GET /traits/{trait}/studies`
    ///
    /// Lists studies for a specific trait. Returns 404 if trait not found.
    pub async fn list_trait_studies(
        &self,
        trait_id: &str,
        query: &PaginationQuery,
    ) -> Result<PaginatedResponse<EmbeddedTraitStudies>, GwasCatalogError> {
        self.get(&format!("/traits/{trait_id}/studies"), query)
            .await
    }

    // ── Studies ─────────────────────────────────────────────────────────

    /// `GET /studies`
    ///
    /// Lists all existing study resources.
    pub async fn list_studies(
        &self,
        query: &PaginationQuery,
    ) -> Result<PaginatedResponse<EmbeddedStudies>, GwasCatalogError> {
        self.get("/studies", query).await
    }

    /// `GET /studies/{study_accession}`
    ///
    /// Returns a specific study resource. Returns 404 if not found.
    pub async fn get_study(&self, study_accession: &str) -> Result<Study, GwasCatalogError> {
        self.get(&format!("/studies/{study_accession}"), &()).await
    }

    /// `GET /studies/{study_accession}/associations`
    ///
    /// Returns associations for a specific study. Returns 404 if study not found.
    pub async fn list_study_associations(
        &self,
        study_accession: &str,
        query: &AssociationQuery,
    ) -> Result<PaginatedResponse<EmbeddedAssociations>, GwasCatalogError> {
        self.get(&format!("/studies/{study_accession}/associations"), query)
            .await
    }

    // ── Internal ────────────────────────────────────────────────────────

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &dyn QueryParams,
    ) -> Result<T, GwasCatalogError> {
        let mut url = format!("{}{path}", self.base_url);
        let pairs = query.query_pairs();
        if !pairs.is_empty() {
            url.push('?');
            url.push_str(
                &pairs
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("&"),
            );
        }
        let resp = self.client.get(&url).send().await?;
        let status = resp.status();
        if status.is_success() {
            resp.json().await.map_err(GwasCatalogError::Http)
        } else {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let message = body["message"]
                .as_str()
                .unwrap_or("unknown error")
                .to_string();
            Err(GwasCatalogError::Api {
                status: status.as_u16(),
                message,
            })
        }
    }
}

trait QueryParams {
    fn query_pairs(&self) -> Vec<(&'static str, String)>;
}

impl QueryParams for () {
    fn query_pairs(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }
}

impl QueryParams for PaginationQuery {
    fn query_pairs(&self) -> Vec<(&'static str, String)> {
        let mut p = Vec::new();
        if let Some(v) = self.start {
            p.push(("start", v.to_string()));
        }
        if let Some(v) = self.size {
            p.push(("size", v.to_string()));
        }
        p
    }
}

impl QueryParams for AssociationQuery {
    fn query_pairs(&self) -> Vec<(&'static str, String)> {
        let mut p = Vec::new();
        if let Some(v) = self.start {
            p.push(("start", v.to_string()));
        }
        if let Some(v) = self.size {
            p.push(("size", v.to_string()));
        }
        if let Some(r) = &self.reveal {
            p.push(("reveal", r.to_string()));
        }
        if let Some(v) = self.p_lower {
            p.push(("p_lower", v.to_string()));
        }
        if let Some(v) = self.p_upper {
            p.push(("p_upper", v.to_string()));
        }
        if let Some(v) = &self.study_accession {
            p.push(("study_accession", v.clone()));
        }
        p
    }
}

impl QueryParams for ChromosomeAssociationQuery {
    fn query_pairs(&self) -> Vec<(&'static str, String)> {
        let mut p = Vec::new();
        if let Some(v) = self.start {
            p.push(("start", v.to_string()));
        }
        if let Some(v) = self.size {
            p.push(("size", v.to_string()));
        }
        if let Some(r) = &self.reveal {
            p.push(("reveal", r.to_string()));
        }
        if let Some(v) = self.p_lower {
            p.push(("p_lower", v.to_string()));
        }
        if let Some(v) = self.p_upper {
            p.push(("p_upper", v.to_string()));
        }
        if let Some(v) = self.bp_lower {
            p.push(("bp_lower", v.to_string()));
        }
        if let Some(v) = self.bp_upper {
            p.push(("bp_upper", v.to_string()));
        }
        if let Some(v) = &self.study_accession {
            p.push(("study_accession", v.clone()));
        }
        if let Some(v) = &self.trait_ {
            p.push(("trait", v.clone()));
        }
        p
    }
}

impl Default for GwasCatalogApi {
    fn default() -> Self {
        Self::new()
    }
}
