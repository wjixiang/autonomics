//! PubMed literature search tools for the agentik-core runtime.
//!
//! This crate exposes two tools backed by a Node.js subprocess bridge that
//! calls [`@wjx/bibliography-search`](https://github.com/wjx/bibliography-search):
//!
//! - **`pubmed_search`** — search PubMed by keyword, returning article profiles.
//! - **`pubmed_article_detail`** — fetch the full record of a single article by PMID.
//!
//! Wire the tools into an agent's toolset via [`bibliography_registrations`].

pub mod common;
pub mod detail;
pub mod search;

pub use agentik_core::tools::ToolRegistration;
pub use detail::{ArticleDetailInput, ArticleDetailTool};
pub use search::{PubmedSearchInput, PubmedSearchTool};

/// All bibliography tool registrations, ready to register into a toolset.
pub fn bibliography_registrations() -> Vec<ToolRegistration> {
    vec![
        ToolRegistration::from(PubmedSearchTool),
        ToolRegistration::from(ArticleDetailTool),
    ]
}
