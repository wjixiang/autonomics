//! Render raw E-utility JSON / text responses into clean, LLM-friendly Markdown.

use serde_json::Value;

// ---------------------------------------------------------------------------
// ESummary
// ---------------------------------------------------------------------------

/// Format an ESummary v2.0 JSON response into readable Markdown.
///
/// The raw response has the shape:
/// ```json
/// {
///   "header": { "type": "esummary", "version": "0.3" },
///   "result": {
///     "uids": ["123", "456"],
///     "123": { "title", "authors"[], "journal"{}, "pubdate", "doi", ... },
///     "456": { ... }
///   }
/// }
/// ```
pub fn format_esummary(data: &Value) -> String {
    let result = match data.get("result") {
        Some(v) => v,
        None => return "No result data returned.".to_string(),
    };

    let uids: Vec<&str> = result
        .get("uids")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    if uids.is_empty() {
        return "No records found.".to_string();
    }

    let mut out = String::with_capacity(4096);
    out.push_str(&format!("**{} articles**\n\n", uids.len()));

    for pmid in &uids {
        let article = match result.get(*pmid) {
            Some(v) => v,
            None => continue,
        };

        let title = str_field(article, "title");
        out.push_str(&format!("### [PMID {pmid}] {title}\n"));

        // Authors
        if let Some(authors) = article.get("authors").and_then(|v| v.as_array()) {
            let names: Vec<&str> = authors
                .iter()
                .filter_map(|a| a.get("name").and_then(|v| v.as_str()))
                .collect();
            if !names.is_empty() {
                out.push_str(&format!("**Authors:** {}\n", names.join(", ")));
            }
        }

        // Journal
        if let Some(journal) = article.get("journal") {
            let jname = str_field(journal, "title");
            let vol = str_field(journal, "volume");
            let iss = str_field(journal, "issue");
            if !jname.is_empty() {
                let mut jline = format!("**Journal:** {jname}");
                let mut parts: Vec<String> = Vec::new();
                if !vol.is_empty() {
                    parts.push(vol.to_string());
                }
                if !iss.is_empty() {
                    parts.push(format!("({iss})"));
                }
                if !parts.is_empty() {
                    jline.push_str(&format!(", {}", parts.join(" ")));
                }
                out.push_str(&format!("{jline}\n"));
            }
        }

        // Publication date
        let pubdate = str_field(article, "pubdate");
        let epubdate = str_field(article, "epubdate");
        if !pubdate.is_empty() {
            out.push_str(&format!("**Published:** {pubdate}"));
            if !epubdate.is_empty() && epubdate != pubdate {
                out.push_str(&format!(" (Epub: {epubdate})"));
            }
            out.push('\n');
        }

        // DOI — prefer elocationid, fall back to articleids array
        let doi_from_location = str_field(article, "elocationid")
            .replace("doi: ", "")
            .trim()
            .to_string();
        let doi_printed = if !doi_from_location.is_empty() && !doi_from_location.starts_with("doi:")
        {
            out.push_str(&format!("**DOI:** {doi_from_location}\n"));
            true
        } else {
            false
        };
        if !doi_printed {
            if let Some(ids) = article.get("articleids").and_then(|v| v.as_array()) {
                for id_entry in ids {
                    let id_type = str_field(id_entry, "idtype");
                    if id_type == "doi" {
                        out.push_str(&format!("**DOI:** {}\n", str_field(id_entry, "value")));
                        break;
                    }
                }
            }
        }

        // Abstract (may be HTML, strip tags roughly)
        if let Some(abstract_val) = article.get("abstract") {
            if let Some(abs_text) = abstract_val.as_str() {
                let cleaned = strip_html_tags(abs_text);
                if !cleaned.is_empty() {
                    out.push_str("\n");
                    out.push_str(&cleaned);
                    out.push_str("\n\n");
                }
            }
        }

        out.push('\n');
    }

    out.trim_end().to_string()
}

// ---------------------------------------------------------------------------
// ESearch
// ---------------------------------------------------------------------------

/// Format an ESearch result (already shaped by the tool into `{ count, id_list, ... }`)
/// into a compact summary.
pub fn format_esearch(data: &Value) -> String {
    let count = str_field(data, "count");
    let ids: Vec<&str> = data
        .get("id_list")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let mut out = String::with_capacity(512);
    out.push_str(&format!("**{count}** results found\n\n"));

    if ids.is_empty() {
        out.push_str("No PMIDs returned.\n");
    } else {
        out.push_str("**PMIDs:** ");
        // Show up to 50 IDs in a compact comma-separated list.
        let display: Vec<&str> = ids.iter().take(50).copied().collect();
        out.push_str(&display.join(", "));
        if ids.len() > 50 {
            out.push_str(&format!(" ... ({} total)", ids.len()));
        }
        out.push('\n');

        if let Some(qt) = data.get("query_translation").and_then(|v| v.as_str()) {
            if !qt.is_empty() {
                out.push_str(&format!("**Query:** {qt}\n"));
            }
        }
    }

    out
}

// ---------------------------------------------------------------------------
// EFetch
// ---------------------------------------------------------------------------

/// Format an EFetch result (shaped as `{ format, content }`) into Markdown.
pub fn format_efetch(data: &Value) -> String {
    let format = str_field(data, "format");
    let content = str_field(data, "content");

    let mut out = String::new();
    out.push_str(&format!("**Format:** {format}\n\n"));
    // The raw text content from EFetch is already reasonably structured.
    // Just present it as a code block for readability.
    out.push_str("```\n");
    out.push_str(&content);
    out.push_str("\n```\n");
    out
}

// ---------------------------------------------------------------------------
// ELink (related articles)
// ---------------------------------------------------------------------------

/// Format an ELink neighbor response into a compact list of related PMIDs.
pub fn format_elink(data: &Value) -> String {
    let mut out = String::with_capacity(1024);

    // ELink responses can have nested structures.
    // Look for linksets → linksetdbs → links
    let linksets = data
        .get("linksets")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);

    if linksets.is_empty() {
        // Try flattened structure
        let ids = data
            .get("ids")
            .and_then(|v| v.as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[]);
        let scores = data
            .get("scores")
            .and_then(|v| v.as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[]);

        if !ids.is_empty() {
            out.push_str(&format!("**{} related articles**\n\n", ids.len()));
            for (i, id_val) in ids.iter().take(30).enumerate() {
                let id = match id_val.as_str() {
                    Some(s) => s,
                    None => continue,
                };
                let score_line = if i < scores.len() {
                    match scores[i].as_f64() {
                        Some(s) => format!(" (score: {s:.2})"),
                        None => String::new(),
                    }
                } else {
                    String::new()
                };
                out.push_str(&format!("- PMID {id}{score_line}\n"));
            }
            if ids.len() > 30 {
                out.push_str(&format!("... and {} more\n", ids.len() - 30));
            }
        } else {
            out.push_str("No related articles found.\n");
        }
    } else {
        for ls in linksets {
            let dbs = ls
                .get("linksetdbs")
                .and_then(|v| v.as_array())
                .map(|a| a.as_slice())
                .unwrap_or(&[]);
            for db in dbs {
                let db_name = str_field(db, "dbto");
                let links = db
                    .get("links")
                    .and_then(|v| v.as_array())
                    .map(|a| a.as_slice())
                    .unwrap_or(&[]);
                out.push_str(&format!(
                    "**Related (→ {db_name}):** {} articles\n\n",
                    links.len()
                ));
                for link in links.iter().take(30) {
                    let id = match link.as_str() {
                        Some(s) => s,
                        None => continue,
                    };
                    out.push_str(&format!("- PMID {id}\n"));
                }
                if links.len() > 30 {
                    out.push_str(&format!("... and {} more\n", links.len() - 30));
                }
                out.push('\n');
            }
        }
    }

    if out.is_empty() {
        "No related articles found.".to_string()
    } else {
        out.trim_end().to_string()
    }
}

// ---------------------------------------------------------------------------
// ESpell
// ---------------------------------------------------------------------------

/// Format an ESpell response into a compact suggestion block.
pub fn format_espell(data: &Value) -> String {
    let mut out = String::with_capacity(256);

    let corrected = data.get("CorrectedQuery").and_then(|v| v.as_str());
    let original = data.get("OriginalQuery").and_then(|v| v.as_str());

    if let Some(corr) = corrected {
        out.push_str("**Corrected spelling:**\n");
        out.push_str(corr);
        out.push('\n');
        if let Some(orig) = original {
            out.push_str(&format!("(original: \"{orig}\")\n"));
        }
    } else {
        let db = str_field(data, "Database");
        let query = str_field(data, "Query");
        out.push_str(&format!(
            "No spelling correction needed for \"{query}\" in {db}.\n"
        ));
    }

    out
}

// ---------------------------------------------------------------------------
// EInfo
// ---------------------------------------------------------------------------

/// Format an EInfo response into a compact database listing or field table.
pub fn format_einfo(data: &Value) -> String {
    let mut out = String::with_capacity(1024);

    // Try to detect if this is the full database list or a single-db info
    if let Some(dblist) = data
        .get("einforesult")
        .and_then(|r| r.get("dblist"))
        .and_then(|v| v.as_array())
    {
        out.push_str(&format!(
            "**{} Entrez databases available:**\n\n",
            dblist.len()
        ));
        for db in dblist.iter().take(60) {
            if let Some(name) = db.as_str() {
                out.push_str(&format!("- {name}\n"));
            }
        }
        if dblist.len() > 60 {
            out.push_str(&format!("... and {} more\n", dblist.len() - 60));
        }
        return out.trim_end().to_string();
    }

    // Single database info
    let dbinfos = data
        .get("einforesult")
        .and_then(|r| r.get("dbinfo"))
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);

    for info in dbinfos {
        let name = str_field(info, "dbname");
        let menu = str_field(info, "menuname");
        let desc = str_field(info, "description");
        let count = str_field(info, "count");
        let update = str_field(info, "lastupdate");

        out.push_str(&format!("## {name}\n"));
        if !menu.is_empty() {
            out.push_str(&format!("**Name:** {menu}\n"));
        }
        if !count.is_empty() {
            out.push_str(&format!("**Records:** {count}\n"));
        }
        if !update.is_empty() {
            out.push_str(&format!("**Last updated:** {update}\n"));
        }
        if !desc.is_empty() {
            let cleaned = strip_html_tags(&desc);
            out.push_str(&format!("**Description:** {cleaned}\n"));
        }
        out.push('\n');

        // Fields
        if let Some(fields) = info.get("fieldlist").and_then(|v| v.as_array()) {
            if !fields.is_empty() {
                out.push_str("**Searchable fields:**\n");
                out.push_str("| Field | Name | Description |\n");
                out.push_str("|-------|------|-------------|\n");
                for field in fields.iter().take(30) {
                    let fname = str_field(field, "name");
                    let ffull = str_field(field, "fullname");
                    let fdesc = truncate_str(&str_field(field, "description"), 60);
                    out.push_str(&format!("| {fname} | {ffull} | {fdesc} |\n"));
                }
                out.push('\n');
            }
        }

        // Links
        if let Some(links) = info.get("linklist").and_then(|v| v.as_array()) {
            if !links.is_empty() {
                out.push_str("**Links to other databases:**\n");
                for link in links.iter().take(20) {
                    let lname = str_field(link, "name");
                    let lmenu = str_field(link, "menu");
                    out.push_str(&format!("- **{lname}** → {lmenu}\n"));
                }
                out.push('\n');
            }
        }
    }

    if out.is_empty() {
        "No database information returned.".to_string()
    } else {
        out.trim_end().to_string()
    }
}

// ---------------------------------------------------------------------------
// EGQuery
// ---------------------------------------------------------------------------

/// Format an EGQuery response into a compact per-database count table.
pub fn format_egquery(data: &Value) -> String {
    let results = data
        .get("result")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);

    if results.is_empty() {
        return "No results returned.".to_string();
    }

    let mut out = String::with_capacity(512);
    out.push_str("**Cross-database query results:**\n\n");

    // Collect entries with non-zero counts, sorted by count descending
    let mut entries: Vec<(&str, u64)> = results
        .iter()
        .filter_map(|item| {
            let name = item.get("dbname")?.as_str()?;
            let count = item.get("count")?.as_str()?.parse::<u64>().ok()?;
            Some((name, count))
        })
        .filter(|(_, c)| *c > 0)
        .collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1));

    out.push_str("| Database | Count |\n");
    out.push_str("|----------|-------|\n");
    for (name, count) in entries.iter().take(25) {
        out.push_str(&format!("| {name} | {count} |\n"));
    }
    if entries.len() > 25 {
        out.push_str(&format!(
            "| ... | {} more databases |\n",
            entries.len() - 25
        ));
    }

    out.trim_end().to_string()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn str_field<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// Truncate a string to `max_len` characters at a word boundary.
fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        let trimmed = &s[..end];
        if let Some(space) = trimmed.rfind(' ') {
            &s[..space]
        } else {
            trimmed
        }
    }
}

/// Roughly strip common HTML tags (<b>, <i>, <a>, etc.) from a string.
fn strip_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn format_esummary_single() {
        let data = json!({
            "header": { "type": "esummary", "version": "0.3" },
            "result": {
                "uids": ["12345"],
                "12345": {
                    "title": "A Great Paper",
                    "authors": [
                        { "name": "Smith A", "authtype": "Author" },
                        { "name": "Jones B", "authtype": "Author" }
                    ],
                    "journal": { "title": "Nature", "volume": "1", "issue": "2" },
                    "pubdate": "2024 Jan 15",
                    "epubdate": "2023 Dec 20",
                    "elocationid": "doi: 10.1234/test",
                    "abstract": "This is the abstract text."
                }
            }
        });

        let rendered = format_esummary(&data);
        assert!(rendered.contains("**1 articles**"));
        assert!(rendered.contains("[PMID 12345] A Great Paper"));
        assert!(rendered.contains("Smith A, Jones B"));
        assert!(rendered.contains("Nature, 1 (2)"));
        assert!(rendered.contains("Published:** 2024 Jan 15"));
        assert!(rendered.contains("This is the abstract text."));
    }

    #[test]
    fn format_esummary_empty() {
        let data = json!({ "header": {}, "result": {} });
        let rendered = format_esummary(&data);
        assert!(rendered.contains("No records"));
    }

    #[test]
    fn format_esearch_basic() {
        let data = json!({
            "count": "42",
            "id_list": ["111", "222", "333"]
        });
        let rendered = format_esearch(&data);
        assert!(rendered.contains("**42** results"));
        assert!(rendered.contains("111, 222, 333"));
    }

    #[test]
    fn format_espell_correction() {
        let data = json!({
            "OriginalQuery": "cancr immuntherapy",
            "CorrectedQuery": "cancer immunotherapy",
            "Database": "pubmed"
        });
        let rendered = format_espell(&data);
        assert!(rendered.contains("Corrected spelling:"));
        assert!(rendered.contains("cancer immunotherapy"));
        assert!(rendered.contains("original:"));
    }

    #[test]
    fn format_espell_no_correction() {
        let data = json!({
            "Query": "cancer",
            "Database": "pubmed"
        });
        let rendered = format_espell(&data);
        assert!(rendered.contains("No spelling correction"));
    }

    #[test]
    fn format_egquery_basic() {
        let data = json!({
            "result": [
                { "dbname": "pubmed", "count": "1500" },
                { "dbname": "gene", "count": "200" },
                { "dbname": "pmc", "count": "0" }
            ]
        });
        let rendered = format_egquery(&data);
        assert!(rendered.contains("Cross-database"));
        assert!(rendered.contains("pubmed"));
        // Zero-count entries are filtered out
        assert!(!rendered.contains("pmc"));
    }

    #[test]
    fn format_efetch_basic() {
        let data = json!({
            "format": "abstract",
            "content": "Title: A paper\nAuthors: Smith\nAbstract: blah"
        });
        let rendered = format_efetch(&data);
        assert!(rendered.contains("**Format:** abstract"));
        assert!(rendered.contains("Title: A paper"));
    }
}
