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
    #[serde(default, rename = "trait", deserialize_with = "single_or_vec::deserialize")]
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

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<Option<Vec<HalLink>>, D::Error>
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

#[derive(Debug, Deserialize)]
pub struct Chromosome {
    pub chromosome: String,
    #[serde(default)]
    pub _links: HalLinks,
}

#[derive(Debug, Deserialize)]
pub struct Trait {
    #[serde(rename = "trait")]
    pub trait_: String,
    #[serde(default)]
    pub _links: HalLinks,
}

#[derive(Debug, Deserialize)]
pub struct Study {
    pub study_accession: String,
    #[serde(default)]
    pub _links: HalLinks,
}

#[derive(Debug, Default, Deserialize)]
pub struct EmbeddedAssociations {
    #[serde(default)]
    pub associations: std::collections::HashMap<String, Association>,
}

#[derive(Debug, Default, Deserialize)]
pub struct EmbeddedChromosomes {
    #[serde(default)]
    pub chromosomes: Vec<Chromosome>,
}

#[derive(Debug, Default, Deserialize)]
pub struct EmbeddedTraits {
    #[serde(default, rename = "trait")]
    pub traits: Vec<Trait>,
}

#[derive(Debug, Default, Deserialize)]
pub struct EmbeddedStudies {
    #[serde(default)]
    pub studies: Vec<Vec<Study>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct EmbeddedTraitStudies {
    #[serde(default)]
    pub studies: Vec<Study>,
}

#[derive(Debug, Deserialize)]
#[serde(bound = "")]
pub struct PaginatedResponse<T: serde::de::DeserializeOwned> {
    #[serde(default, bound = "")]
    pub _embedded: Option<T>,
    #[serde(default)]
    pub _links: HalLinks,
}

// ── Query builders ────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct AssociationQuery {
    pub start: Option<usize>,
    pub size: Option<usize>,
    pub reveal: Option<RevealMode>,
    pub p_lower: Option<f64>,
    pub p_upper: Option<f64>,
    pub study_accession: Option<String>,
}

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

#[derive(Debug, Default, Clone)]
pub struct PaginationQuery {
    pub start: Option<usize>,
    pub size: Option<usize>,
}

// ── API client ────────────────────────────────────────────────────────────

pub struct GwasCatalogApi {
    client: reqwest::Client,
    base_url: String,
}

impl GwasCatalogApi {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: BASE_URL.to_string(),
        }
    }

    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
        }
    }

    // ── Associations ───────────────────────────────────────────────────

    pub async fn list_associations(
        &self,
        query: &AssociationQuery,
    ) -> Result<PaginatedResponse<EmbeddedAssociations>, GwasCatalogError> {
        self.get("/associations", query).await
    }

    pub async fn get_variant_associations(
        &self,
        variant_id: &str,
        query: &AssociationQuery,
    ) -> Result<PaginatedResponse<EmbeddedAssociations>, GwasCatalogError> {
        self.get(&format!("/associations/{variant_id}"), query)
            .await
    }

    // ── Chromosomes ────────────────────────────────────────────────────

    pub async fn list_chromosomes(
        &self,
        query: &PaginationQuery,
    ) -> Result<PaginatedResponse<EmbeddedChromosomes>, GwasCatalogError> {
        self.get("/chromosomes", query).await
    }

    pub async fn get_chromosome(&self, chromosome: &str) -> Result<Chromosome, GwasCatalogError> {
        self.get(&format!("/chromosomes/{chromosome}"), &()).await
    }

    pub async fn list_chromosome_associations(
        &self,
        chromosome: &str,
        query: &ChromosomeAssociationQuery,
    ) -> Result<PaginatedResponse<EmbeddedAssociations>, GwasCatalogError> {
        self.get(&format!("/chromosomes/{chromosome}/associations"), query)
            .await
    }

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

    pub async fn list_traits(
        &self,
        query: &PaginationQuery,
    ) -> Result<PaginatedResponse<EmbeddedTraits>, GwasCatalogError> {
        self.get("/traits", query).await
    }

    pub async fn get_trait(&self, trait_id: &str) -> Result<Trait, GwasCatalogError> {
        self.get(&format!("/traits/{trait_id}"), &()).await
    }

    pub async fn list_trait_associations(
        &self,
        trait_id: &str,
        query: &AssociationQuery,
    ) -> Result<PaginatedResponse<EmbeddedAssociations>, GwasCatalogError> {
        self.get(&format!("/traits/{trait_id}/associations"), query)
            .await
    }

    pub async fn list_trait_studies(
        &self,
        trait_id: &str,
        query: &PaginationQuery,
    ) -> Result<PaginatedResponse<EmbeddedTraitStudies>, GwasCatalogError> {
        self.get(&format!("/traits/{trait_id}/studies"), query)
            .await
    }

    // ── Studies ─────────────────────────────────────────────────────────

    pub async fn list_studies(
        &self,
        query: &PaginationQuery,
    ) -> Result<PaginatedResponse<EmbeddedStudies>, GwasCatalogError> {
        self.get("/studies", query).await
    }

    pub async fn get_study(&self, study_accession: &str) -> Result<Study, GwasCatalogError> {
        self.get(&format!("/studies/{study_accession}"), &()).await
    }

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
