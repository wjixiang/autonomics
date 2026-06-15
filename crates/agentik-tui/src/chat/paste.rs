//! Paste handling for the chat input box.

/// Lower bound (in lines) above which a paste is collapsed.
pub const PASTE_SUMMARY_LINE_THRESHOLD: usize = 3;

/// Lower bound (in characters) above which a single-line paste is collapsed.
pub const PASTE_SUMMARY_LEN_THRESHOLD: usize = 150;

/// A paste event recorded in the chat panel's input buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteEntry {
    pub placeholder: String,
    pub content: String,
}

impl PasteEntry {
    pub fn from_content(content: &str) -> Self {
        if let Some(placeholder) = summarize_paste(content) {
            Self {
                placeholder,
                content: content.to_string(),
            }
        } else {
            Self {
                placeholder: content.to_string(),
                content: content.to_string(),
            }
        }
    }
}

/// Decide whether a paste should be collapsed into a compact placeholder.
pub fn needs_placeholder(content: &str) -> bool {
    let line_count = content.lines().count().max(1);
    line_count >= PASTE_SUMMARY_LINE_THRESHOLD
        || content.chars().count() > PASTE_SUMMARY_LEN_THRESHOLD
}

/// Build a `[Pasted ~N lines]` placeholder for the given content.
pub fn summarize_paste(content: &str) -> Option<String> {
    if needs_placeholder(content) {
        let line_count = content.lines().count().max(1);
        Some(format!("[Pasted ~{line_count} lines]"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        needs_placeholder, summarize_paste, PasteEntry, PASTE_SUMMARY_LEN_THRESHOLD,
        PASTE_SUMMARY_LINE_THRESHOLD,
    };

    #[test]
    fn short_single_line_paste_is_verbatim() {
        assert!(summarize_paste("hello world").is_none());
        assert!(!needs_placeholder("hello world"));
    }

    #[test]
    fn exactly_two_lines_is_verbatim() {
        assert!(summarize_paste("line one\nline two").is_none());
        assert!(!needs_placeholder("line one\nline two"));
    }

    #[test]
    fn three_lines_triggers_placeholder() {
        let p = summarize_paste("a\nb\nc").unwrap();
        assert_eq!(p, "[Pasted ~3 lines]");
        assert!(needs_placeholder("a\nb\nc"));
    }

    #[test]
    fn many_lines_triggers_placeholder_with_correct_count() {
        let text = (1..=10)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let p = summarize_paste(&text).unwrap();
        assert_eq!(p, "[Pasted ~10 lines]");
    }

    #[test]
    fn single_very_long_line_triggers_placeholder() {
        let long = "a".repeat(PASTE_SUMMARY_LEN_THRESHOLD + 1);
        let p = summarize_paste(&long).unwrap();
        assert_eq!(p, "[Pasted ~1 lines]");
        assert!(needs_placeholder(&long));
    }

    #[test]
    fn blank_lines_count_toward_total() {
        let p = summarize_paste("a\n\nb").unwrap();
        assert_eq!(p, "[Pasted ~3 lines]");
    }

    #[test]
    fn threshold_lengths_match_opencode_default() {
        assert_eq!(PASTE_SUMMARY_LINE_THRESHOLD, 3);
        assert_eq!(PASTE_SUMMARY_LEN_THRESHOLD, 150);
    }

    #[test]
    fn paste_entry_from_short_content_has_placeholder_equal_to_content() {
        let e = PasteEntry::from_content("hello world");
        assert_eq!(e.placeholder, "hello world");
        assert_eq!(e.content, "hello world");
    }

    #[test]
    fn paste_entry_from_long_content_has_summarized_placeholder() {
        let e = PasteEntry::from_content("a\nb\nc\nd");
        assert_eq!(e.placeholder, "[Pasted ~4 lines]");
        assert_eq!(e.content, "a\nb\nc\nd");
    }

    #[test]
    fn needs_placeholder_matches_summarize_paste() {
        for sample in ["", "short", "a\nb", "a\nb\nc", &"x".repeat(200)] {
            assert_eq!(
                needs_placeholder(sample),
                summarize_paste(sample).is_some(),
                "mismatch on {sample:?}",
            );
        }
    }
}
