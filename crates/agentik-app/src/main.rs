//! `agentik-app` — host binary that wires the runtime, agent kinds, and
//! the TUI together.
//!
//! Architecture:
//! - Registers agent kinds into the runtime registry (see `kinds.rs`).
//! - Reads `data/settings.json` at startup, configures the shared pool.
//! - Runs a ratatui terminal loop hosting agentik-tui panels.
//! - Settings panel actions → configure/reconfigure the pool.
//! - Chat panel drives agent spawn-by-kind and message injection.

mod kinds;
mod settings_io;

use std::io;
use std::sync::Arc;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use agentik_runtime::{AgentRegistry, ModelConfig, ProcessManager};
use agentik_tui::{
    render_settings_panel, ChatPanelState, ChatMessage, SettingsAction, SettingsKey, SettingsKeyCode,
    SettingsKeyModifiers, SettingsPanelState,
    handle_settings_key,
};

/// Path to the settings file (relative to CWD).
const SETTINGS_FILE: &str = "data/settings.json";

/// The application state held by the main loop.
struct App {
    manager: ProcessManager,
    settings: SettingsPanelState,
    chat: ChatPanelState<String>,
    /// Cached pool model count (updated on configure/reconfigure).
    pool_model_count: usize,
}

impl App {
    fn new(manager: ProcessManager, initial_config: &ModelConfig, pool_model_count: usize) -> Self {
        App {
            manager,
            settings: SettingsPanelState::from_config(initial_config),
            chat: ChatPanelState::new("default".to_string()),
            pool_model_count,
        }
    }

    /// Apply a settings action (from the settings panel).
    fn apply_settings_action(&mut self, action: SettingsAction) {
        match action {
            SettingsAction::Apply(config) => {
                tracing::info!(
                    "Settings applied: {} providers, {} pool entries",
                    config.providers.len(),
                    config.pool.len()
                );
                // Store the config so the runtime block can use it.
                // In a real app this would be sent to an async task.
                // For now, we just log it.
            }
            SettingsAction::Cancel => {
                tracing::info!("Settings cancelled");
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── Init tracing ─────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // ── Create runtime components ─────────────────────
    let registry = Arc::new(AgentRegistry::new());
    registry.register(Arc::new(kinds::GenericCoderKind::new()));

    let manager = ProcessManager::with_registry_and_pool(registry, Default::default());

    // ── Load settings ─────────────────────────────────
    let initial_config = settings_io::load_settings(SETTINGS_FILE);
    let rt = tokio::runtime::Runtime::new()?;

    // Configure pool.
    let pool_model_count = rt.block_on(async {
        match manager.configure_pool(&initial_config).await {
            Ok(()) => {
                let names = manager.pool_model_names().await;
                tracing::info!("Pool configured with {} models", names.len());
                names.len()
            }
            Err(e) => {
                tracing::warn!("Failed to configure pool: {e}");
                0
            }
        }
    });

    // ── Build app ──────────────────────────────────────
    let mut app = App::new(manager, &initial_config, pool_model_count);

    // ── Terminal setup ────────────────────────────────
    let mut terminal = init_terminal()?;

    // ── Event loop ────────────────────────────────────
    loop {
        terminal.draw(|frame| {
            render_app(frame, &app);
        })?;

        if crossterm::event::poll(std::time::Duration::from_millis(100))? {
            match crossterm::event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if handle_key(&mut app, key) {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    // ── Shutdown ──────────────────────────────────────
    restore_terminal()?;
    // TODO: proper async shutdown via runtime.block_on(manager.shutdown())
    tracing::info!("Shutdown complete");
    Ok(())
}

/// Handle a key event.  Returns `true` if the app should quit.
fn handle_key(app: &mut App, key: event::KeyEvent) -> bool {
    // Settings panel captures input when open.
    if app.settings.is_open() {
        let settings_key = SettingsKey {
            code: map_key_code(key.code),
            modifiers: SettingsKeyModifiers {
                shift: key.modifiers.contains(event::KeyModifiers::SHIFT),
            },
        };
        if let Some(action) = handle_settings_key(&mut app.settings, settings_key) {
            app.apply_settings_action(action);
        }
        return false;
    }

    // Global keys
    match key.code {
        KeyCode::Char('q') => return true,
        KeyCode::Char('s') => {
            app.settings.toggle_open();
        }
        KeyCode::Char('?') => {
            // Help overlay (TODO)
        }
        _ => {}
    }
    false
}

fn map_key_code(code: KeyCode) -> SettingsKeyCode {
    match code {
        KeyCode::Char(c) => SettingsKeyCode::Char(c),
        KeyCode::Up => SettingsKeyCode::Up,
        KeyCode::Down => SettingsKeyCode::Down,
        KeyCode::Left => SettingsKeyCode::Left,
        KeyCode::Right => SettingsKeyCode::Right,
        KeyCode::Enter => SettingsKeyCode::Enter,
        KeyCode::Backspace => SettingsKeyCode::Backspace,
        KeyCode::Tab => SettingsKeyCode::Tab,
        KeyCode::BackTab => SettingsKeyCode::BackTab,
        KeyCode::Esc => SettingsKeyCode::Esc,
        KeyCode::Delete => SettingsKeyCode::Delete,
        _ => SettingsKeyCode::Char('\x00'), // unknown
    }
}

fn render_app(frame: &mut ratatui::Frame, app: &App) {
    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Percentage(80), // main content
            ratatui::layout::Constraint::Percentage(20), // status / input
        ])
        .split(frame.area());

    // Chat area (placeholder for now — will be wired to agent events).
    let chat_lines: Vec<ratatui::text::Line> = app
        .chat
        .current_messages()
        .iter()
        .flat_map(|msg| msg.to_lines(&agentik_tui::chat::theme::DefaultChatPanelTheme))
        .collect();
    let chat_widget = ratatui::widgets::Paragraph::new(chat_lines)
        .wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(chat_widget, chunks[0]);

    // Status bar.
    let status_text = format!(
        " {} | {} models | [s] settings | [q] quit",
        if app.settings.is_open() { "⚙ Settings" } else { "● Ready" },
        app.pool_model_count,
    );
    let status = ratatui::widgets::Paragraph::new(status_text)
        .style(ratatui::style::Style::default().fg(ratatui::style::Color::Gray));
    frame.render_widget(status, chunks[1]);

    // Settings overlay.
    if app.settings.is_open() {
        let settings_lines = render_settings_panel(
            &app.settings,
            &agentik_tui::DefaultSettingsTheme,
        );
        let settings_area = centered_rect(60, 20, frame.area());
        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(ratatui::style::Color::Cyan));
        let settings_widget = ratatui::widgets::Paragraph::new(settings_lines)
            .block(block);
        frame.render_widget(settings_widget, settings_area);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let popup_width = area.width * percent_x / 100;
    let popup_height = area.height * percent_y / 100;
    ratatui::layout::Rect {
        x: area.width.saturating_sub(popup_width) / 2,
        y: area.height.saturating_sub(popup_height) / 2,
        width: popup_width,
        height: popup_height,
    }
}

fn init_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>, io::Error> {
    crossterm::execute!(io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
    crossterm::terminal::enable_raw_mode()?;
    let backend = CrosstermBackend::new(io::stdout());
    Terminal::new(backend)
}

fn restore_terminal() -> Result<(), io::Error> {
    crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()?;
    Ok(())
}
