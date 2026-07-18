//! Rendering for the Config tab: provider/model lists and edit forms.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    prelude::{Buffer, StatefulWidget},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Widget,
    },
};

use crossterm::event::{KeyCode, KeyEvent};

use crate::state::{ConfigMode, ConfigPane, ConfigTabState, ModelForm, ProviderForm};
use crate::widgets::input_area::InputState;

// ── Shared style helpers ────────────────────────────────

/// Selection highlight applied to the active list row and focused form fields.
fn highlight_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

/// Symbol prepended to the highlighted list row.
const HIGHLIGHT_SYMBOL: &str = "▶ ";

/// Build a Block border for a list pane. Active panes get a thick yellow border,
/// inactive panes get a plain dark-gray border.
fn pane_block(title: &str, count: usize, active: bool) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(if active {
            BorderType::Thick
        } else {
            BorderType::Plain
        })
        .border_style(if active {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        })
        .title(Line::from(vec![
            Span::raw(format!(" {title} ")),
            Span::styled(format!("({count}) "), Style::default().fg(Color::DarkGray)),
        ]))
}

/// Public entry point used by the render path.
pub fn render_config(state: &ConfigTabState, area: Rect, buf: &mut Buffer) {
    match &state.mode {
        ConfigMode::Browsing => render_browsing(state, area, buf),
        ConfigMode::EditProvider(form) => {
            render_browsing(state, area, buf);
            let popup = centered(area, 70, 60);
            Clear.render(popup, buf);
            render_provider_form(form, &state.message, popup, buf);
        }
        ConfigMode::EditModel(form) => {
            render_browsing(state, area, buf);
            let popup = centered(area, 80, 75);
            Clear.render(popup, buf);
            render_model_form(form, &state.providers, &state.message, popup, buf);
        }
    }
}

fn render_browsing(state: &ConfigTabState, area: Rect, buf: &mut Buffer) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[0]);

    render_provider_list(state, panes[0], buf);
    render_model_list(state, panes[1], buf);
    render_hint(&state.message, chunks[1], buf);
}

/// Render a selectable list pane (providers or models).
fn render_selectable_list<T>(
    title: &str,
    count: usize,
    active: bool,
    rows: &[T],
    selected: usize,
    area: Rect,
    buf: &mut Buffer,
    map_item: impl Fn(&T) -> ListItem,
) {
    let items: Vec<ListItem> = rows.iter().map(map_item).collect();

    let list = List::new(items)
        .block(pane_block(title, count, active))
        .highlight_style(highlight_style())
        .highlight_symbol(HIGHLIGHT_SYMBOL);

    let mut list_state = ListState::default();
    list_state.select(if rows.is_empty() {
        None
    } else {
        Some(selected)
    });
    StatefulWidget::render(list, area, buf, &mut list_state);
}

fn render_provider_list(state: &ConfigTabState, area: Rect, buf: &mut Buffer) {
    render_selectable_list(
        "Providers",
        state.providers.len(),
        state.pane == ConfigPane::Providers,
        &state.providers,
        state.selected_provider,
        area,
        buf,
        |p| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<14} ", p.name),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    p.provider_type.clone(),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        },
    );
}

fn render_model_list(state: &ConfigTabState, area: Rect, buf: &mut Buffer) {
    let provider_name = |pid: i64| -> String {
        state
            .providers
            .iter()
            .find(|p| p.id == pid)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| format!("#{pid}"))
    };
    render_selectable_list(
        "Models",
        state.models.len(),
        state.pane == ConfigPane::Models,
        &state.models,
        state.selected_model,
        area,
        buf,
        |m| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<28} ", m.model_name),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    provider_name(m.provider_id),
                    Style::default().fg(Color::Cyan),
                ),
            ]))
        },
    );
}

fn render_hint(message: &str, area: Rect, buf: &mut Buffer) {
    let hint = if message.is_empty() {
        "[n] new  [e] edit  [d] delete  [Tab] switch pane  [Esc] clear msg".to_string()
    } else {
        message.to_string()
    };
    let style = if message.is_empty() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::Yellow)
    };
    Paragraph::new(hint)
        .style(style)
        .alignment(Alignment::Center)
        .render(area, buf);
}

// ── Forms ──────────────────────────────────────────────

fn render_provider_form(form: &ProviderForm, message: &str, area: Rect, buf: &mut Buffer) {
    let block = form_block(
        if form.id.is_some() {
            " Edit Provider "
        } else {
            " New Provider "
        },
        message,
    );
    let inner = block.inner(area);
    block.render(area, buf);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            ProviderForm::FIELDS
                .iter()
                .map(|_| Constraint::Length(1))
                .chain([Constraint::Min(0)])
                .collect::<Vec<_>>(),
        )
        .split(inner);

    for (i, label) in ProviderForm::FIELDS.iter().enumerate() {
        if i == 3 {
            // API Key field: masked rendering
            render_masked_field_line(rows[i], label, &form.fields()[i], i == form.focus, buf);
        } else {
            render_field_line(rows[i], label, &form.fields()[i], i == form.focus, buf);
        }
    }
}

fn render_model_form(
    form: &ModelForm,
    providers: &[crate::config_db::ProviderRow],
    message: &str,
    area: Rect,
    buf: &mut Buffer,
) {
    let block = form_block(
        if form.id.is_some() {
            " Edit Model "
        } else {
            " New Model "
        },
        message,
    );
    let inner = block.inner(area);
    block.render(area, buf);

    // 5 text fields + provider selector + toggles row
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // model name
            Constraint::Length(1), // provider
            Constraint::Length(1), // context
            Constraint::Length(1), // max output
            Constraint::Length(1), // input price
            Constraint::Length(1), // output price
            Constraint::Length(1), // toggles
            Constraint::Min(0),
        ])
        .split(inner);

    let labels = [
        "Model Name",
        "Provider",
        "Context Len",
        "Max Output",
        "In Price",
        "Out Price",
    ];

    // text fields 0..5 in the form correspond to rows 0,2,3,4,5 (skip provider row 1)
    let text_to_row = [0usize, 2, 3, 4, 5];
    for (form_idx, &row_idx) in text_to_row.iter().enumerate() {
        render_field_line(
            rows[row_idx],
            labels[row_idx],
            &form.text_fields()[form_idx],
            form.focus == form_idx,
            buf,
        );
    }

    // Provider selector row
    let provider_focus = form.focus == ModelForm::TEXT_FIELD_COUNT;
    let pv = providers
        .get(form.provider_index)
        .map(|p| format!("{} (#{})", p.name, p.id))
        .unwrap_or_else(|| "<no provider>".to_string());
    // Provider selector row is read-only (Up/Down cycles through the
    // list), so render it through `render_field_line` with a scratch
    // InputState populated from the resolved provider name. Cursor
    // lands at end-of-text and signals focus via the yellow ▏ glyph.
    let mut provider_display = InputState::new();
    let _ = provider_display.insert_str(&pv);
    render_field_line(rows[1], "Provider", &provider_display, provider_focus, buf);

    // Toggles row
    let toggle_focus = form.focus == ModelForm::TEXT_FIELD_COUNT + 1;
    let toggles = format!(
        "[FnCall:{}] [Stream:{}] [Think:{}] [Vision:{}]",
        yn(form.supports_function_calling),
        yn(form.supports_streaming),
        yn(form.supports_thinking),
        yn(form.vision_ability)
    );
    let style = if toggle_focus {
        highlight_style()
    } else {
        Style::default()
    };
    Paragraph::new(format!(" Toggles:  {toggles}"))
        .style(style)
        .render(rows[6], buf);
}

fn yn(b: bool) -> &'static str {
    if b { "Y" } else { "n" }
}

fn render_field_line(row: Rect, label: &str, input: &InputState, focused: bool, buf: &mut Buffer) {
    let _ = render_field_line_impl(row, label, input.value(), input.cursor(), focused, None, buf);
}

/// Like [`render_field_line`] but replaces the displayed value with bullet characters
/// to hide sensitive content (e.g. API keys).  The underlying value and cursor are
/// unchanged — only the visual representation is masked.
fn render_masked_field_line(row: Rect, label: &str, input: &InputState, focused: bool, buf: &mut Buffer) {
    let value = input.value();
    let graphemes_before = unicode_segmentation::UnicodeSegmentation::graphemes(&value[..input.cursor().min(value.len())], true).count();
    let mask = "•".repeat(unicode_segmentation::UnicodeSegmentation::graphemes(value, true).count());
    // Map the grapheme-aligned cursor to a byte offset in the mask string.
    let mask_cursor = graphemes_before * "•".len();
    render_field_line_impl(row, label, value, mask_cursor, focused, Some(&mask), buf);
}

fn render_field_line_impl(
    row: Rect,
    label: &str,
    value: &str,
    cursor: usize,
    focused: bool,
    mask: Option<&str>,
    buf: &mut Buffer,
) {
    let label_span = Span::styled(
        format!(" {:<11}: ", label),
        Style::default().fg(Color::Cyan),
    );
    let mut spans = vec![label_span];
    let display = mask.unwrap_or(value);
    if display.is_empty() {
        // Empty field: dim hint text (no highlight bg, the active border is the cue).
        spans.push(Span::styled(
            if focused { "▏" } else { "·" }.to_string(),
            Style::default().fg(if focused {
                Color::Yellow
            } else {
                Color::DarkGray
            }),
        ));
    } else {
        // When masked the cursor byte offset maps directly onto the uniform-width
        // mask string ("•••…"), so we can split at the same byte position.
        let cursor = cursor.min(display.len());
        let before = &display[..cursor];
        let after = &display[cursor..];
        if focused {
            if !before.is_empty() {
                spans.push(Span::styled(
                    before.to_string(),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            spans.push(Span::styled(
                "▏".to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::SLOW_BLINK),
            ));
            if !after.is_empty() {
                spans.push(Span::styled(after.to_string(), Style::default().fg(Color::White)));
            }
        } else {
            spans.push(Span::styled(
                display.to_string(),
                Style::default().fg(Color::White),
            ));
        }
    }
    Paragraph::new(Line::from(spans)).render(row, buf);
}

fn form_block<'a>(title: &'a str, message: &'a str) -> Block<'a> {
    let title_line = if message.is_empty() {
        Line::from(title).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Line::from(vec![
            Span::styled(title, Style::default().fg(Color::Yellow)),
            Span::raw("  "),
            Span::styled(
                message,
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
        ])
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Yellow))
        .padding(Padding::horizontal(1))
        .title_top(title_line)
        .title_bottom(
            Line::from("[Tab/↑↓] move  [Enter] save  [Esc] cancel").alignment(Alignment::Center),
        )
}

fn centered(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let popup = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup[1])[1]
}

/// Key handling for the Config tab. Returns true if the key was consumed.
pub fn handle_config_key(state: &mut ConfigTabState, key: KeyEvent) -> bool {
    // Split borrows: match on `mode` mutably, and (for model forms) read
    // `providers` as a disjoint immutable field.
    let providers = &state.providers;
    match &mut state.mode {
        ConfigMode::Browsing => handle_browse_key(state, key),
        ConfigMode::EditProvider(form) => handle_provider_form_key(form, key),
        ConfigMode::EditModel(form) => handle_model_form_key(providers, form, key),
    }
}

fn handle_browse_key(state: &mut ConfigTabState, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            state.move_selection(1);
            true
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.move_selection(-1);
            true
        }
        KeyCode::Tab => {
            state.pane = match state.pane {
                ConfigPane::Providers => ConfigPane::Models,
                ConfigPane::Models => ConfigPane::Providers,
            };
            true
        }
        KeyCode::Char('h') => {
            state.pane = ConfigPane::Providers;
            true
        }
        KeyCode::Char('l') => {
            state.pane = ConfigPane::Models;
            true
        }
        KeyCode::Char('n') => {
            state.message.clear();
            match state.pane {
                ConfigPane::Providers => {
                    state.mode = ConfigMode::EditProvider(ProviderForm::new(
                        crate::config_db::ProviderInput::empty(),
                        None,
                    ));
                }
                ConfigPane::Models => {
                    state.mode = ConfigMode::EditModel(ModelForm::new(
                        crate::config_db::ModelInput::empty(),
                        None,
                        &state.providers,
                    ));
                }
            }
            true
        }
        KeyCode::Char('e') | KeyCode::Enter => {
            state.message.clear();
            match state.pane {
                ConfigPane::Providers => {
                    if let Some(p) = state.selected_provider_row().cloned() {
                        state.mode = ConfigMode::EditProvider(ProviderForm::new(
                            crate::config_db::ProviderInput {
                                name: p.name,
                                provider_type: p.provider_type,
                                base_url: p.base_url,
                                api_key: p.api_key,
                                auth_method: p.auth_method,
                            },
                            Some(p.id),
                        ));
                    }
                }
                ConfigPane::Models => {
                    if let Some(m) = state.selected_model_row().cloned() {
                        state.mode = ConfigMode::EditModel(ModelForm::new(
                            crate::config_db::ModelInput {
                                model_name: m.model_name,
                                provider_id: m.provider_id,
                                context_length: m.context_length,
                                max_output_tokens: m.max_output_tokens,
                                vision_ability: m.vision_ability,
                                supports_function_calling: m.supports_function_calling,
                                supports_streaming: m.supports_streaming,
                                supports_thinking: m.supports_thinking,
                                input_token_price: m.input_token_price,
                                output_token_price: m.output_token_price,
                            },
                            Some(m.id),
                            &state.providers,
                        ));
                    }
                }
            }
            true
        }
        KeyCode::Char('d') => {
            // Delete is destructive but the user explicitly asked for it.
            // Actual deletion is committed by the caller (App) which owns the
            // connection; here we just signal via a sentinel message.
            state.message.clear();
            false // not consumed here — App handles 'd' so it can hit the DB
        }
        _ => false,
    }
}

fn handle_provider_form_key(form: &mut ProviderForm, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => true, // caller closes the form
        KeyCode::Tab => {
            form.focus = (form.focus + 1) % ProviderForm::FIELDS.len();
            true
        }
        KeyCode::BackTab => {
            if form.focus == 0 {
                form.focus = ProviderForm::FIELDS.len() - 1;
            } else {
                form.focus -= 1;
            }
            true
        }
        KeyCode::Up => {
            if form.focus == 0 {
                form.focus = ProviderForm::FIELDS.len() - 1;
            } else {
                form.focus -= 1;
            }
            true
        }
        KeyCode::Down => {
            form.focus = (form.focus + 1) % ProviderForm::FIELDS.len();
            true
        }
        KeyCode::Enter => true, // caller validates & saves
        _ => {
            let f = form.focus;
            form.fields_mut()[f].handle_key(key);
            true
        }
    }
}

fn handle_model_form_key(
    providers: &[crate::config_db::ProviderRow],
    form: &mut ModelForm,
    key: KeyEvent,
) -> bool {
    // Field indices: 0..4 text fields, 5 provider, 6 toggles
    let total = ModelForm::TEXT_FIELD_COUNT + 2;
    match key.code {
        KeyCode::Esc => true, // caller closes
        KeyCode::Tab => {
            form.focus = (form.focus + 1) % total;
            true
        }
        KeyCode::BackTab => {
            if form.focus == 0 {
                form.focus = total - 1;
            } else {
                form.focus -= 1;
            }
            true
        }
        KeyCode::Up => {
            if form.focus == 0 {
                form.focus = total - 1;
            } else {
                form.focus -= 1;
            }
            true
        }
        KeyCode::Down => {
            form.focus = (form.focus + 1) % total;
            true
        }
        KeyCode::Enter => true, // caller validates & saves
        _ => {
            if form.focus < ModelForm::TEXT_FIELD_COUNT {
                let f = form.focus;
                form.text_fields_mut()[f].handle_key(key);
                true
            } else if form.focus == ModelForm::TEXT_FIELD_COUNT {
                // Provider selector: cycle with Left/Right or h/l
                match key.code {
                    KeyCode::Left | KeyCode::Char('h') => {
                        if !providers.is_empty() {
                            let len = providers.len();
                            form.provider_index = (form.provider_index + len - 1) % len;
                        }
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        if !providers.is_empty() {
                            form.provider_index = (form.provider_index + 1) % providers.len();
                        }
                    }
                    _ => {}
                }
                true
            } else {
                // Toggles row: cycle which toggle with Left/Right, flip with Space/Enter-ish
                // Keep it simple: Space flips the matching toggle based on a sub-focus.
                // For minimal UX, map number keys 1-4 to flip the four toggles.
                match key.code {
                    KeyCode::Char('1') => form.supports_function_calling.toggle(),
                    KeyCode::Char('2') => form.supports_streaming.toggle(),
                    KeyCode::Char('3') => form.supports_thinking.toggle(),
                    KeyCode::Char('4') => form.vision_ability.toggle(),
                    _ => {}
                }
                true
            }
        }
    }
}

/// Helper trait so we can write `.toggle()` on bool fields.
trait BoolToggle {
    fn toggle(&mut self);
}

impl BoolToggle for bool {
    fn toggle(&mut self) {
        *self = !*self;
    }
}
