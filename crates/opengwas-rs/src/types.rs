use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Shared enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Population {
    EAS,
    SAS,
    EUR,
    AFR,
    AMR,
    #[serde(untagged)]
    Other(String),
}

impl Default for Population {
    fn default() -> Self {
        Self::EUR
    }
}

impl std::fmt::Display for Population {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Other(s) => s.fmt(f),
            _ => write!(f, "{:?}", self),
        }
    }
}

// ---------------------------------------------------------------------------
// GwasInfo – the canonical metadata model returned by many endpoints
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GwasInfo {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default, rename(serialize = "trait", deserialize = "trait"))]
    pub trait_: Option<String>,
    #[serde(default)]
    pub build: Option<String>,
    #[serde(default)]
    pub group_name: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub subcategory: Option<String>,
    #[serde(default)]
    pub population: Option<String>,
    #[serde(default)]
    pub sex: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub nsnp: Option<i64>,
    #[serde(default)]
    pub sample_size: Option<i64>,
    #[serde(default)]
    pub year: Option<i64>,
    #[serde(default)]
    pub ontology: Option<String>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub ncase: Option<i64>,
    #[serde(default)]
    pub ncontrol: Option<i64>,
    #[serde(default)]
    pub study_design: Option<String>,
    #[serde(default)]
    pub covariates: Option<String>,
    #[serde(default)]
    pub coverage: Option<String>,
    #[serde(default)]
    pub qc_prior_to_upload: Option<String>,
    #[serde(default)]
    pub imputation_panel: Option<String>,
    #[serde(default)]
    pub beta_transformation: Option<String>,
    #[serde(default)]
    pub doi: Option<String>,
    #[serde(default)]
    pub consortium: Option<String>,
    #[serde(default)]
    pub pmid: Option<i64>,
    #[serde(default)]
    pub sd: Option<f64>,
    #[serde(default)]
    pub mr: Option<i64>,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub is_nc: Option<i64>,
}

// ---------------------------------------------------------------------------
// /associations  –  POST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct AssociationsRequest {
    pub variant: Vec<String>,
    pub id: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxies: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub population: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r2: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align_alleles: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palindromes: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maf_threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commercial_approval_received: Option<i32>,
}

// ---------------------------------------------------------------------------
// /tophits  –  POST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct TophitsRequest {
    pub id: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pval: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preclumped: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clump: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r2: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kb: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pop: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commercial_approval_received: Option<i32>,
}

// ---------------------------------------------------------------------------
// /phewas  –  POST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct PhewasRequest {
    pub variant: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pval: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_list: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commercial_approval_received: Option<i32>,
}

// ---------------------------------------------------------------------------
// /ld/clump  –  POST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct LdClumpRequest {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rsid: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pval: Vec<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pthresh: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r2: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kb: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pop: Option<String>,
}

// ---------------------------------------------------------------------------
// /ld/matrix  –  POST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct LdMatrixRequest {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rsid: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pop: Option<String>,
}

// ---------------------------------------------------------------------------
// /ld/reflookup  –  POST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct LdReflookupRequest {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rsid: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pop: Option<String>,
}

// ---------------------------------------------------------------------------
// /variants/rsid  –  POST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct VariantsRsidRequest {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rsid: Vec<String>,
}

// ---------------------------------------------------------------------------
// /variants/chrpos  –  POST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct VariantsChrposRequest {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chrpos: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<i32>,
}

// ---------------------------------------------------------------------------
// /variants/afl2  –  POST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct VariantsAfl2Request {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rsid: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chrpos: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<i32>,
}

// ---------------------------------------------------------------------------
// /gwasinfo  –  POST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct GwasInfoRequest {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub id: Vec<String>,
}

// ---------------------------------------------------------------------------
// /gwasinfo/files  –  POST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct GwasInfoFilesRequest {
    pub id: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commercial_approval_received: Option<i32>,
}

// ---------------------------------------------------------------------------
// /edit/add  –  POST   (all required + optional fields)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct EditAddRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub trait_: String,
    pub build: String,
    pub group_name: String,
    pub category: String,
    pub subcategory: String,
    pub population: String,
    pub sex: String,
    pub author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nsnp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ncase: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ncontrol: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub study_design: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub covariates: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qc_prior_to_upload: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imputation_panel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub beta_transformation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doi: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consortium: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pmid: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mr: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_nc: Option<i32>,
}

// ---------------------------------------------------------------------------
// /edit/edit  –  POST  (same fields as add but id is required)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct EditEditRequest {
    pub id: String,
    pub trait_: String,
    pub build: String,
    pub group_name: String,
    pub category: String,
    pub subcategory: String,
    pub population: String,
    pub sex: String,
    pub author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nsnp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ncase: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ncontrol: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub study_design: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub covariates: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qc_prior_to_upload: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imputation_panel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub beta_transformation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doi: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consortium: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pmid: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mr: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_nc: Option<i32>,
}

// ---------------------------------------------------------------------------
// /edit/list  –  GET
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct EditListQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,
}

// ---------------------------------------------------------------------------
// /quality_control/release  –  POST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct QcReleaseRequest {
    pub id: String,
    pub passed_qc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comments: Option<String>,
}
