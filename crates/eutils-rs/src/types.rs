use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// Parameters for [`EutilsClient::esearch`](crate::EutilsClient::esearch).
#[derive(Debug, Clone, Default, Serialize)]
pub struct ESearchRequest {
    /// Entrez database to search (e.g. `"pubmed"`, `"gene"`).
    pub db: String,
    /// Entrez text query (e.g. `"cancer immunotherapy[Title/Abstract]"`).
    pub term: String,
    /// Maximum number of UIDs to return (default 20, max 10 000).
    pub retmax: Option<u32>,
    /// Sequential start index for pagination.
    pub retstart: Option<u32>,
    /// Sort order. Common PubMed values: `"pub_date"`, `"Author"`, `"relevance"`.
    pub sort: Option<String>,
    /// Save results to the Entrez History server.
    pub usehistory: Option<bool>,
    /// Web environment for chaining E-utility calls.
    pub web_env: Option<String>,
    /// Query key for chaining E-utility calls.
    pub query_key: Option<String>,
    /// Date type filter (`"pdat"`, `"mdat"`, `"edat"`).
    pub datetype: Option<String>,
    /// Relative date filter (results within N days).
    pub reldate: Option<u32>,
    /// Minimum date for range filter (e.g. `"2020/01/01"`).
    pub mindate: Option<String>,
    /// Maximum date for range filter.
    pub maxdate: Option<String>,
}

impl ESearchRequest {
    /// Convenience constructor for a simple search.
    pub fn new(db: &str, term: &str) -> Self {
        Self {
            db: db.to_owned(),
            term: term.to_owned(),
            retmax: None,
            retstart: None,
            sort: None,
            usehistory: None,
            web_env: None,
            query_key: None,
            datetype: None,
            reldate: None,
            mindate: None,
            maxdate: None,
        }
    }
}

/// Parameters for [`EutilsClient::esummary`](crate::EutilsClient::esummary).
#[derive(Debug, Clone, Default, Serialize)]
pub struct ESummaryRequest {
    /// Entrez database.
    pub db: String,
    /// Comma-separated UIDs or a single UID.
    pub id: String,
    /// Maximum number of records to return.
    pub retmax: Option<u32>,
    /// Sequential start index.
    pub retstart: Option<u32>,
    /// Use version 2.0 (returns richer, database-specific DocSums).
    pub version: Option<String>,
}

impl ESummaryRequest {
    pub fn new(db: &str, id: &str) -> Self {
        Self {
            db: db.to_owned(),
            id: id.to_owned(),
            retmax: None,
            retstart: None,
            version: None,
        }
    }
}

/// Parameters for [`EutilsClient::efetch`](crate::EutilsClient::efetch).
#[derive(Debug, Clone, Default, Serialize)]
pub struct EFetchRequest {
    /// Entrez database.
    pub db: String,
    /// Comma-separated UIDs or a single UID.
    pub id: String,
    /// Retrieval type (e.g. `"abstract"`, `"medline"`, `"full"` for PubMed).
    pub rettype: Option<String>,
    /// Retrieval mode (`"text"` or `"xml"`).
    pub retmode: Option<String>,
    /// Maximum number of records to return.
    pub retmax: Option<u32>,
    /// Sequential start index.
    pub retstart: Option<u32>,
    /// Web environment for History server.
    pub web_env: Option<String>,
    /// Query key for History server.
    pub query_key: Option<String>,
}

impl EFetchRequest {
    pub fn new(db: &str, id: &str) -> Self {
        Self {
            db: db.to_owned(),
            id: id.to_owned(),
            rettype: None,
            retmode: None,
            retmax: None,
            retstart: None,
            web_env: None,
            query_key: None,
        }
    }
}

/// Parameters for [`EutilsClient::elink`](crate::EutilsClient::elink).
#[derive(Debug, Clone, Default, Serialize)]
pub struct ELinkRequest {
    /// Source database.
    pub dbfrom: String,
    /// Destination database (default `"pubmed"`).
    pub db: Option<String>,
    /// Comma-separated UIDs.
    pub id: String,
    /// Command mode (`"neighbor"`, `"neighbor_score"`, `"neighbor_history"`, etc.).
    pub cmd: Option<String>,
    /// Specific link name to retrieve.
    pub linkname: Option<String>,
}

impl ELinkRequest {
    pub fn new(dbfrom: &str, id: &str) -> Self {
        Self {
            dbfrom: dbfrom.to_owned(),
            id: id.to_owned(),
            db: None,
            cmd: None,
            linkname: None,
        }
    }
}

/// Parameters for [`EutilsClient::ecitmatch`](crate::EutilsClient::ecitmatch).
#[derive(Debug, Clone, Serialize)]
pub struct ECitMatchRequest {
    /// Citation strings in the format `"journal|volume|page|year|authors|title"`.
    pub bdata: Vec<String>,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Parsed response from [`EutilsClient::einfo`](crate::EutilsClient::einfo).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EInfoResponse {
    #[serde(rename = "einforesult")]
    pub result: EInfoResult,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EInfoResult {
    /// When querying all databases, the list of database names.
    pub dblist: Option<Vec<String>>,
    /// When querying a single database, an array of database info objects.
    pub dbinfo: Option<Vec<EInfoDbInfo>>,
}

/// Per-database information returned by EInfo.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EInfoDbInfo {
    /// Database name (e.g. `"pubmed"`, `"gene"`).
    pub dbname: String,
    /// Human-readable menu name.
    pub menuname: Option<String>,
    /// Database description.
    pub description: Option<String>,
    /// Number of records in the database.
    pub count: Option<String>,
    /// Last update date (e.g. `"2024/01/15 10:30"`).
    pub lastupdate: Option<String>,
    /// Database build identifier.
    pub dbbuild: Option<String>,
    /// List of searchable fields.
    pub fieldlist: Option<Vec<serde_json::Value>>,
    /// List of links to other databases.
    pub linklist: Option<Vec<serde_json::Value>>,
}

/// Parsed response from [`EutilsClient::esearch`](crate::EutilsClient::esearch).
#[derive(Debug, Clone, Deserialize)]
pub struct ESearchResponse {
    #[serde(rename = "esearchresult")]
    pub result: ESearchResult,
}

/// Note: NCBI's ESearch JSON uses all-lowercase concatenated keys
/// (`idlist`, `querykey`, `webenv`, …), not camelCase. Field names are
/// therefore mapped explicitly below.
#[derive(Debug, Clone, Deserialize)]
pub struct ESearchResult {
    /// Total count of matching records.
    pub count: String,
    /// Number of IDs returned in this response.
    pub retmax: String,
    /// Start index of returned IDs.
    pub retstart: String,
    /// List of matching UIDs (PMIDs for PubMed).
    #[serde(rename = "idlist")]
    pub id_list: Vec<String>,
    /// Query key for History server chaining.
    #[serde(rename = "querykey")]
    pub query_key: Option<String>,
    /// Web environment string for History server chaining.
    #[serde(rename = "webenv")]
    pub web_env: Option<String>,
    /// Query translation stack.
    #[serde(rename = "translationstack")]
    pub translation_stack: Option<serde_json::Value>,
    /// Translated query string.
    #[serde(rename = "querytranslation")]
    pub query_translation: Option<String>,
}

/// Parsed response from [`EutilsClient::egquery`](crate::EutilsClient::egquery).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EGQueryResponse {
    #[serde(rename = "result")]
    pub result: Vec<serde_json::Value>,
}
