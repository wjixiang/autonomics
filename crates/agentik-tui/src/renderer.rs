//! Pure render functions for the Agent Status Panel.
//!
//! All output goes into a `Paragraph` of pre-built `Line`s; the
//! panel never creates a `Block` or border. The host supplies the
//! surrounding border so the panel can be visually grouped with
//! the chat history (or floated standalone in the future).
//!
//! `render_agent_panel` is parameterized on `&dyn AgentPanelTheme`
//! and `&dyn AgentPanelTools`. Hosts that don't want to implement
//! either pass [`DefaultAgentPanelTheme`] and
//! [`DefaultAgentPanelTools`] respectively.

use std::time::Instant;

use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::{Frame, layout::Rect};

use crate::state::{
    AgentEntryLayout, AgentEntryStatus, AgentPanelEntry, AgentPanelEvent, AgentPanelState,
    MAX_VISIBLE_AGENTS,
};
use crate::theme::AgentPanelTheme;
use crate::tools::{format_duration, truncate_str, AgentPanelTools};

/// Braille-pattern spinner frames. Indexed by the host's
/// `spinner_tick` counter.
const SPINNER_FRAMES: &[&str] = &[
    "\u{2807}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}", "\u{2827}",
];

/// Render the sub-agent list into the given `area`.
///
/// `theme` controls every color and style. `tools` renders the
/// tool-call label shown in the row hint and the expanded event log.
/// `spinner_tick` is the host's monotonically-advancing frame counter;
/// the renderer uses it to pick a spinner frame for running rows.
pub fn render_agent_panel(
    f: &mut Frame,
    state: &AgentPanelState,
    theme: &dyn AgentPanelTheme,
    tools: &dyn AgentPanelTools,
    area: Rect,
    spinner_tick: usize,
) {
    let lines = render_panel_lines(state, theme, tools, area.width as usize, spinner_tick);
    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

fn render_panel_lines(
    state: &AgentPanelState,
    theme: &dyn AgentPanelTheme,
    tools: &dyn AgentPanelTools,
    width: usize,
    spinner_tick: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    let running = state.running_count();
    let completed = state.completed_count();
    let failed = state.failed_count();
    let total = state.agents.len();

    let header = format!(
        " Agents · {} total ({} ✓ · {} ⠋ · {} ✗)",
        total, completed, running, failed
    );
    lines.push(Line::from(Span::styled(header, theme.tool_call_bold_style())));

    if state.agents.is_empty() {
        lines.push(Line::from(Span::styled(
            "    (no agents)".to_string(),
            ratatui::style::Style::default().fg(theme.text_muted()),
        )));
        return lines;
    }

    // Prioritize: running + failed first, then recently completed, then fold old.
    let now = Instant::now();
    let mut visible: Vec<usize> = Vec::new();
    let mut foldable: Vec<usize> = Vec::new();
    for (i, e) in state.agents.iter().enumerate() {
        let is_old_done =
            matches!(e.status, AgentEntryStatus::Completed { .. }) && !e.is_recently_completed(now);
        if is_old_done {
            foldable.push(i);
        } else {
            visible.push(i);
        }
    }
    while visible.len() > MAX_VISIBLE_AGENTS {
        if let Some(pos) = visible
            .iter()
            .position(|&i| matches!(state.agents[i].status, AgentEntryStatus::Completed { .. }))
        {
            foldable.push(visible.remove(pos));
        } else {
            break;
        }
    }

    let visible_count = visible.len();
    let total_to_render = visible_count + if !foldable.is_empty() { 1 } else { 0 };

    for (rank, &i) in visible.iter().enumerate() {
        let entry = &state.agents[i];
        let is_last = rank + 1 == total_to_render;
        let is_selected = i == state.selected;
        let layout = AgentEntryLayout {
            index_1based: i + 1,
            total,
            is_last,
            is_selected,
            now,
        };
        lines.push(render_agent_row(
            entry, &layout, theme, width, spinner_tick,
        ));

        if entry.expanded {
            for ev in &entry.events {
                for ev_line in render_panel_event(ev, theme, tools) {
                    let mut spans = vec![Span::raw("│   ")];
                    spans.extend(ev_line.spans);
                    lines.push(Line::from(spans));
                }
            }
        } else if matches!(entry.status, AgentEntryStatus::Running) {
            let hint = entry
                .activity_hint_with(tools)
                .unwrap_or_else(|| "starting…".to_string());
            lines.push(render_peek_line(&hint, theme, width));
        }
    }

    if !foldable.is_empty() {
        let summary = format!(
            "    … +{} completed  (e expand all · c collapse)",
            foldable.len()
        );
        lines.push(Line::from(Span::styled(
            summary,
            ratatui::style::Style::default()
                .fg(theme.text_muted())
                .add_modifier(Modifier::DIM),
        )));
    }

    lines
}

fn render_agent_row(
    entry: &AgentPanelEntry,
    layout: &AgentEntryLayout,
    theme: &dyn AgentPanelTheme,
    width: usize,
    spinner_tick: usize,
) -> Line<'static> {
    let (icon, icon_style) = match &entry.status {
        AgentEntryStatus::Running => (
            SPINNER_FRAMES[spinner_tick % SPINNER_FRAMES.len()],
            ratatui::style::Style::default().fg(theme.spinner_color()),
        ),
        AgentEntryStatus::Completed { .. } => ("✓", theme.success_style()),
        AgentEntryStatus::Failed { .. } => ("✗", theme.error_style()),
    };

    let title_style = match &entry.status {
        AgentEntryStatus::Running => ratatui::style::Style::default()
            .fg(theme.text_primary())
            .add_modifier(Modifier::BOLD),
        AgentEntryStatus::Failed { .. } => theme.error_style(),
        AgentEntryStatus::Completed { .. } => {
            let base = if entry.is_recently_completed(layout.now) {
                ratatui::style::Style::default().fg(theme.text_primary())
            } else {
                ratatui::style::Style::default().fg(theme.text_muted())
            };
            base.add_modifier(Modifier::CROSSED_OUT)
        }
    };

    let mut meta = format!(" [{}/{}]", layout.index_1based, layout.total);
    if entry.tool_call_count > 0 {
        meta.push_str(&format!(
            " · {} tool{}",
            entry.tool_call_count,
            if entry.tool_call_count == 1 { "" } else { "s" }
        ));
    }
    match &entry.status {
        AgentEntryStatus::Running => {}
        AgentEntryStatus::Completed { .. } | AgentEntryStatus::Failed { .. } => {
            meta.push_str(&format!(" · {}", format_duration(entry.elapsed())));
        }
    }

    let hint = match &entry.status {
        AgentEntryStatus::Running => "starting…".to_string(),
        AgentEntryStatus::Failed { error, .. } => truncate_str(error, 60),
        AgentEntryStatus::Completed { .. } => "done".to_string(),
    };

    let connector = if layout.is_last { "└─ " } else { "├─ " };
    let connector_span = Span::styled(
        connector,
        ratatui::style::Style::default().fg(theme.text_muted()),
    );

    let budget = width.saturating_sub(2);
    let meta_len = meta.chars().count();
    let hint_full = format!("  ↳ {}", hint);
    let hint_len = hint_full.chars().count();
    let prefix_len = connector.chars().count() + icon.chars().count() + 1;
    let available = budget.saturating_sub(prefix_len + meta_len + hint_len + 1);
    let title_max = available.max(8);
    let title_displayed = truncate_str(&entry.title, title_max);

    let title_span = Span::styled(
        format!(" {} ", title_displayed),
        if layout.is_selected {
            title_style.add_modifier(Modifier::BOLD)
        } else {
            title_style
        },
    );

    Line::from(vec![
        connector_span,
        Span::styled(icon.to_string(), icon_style),
        title_span,
        Span::styled(meta, ratatui::style::Style::default().fg(theme.text_muted())),
        Span::styled(
            hint_full,
            ratatui::style::Style::default().fg(theme.text_secondary()),
        ),
    ])
}

fn render_panel_event(
    ev: &AgentPanelEvent,
    theme: &dyn AgentPanelTheme,
    tools: &dyn AgentPanelTools,
) -> Vec<Line<'static>> {
    match ev {
        AgentPanelEvent::LlmResponse(s) if !s.is_empty() => {
            vec![Line::from(vec![
                Span::styled("      💬 ".to_string(), ratatui::style::Style::default()),
                Span::styled(
                    truncate_str(s, 200),
                    ratatui::style::Style::default().fg(theme.text_primary()),
                ),
            ])]
        }
        AgentPanelEvent::LlmResponse(_) => Vec::new(),
        AgentPanelEvent::ToolCall { name, input } => {
            let summary = tools.user_facing_name(name, input);
            vec![Line::from(vec![
                Span::styled("      🔧 ".to_string(), ratatui::style::Style::default()),
                Span::styled(
                    summary,
                    ratatui::style::Style::default().fg(theme.text_secondary()),
                ),
            ])]
        }
        AgentPanelEvent::ToolResult { ok, content } => {
            let (icon, color) = if *ok {
                ("✓", theme.tool_ok())
            } else {
                ("✗", theme.tool_err())
            };
            let summary = if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
                if let Some(s) = val.as_str() {
                    s.to_string()
                } else {
                    format!("{}", val)
                }
            } else {
                truncate_str(content, 80)
            };
            vec![Line::from(vec![
                Span::styled(
                    format!("      {} ", icon),
                    ratatui::style::Style::default().fg(color),
                ),
                Span::styled(
                    summary,
                    ratatui::style::Style::default().fg(theme.text_muted()),
                ),
            ])]
        }
        AgentPanelEvent::Error(msg) => vec![Line::from(vec![
            Span::styled("      ✗ ".to_string(), theme.error_style()),
            Span::styled(truncate_str(msg, 200), theme.error_style()),
        ])],
    }
}

fn render_peek_line(hint: &str, theme: &dyn AgentPanelTheme, width: usize) -> Line<'static> {
    let prefix = "│   ↳ ";
    let budget = width.saturating_sub(prefix.chars().count());
    let truncated = truncate_str(hint, budget);
    Line::from(vec![
        Span::raw(prefix),
        Span::styled(
            truncated,
            ratatui::style::Style::default().fg(theme.text_secondary()),
        ),
    ])
}
