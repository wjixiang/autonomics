//! Chat input / status row: data model, pure line builder, and
//! shared constants.

pub mod renderer;
pub mod theme;

pub use renderer::render_chat_input;
pub use theme::{ChatInputTheme, DefaultChatInputTheme};

use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// Braille-pattern spinner frames. Indexed by `spinner_tick % 8`.
pub const SPINNER_FRAMES: &[&str] = &[
    "\u{2807}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}", "\u{2827}",
];

/// Discriminates between the four mutually exclusive states the
/// bottom input/status row can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatInputStatus {
    EmptyProviders,
    Running {
        phase: RunningPhase,
        tokens: Option<u64>,
    },
    InputActive,
    Idle,
}

/// Sub-phase of a running agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunningPhase {
    Requesting,
    Streaming,
    Running,
}

impl RunningPhase {
    pub fn label(self) -> &'static str {
        match self {
            RunningPhase::Requesting => "requesting",
            RunningPhase::Streaming => "streaming",
            RunningPhase::Running => "running",
        }
    }
}

const HINT_EMPTY_PROVIDERS: &str = "  [s] open settings  \u{2022}  [q] quit";

/// Build the status line for the given status. Pure: no `Frame`, no side effects.
pub fn build_status_line(
    status: &ChatInputStatus,
    input_text: &str,
    kind_label: &str,
    spinner_tick: usize,
    theme: &dyn ChatInputTheme,
) -> Line<'static> {
    match status {
        ChatInputStatus::EmptyProviders => Line::from(Span::styled(
            HINT_EMPTY_PROVIDERS,
            Style::default().fg(theme.text_muted()),
        )),
        ChatInputStatus::Running { phase, tokens } => {
            let spinner = SPINNER_FRAMES[spinner_tick % SPINNER_FRAMES.len()];
            let usage_suffix = match tokens {
                Some(n) => format!(" ({} tokens)", n),
                None => String::new(),
            };
            Line::from(vec![
                Span::raw("  "),
                Span::styled(spinner, Style::default().fg(theme.spinner_color())),
                Span::styled(
                    format!(" Agent [{}] {}{} ", kind_label, phase.label(), usage_suffix),
                    Style::default().fg(theme.spinner_color()),
                ),
            ])
        }
        ChatInputStatus::InputActive => {
            Line::from(Span::raw(format!("> {}", input_text)))
        }
        ChatInputStatus::Idle => Line::from(vec![
            Span::styled(
                format!("  (Enter to type) [{}] ", kind_label),
                Style::default().fg(theme.text_muted()),
            ),
            Span::styled("[a] switch", Style::default().fg(theme.text_muted())),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Line;

    fn theme() -> DefaultChatInputTheme {
        DefaultChatInputTheme
    }

    fn line_text(l: &Line<'_>) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn empty_providers_uses_static_label() {
        let l = build_status_line(
            &ChatInputStatus::EmptyProviders,
            "",
            "Compose",
            0,
            &theme(),
        );
        assert_eq!(line_text(&l), HINT_EMPTY_PROVIDERS);
    }

    #[test]
    fn idle_uses_kind_label() {
        let l = build_status_line(&ChatInputStatus::Idle, "", "Compose", 0, &theme());
        let text = line_text(&l);
        assert!(text.contains("[Compose]"));
        assert!(text.contains("Enter to type"));
        assert!(text.contains("[a] switch"));
    }

    #[test]
    fn idle_label_changes_with_kind() {
        for kind in &["Compose", "Retrieval", "Parallel"] {
            let l = build_status_line(&ChatInputStatus::Idle, "", kind, 0, &theme());
            assert!(line_text(&l).contains(kind));
        }
    }

    #[test]
    fn input_active_uses_user_text() {
        let l = build_status_line(
            &ChatInputStatus::InputActive,
            "hello world",
            "Compose",
            0,
            &theme(),
        );
        assert_eq!(line_text(&l), "> hello world");
    }

    #[test]
    fn input_active_empty_text_still_renders_prompt() {
        let l = build_status_line(
            &ChatInputStatus::InputActive,
            "",
            "Compose",
            0,
            &theme(),
        );
        assert_eq!(line_text(&l), "> ");
    }

    #[test]
    fn running_includes_phase_and_kind_label() {
        let l = build_status_line(
            &ChatInputStatus::Running {
                phase: RunningPhase::Streaming,
                tokens: None,
            },
            "",
            "Compose",
            0,
            &theme(),
        );
        let text = line_text(&l);
        assert!(text.contains("streaming"));
        assert!(text.contains("[Compose]"));
        assert!(!text.contains("tokens"));
    }

    #[test]
    fn running_with_tokens_appends_count() {
        let l = build_status_line(
            &ChatInputStatus::Running {
                phase: RunningPhase::Streaming,
                tokens: Some(42),
            },
            "",
            "Compose",
            0,
            &theme(),
        );
        assert!(line_text(&l).contains("(42 tokens)"));
    }

    #[test]
    fn running_picks_correct_phase_label() {
        for (phase, expected) in [
            (RunningPhase::Requesting, "requesting"),
            (RunningPhase::Streaming, "streaming"),
            (RunningPhase::Running, "running"),
        ] {
            let l = build_status_line(
                &ChatInputStatus::Running {
                    phase,
                    tokens: None,
                },
                "",
                "Compose",
                0,
                &theme(),
            );
            assert!(
                line_text(&l).contains(expected),
                "expected {:?} in {:?}",
                expected,
                line_text(&l)
            );
        }
    }

    #[test]
    fn running_spinner_cycles_through_eight_frames() {
        for tick in 0..16 {
            let l = build_status_line(
                &ChatInputStatus::Running {
                    phase: RunningPhase::Streaming,
                    tokens: None,
                },
                "",
                "Compose",
                tick,
                &theme(),
            );
            let expected = SPINNER_FRAMES[tick % SPINNER_FRAMES.len()];
            assert_eq!(l.spans[1].content.as_ref(), expected);
        }
    }

    #[test]
    fn spinner_is_ignored_for_non_running_states() {
        for status in [
            ChatInputStatus::EmptyProviders,
            ChatInputStatus::Idle,
            ChatInputStatus::InputActive,
        ] {
            let l = build_status_line(&status, "", "Compose", 999, &theme());
            for span in &l.spans {
                for frame in SPINNER_FRAMES {
                    assert_ne!(
                        span.content.as_ref(),
                        *frame,
                        "spinner frame leaked into non-running state"
                    );
                }
            }
        }
    }

    #[test]
    fn static_states_have_no_dynamic_allocation() {
        let empty = build_status_line(
            &ChatInputStatus::EmptyProviders,
            "ignored",
            "Compose",
            0,
            &theme(),
        );
        assert_eq!(empty.spans.len(), 1);
        assert_eq!(empty.spans[0].content.as_ref(), HINT_EMPTY_PROVIDERS);
    }

    #[test]
    fn input_active_width_ascii() {
        let l = build_status_line(
            &ChatInputStatus::InputActive,
            "hello",
            "Compose",
            0,
            &theme(),
        );
        assert_eq!(l.width(), 7);
    }

    #[test]
    fn input_active_width_cjk() {
        let l = build_status_line(
            &ChatInputStatus::InputActive,
            "你好",
            "Compose",
            0,
            &theme(),
        );
        assert_eq!(l.width(), 6);
    }

    #[test]
    fn input_active_width_mixed_ascii_and_cjk() {
        let l = build_status_line(
            &ChatInputStatus::InputActive,
            "hi 你好 ok",
            "Compose",
            0,
            &theme(),
        );
        assert_eq!(l.width(), 12);
    }

    #[test]
    fn input_active_width_empty() {
        let l = build_status_line(
            &ChatInputStatus::InputActive,
            "",
            "Compose",
            0,
            &theme(),
        );
        assert_eq!(l.width(), 2);
    }
}
