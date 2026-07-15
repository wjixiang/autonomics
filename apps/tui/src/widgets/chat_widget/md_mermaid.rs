//! Synchronous `` ```mermaid `` block renderer.
//!
//! Wraps the vendored [`mermaid_text`] crate (pure-Rust mermaid
//! → Unicode box-drawing text) into the same rounded-box chrome used
//! by `DisplayMath` and fenced code blocks.  No async, no image
//! protocols, no graphics-picker probe — the chat_widget's render path
//! is fully synchronous (`render_markdown_to_lines` at
//! `chat_widget/md_renderer.rs:32`), so the integration is a direct,
//! blocking call into `mermaid_text::render_with_options`.
//!
//! Caching: the chat_widget already memoises rendered lines at the
//! message level (`chat_widget.rs:55-67` + `agent_tab_widget.rs:68-92`,
//! keyed on `(messages_version, available_width)`).  Adding a finer
//! per-block LRU keyed on `hash(source)` would not save meaningful
//! work for typical LLM-emitted mermaid (3–20 nodes, <50 ms render)
//! and would bring an extra dep.  When mermaid-text is called, the
//! coarse-grained cache already prevents the work from re-running on
//! every scroll keypress.
//!
//! Defensive bound: [`ASCII_DIAGRAM_HARD_CAP`] (1000 lines) guards
//! against pathological input that would otherwise produce an absurd
//! allocation.

use ratatui::{
    style::{Style, Stylize},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use super::md_theme::MdTokens;

/// Defensive upper bound on rendered mermaid body height.  A
/// 100-line block is already exceptional; clipping at 1000 prevents
/// accidental allocation blow-ups without affecting normal use.
const ASCII_DIAGRAM_HARD_CAP: usize = 1000;

/// Minimum inner width passed to `mermaid_text`.  Below this the
/// layered-layout engine refuses to compact further and the diagram
/// is rendered at its natural (possibly wider) size, potentially
/// wrapping terminal columns but still readable.
const MERMAID_MIN_INNER_WIDTH: usize = 20;

/// Render a fenced `` ```mermaid `` block.
///
/// On success the body's Unicode box-drawing art is wrapped in a
/// rounded `╭ mermaid ╮` box, mirroring the `╭ math ╮` chrome of
/// `DisplayMath` (see `md_renderer.rs:197-256`).  On failure the same
/// chrome is emitted but the body shows the raw source with an error
/// banner, so user-typed mermaid never crashes the render path.
///
/// # Arguments
///
/// * `source`         – raw mermaid source (e.g. `"graph LR\nA-->B"`).
/// * `available_width`– terminal columns available for the block;
///                      borders and 2-cell padding subtract from this
///                      before being passed to `mermaid_text`.
/// * `tokens`         – design tokens, used for the chrome theme.
pub(crate) fn render_mermaid_block(
    source: &str,
    available_width: usize,
    tokens: &MdTokens,
) -> Vec<Line<'static>> {
    // Total chrome consumes 4 cells (left `│ `, right `│`, plus 1
    // safety margin).  Floor to MERMAID_MIN_INNER_WIDTH so very
    // narrow panes render at a reasonable size rather than crushing
    // node labels to illegibility.
    let max_w = available_width
        .saturating_sub(4)
        .max(MERMAID_MIN_INNER_WIDTH);

    let opts = mermaid_text::RenderOptions {
        max_width: Some(max_w),
        backend: mermaid_text::layout::LayoutBackend::Sugiyama,
        ..Default::default()
    };

    let body = match mermaid_text::render_with_options(source, &opts) {
        Ok(s) => s,
        Err(e) => return render_fallback(source, &format!("{e}"), tokens, max_w),
    };

    let max_line_width = body
        .lines()
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0)
        .max(MERMAID_MIN_INNER_WIDTH);
    // `inner_width` is the column budget for body content; the
    // rendered line is `│ ` + content_padded + `│` = inner_width + 3.
    let inner_width = max_line_width.max(max_w) + 1;

    let mut lines = Vec::with_capacity(body.lines().count() + 3);
    push_top_border(&mut lines, " mermaid ", inner_width, tokens);
    push_body_lines(&mut lines, &body, inner_width, tokens);
    push_bottom_border(&mut lines, inner_width, tokens);
    lines
}

fn render_fallback(
    source: &str,
    err: &str,
    tokens: &MdTokens,
    inner_width: usize,
) -> Vec<Line<'static>> {
    let border = Style::default().fg(tokens.syntax.code_border);
    let side = Style::default()
        .fg(tokens.syntax.code_border)
        .bg(tokens.surface.raised);
    let error_style = Style::default()
        .fg(tokens.syntax.code_fg)
        .bg(tokens.surface.raised)
        .add_modifier(ratatui::style::Modifier::BOLD);
    let body_style = Style::default()
        .fg(tokens.syntax.code_fg)
        .bg(tokens.surface.raised);

    let label = " mermaid ⚠ ";
    let mut lines = Vec::new();

    // Top border with the warning label so the user can tell at a
    // glance the diagram didn't render.
    lines.push(Line::from(vec![
        Span::styled("╭".to_string(), border),
        Span::styled(
            label.to_string(),
            Style::default()
                .fg(tokens.syntax.inline_code)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "{}╮",
                "─".repeat(inner_width + 1 - label.len().min(inner_width))
            ),
            border,
        ),
    ]));

    // Error banner.
    lines.push(Line::from(vec![
        Span::styled("│ ".to_string(), side),
        Span::styled(format!("render failed: {err}"), error_style),
        Span::styled("│".to_string(), side),
    ]));

    // Source lines (clamped to ASCII_DIAGRAM_HARD_CAP, with 1 entry
    // reserved for an ellipsis tail if we actually hit the cap).
    let total_source = source.lines().count();
    let source_lines: Vec<&str> = source.lines().take(ASCII_DIAGRAM_HARD_CAP).collect();
    let more = total_source.saturating_sub(source_lines.len());
    let truncated = more > 0;
    for line in &source_lines {
        let truncated_line = if line.width() > inner_width {
            // Hard wrap extremely long source lines so they fit the
            // box without visual corruption.
            line.chars().take(inner_width).collect::<String>()
        } else {
            line.to_string()
        };
        lines.push(Line::from(vec![
            Span::styled("│ ".to_string(), side),
            Span::styled(format!("{truncated_line:<inner_width$}"), body_style),
            Span::styled("│".to_string(), side),
        ]));
    }
    if truncated {
        lines.push(Line::from(vec![
            Span::styled("│ ".to_string(), side),
            Span::styled(format!("… ({more} more lines)"), body_style),
            Span::styled("│".to_string(), side),
        ]));
    }

    lines.push(Line::from(Span::styled(
        format!("╰{}╯", "─".repeat(inner_width + 1)),
        border,
    )));
    lines
}

fn push_top_border(
    lines: &mut Vec<Line<'static>>,
    label: &str,
    inner_width: usize,
    tokens: &MdTokens,
) {
    let border = Style::default().fg(tokens.syntax.code_border);
    lines.push(Line::from(vec![
        Span::styled("╭".to_string(), border),
        Span::styled(
            label.to_string(),
            Style::default().fg(tokens.syntax.inline_code).bold(),
        ),
        Span::styled(
            format!(
                "{}╮",
                "─".repeat(inner_width + 1 - label.len().min(inner_width))
            ),
            border,
        ),
    ]));
}

fn push_bottom_border(lines: &mut Vec<Line<'static>>, inner_width: usize, tokens: &MdTokens) {
    lines.push(Line::from(Span::styled(
        format!("╰{}╯", "─".repeat(inner_width + 1)),
        Style::default().fg(tokens.syntax.code_border),
    )));
}

fn push_body_lines(
    lines: &mut Vec<Line<'static>>,
    body: &str,
    inner_width: usize,
    tokens: &MdTokens,
) {
    let side = Style::default()
        .fg(tokens.syntax.code_border)
        .bg(tokens.surface.raised);
    let body_style = Style::default()
        .fg(tokens.syntax.code_fg)
        .bg(tokens.surface.raised);

    let mut count = 0;
    for raw in body.lines() {
        if count >= ASCII_DIAGRAM_HARD_CAP {
            lines.push(Line::from(vec![
                Span::styled("│ ".to_string(), side),
                Span::styled(
                    format!("… (diagram truncated at {ASCII_DIAGRAM_HARD_CAP} lines)"),
                    body_style,
                ),
                Span::styled("│".to_string(), side),
            ]));
            break;
        }
        // Hard-wrap lines whose display width exceeds inner_width by
        // taking only the first `inner_width` code points.  Real
        // mermaid-text output fits `max_width` by construction; this
        // is a paranoid guard for users who do not pass a width.
        let truncated = if raw.width() > inner_width {
            raw.chars().take(inner_width).collect::<String>()
        } else {
            raw.to_string()
        };
        lines.push(Line::from(vec![
            Span::styled("│ ".to_string(), side),
            Span::styled(format!("{truncated:<inner_width$}"), body_style),
            Span::styled("│".to_string(), side),
        ]));
        count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::chat_widget::md_theme::MdTokens;

    fn dark() -> MdTokens {
        MdTokens::dark()
    }

    /// Basic flowchart must produce a box whose body contains all
    /// node labels emitted by mermaid-text (Build/Test/Deploy).
    #[test]
    fn basic_flowchart_renders_nodes() {
        let src = "graph LR\nA[Build] --> B[Test] --> C[Deploy]\n";
        let lines = render_mermaid_block(src, 80, &dark());
        assert!(!lines.is_empty(), "expected non-empty output");

        let joined: String = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("Build"), "missing Build label:\n{joined}");
        assert!(joined.contains("Test"), "missing Test label:\n{joined}");
        assert!(joined.contains("Deploy"), "missing Deploy label:\n{joined}");
        assert!(joined.starts_with('╭'), "missing top-border:\n{joined}");
        assert!(joined.contains('─'), "missing horizontal border:\n{joined}");
    }

    /// Width-constrained rendering must not emit any line whose
    /// display width exceeds the supplied `available_width`.
    #[test]
    fn width_constrained_rendering() {
        let src = "graph LR\nA[Build] --> B[Test] --> C[Deploy]\n";
        let width = 40;
        let lines = render_mermaid_block(src, width, &dark());
        for line in &lines {
            let w = line.to_string().width();
            assert!(
                w <= width,
                "line wider than budget (got {w}, budget {width}): {line:?}"
            );
        }
    }

    /// Invalid mermaid must NOT panic and must fall back to rendering
    /// the raw source lines inside the same box chrome.
    #[test]
    fn invalid_syntax_falls_back() {
        let src = "notADiagramType\nfoo bar baz\n";
        let lines = render_mermaid_block(src, 80, &dark());
        assert!(!lines.is_empty(), "fallback should still emit chrome");
        let joined: String = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("notADiagramType"),
            "fallback should expose raw source:\n{joined}"
        );
        assert!(
            joined.contains("render failed"),
            "fallback should advertise the failure:\n{joined}"
        );
    }

    /// Empty input must return at least the chrome box (top + bottom
    /// border) and never panic.
    #[test]
    fn empty_input_does_not_panic() {
        let lines = render_mermaid_block("", 80, &dark());
        assert!(
            !lines.is_empty(),
            "expected chrome box even for empty input"
        );
        let joined = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.starts_with('╭'), "expected top border:\n{joined}");
        assert!(joined.contains('╯'), "expected bottom border:\n{joined}");
    }

    /// Duplicate of the upstream `has_limited_rendering` predicate —
    /// kept for parity tests and to document what v2 might surface
    /// in a UI for users.
    #[allow(dead_code)]
    fn has_limited_rendering(source: &str) -> bool {
        source.trim_start().starts_with("stateDiagram")
    }

    #[test]
    fn has_limited_rounding_detects_state_diagrams() {
        assert!(has_limited_rendering("stateDiagram-v2\n  s1 --> s2"));
        assert!(has_limited_rendering("  stateDiagram\n  X --> Y"));
        assert!(!has_limited_rendering("graph LR\nA-->B"));
    }
}
