//! `agentik-app` — host binary that wires the runtime, agent kinds, and
//! the TUI together.
//!
//! Architecture:
//! - Registers agent kinds into the runtime registry (see `kinds.rs`).
//! - Reads `data/settings.json` at startup, configures the shared pool.
//! - Runs a ratatui terminal loop hosting agentik-tui panels.
//! - Chat panel drives agent spawn-by-kind and message injection.
//! - Settings panel actions → configure/reconfigure the pool.

mod kinds;
mod settings_io;

use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use uuid::Uuid;

use agentik_runtime::{AgentEvent, AgentSpawnOpts, ModelConfig, ProcessEvent, ProcessManager, Runtime, RuntimeConfig};
use agentik_sdk::types::messages::ContentBlock;
use agentik_tui::{
    append_streaming_assistant, append_streaming_thinking, finalize_streaming,
    handle_non_delta_event, render_chat_input, render_chat_panel, ChatInputStatus, ChatMessage,
    ChatPanelState, RunningPhase, SettingsAction, SettingsKey, SettingsKeyCode,
    SettingsKeyModifiers, SettingsPanelState, handle_settings_key, render_settings_panel,
};
use agentik_tui::chat::theme::DefaultChatPanelTheme;
use agentik_tui::chat::input::DefaultChatInputTheme;

/// Path to the settings file (relative to CWD).
const SETTINGS_FILE: &str = "data/settings.json";

/// The application state held by the main loop.
struct App {
    manager: ProcessManager,
    settings: SettingsPanelState,
    chat: ChatPanelState<String>,
    /// Cached pool model count (updated on configure/reconfigure).
    pool_model_count: usize,
    /// Agent event stream receiver.
    event_rx: tokio::sync::broadcast::Receiver<ProcessEvent>,
    /// Currently active agent ID.
    current_agent_id: Option<Uuid>,
    /// Whether an agent is currently running.
    agent_running: bool,
    /// Spinner animation tick counter.
    spinner_tick: usize,
}

impl App {
    fn new(manager: ProcessManager, initial_config: &ModelConfig, pool_model_count: usize) -> Self {
        let event_rx = manager.events();
        App {
            manager,
            settings: SettingsPanelState::from_config(initial_config),
            chat: ChatPanelState::new("default".to_string()),
            pool_model_count,
            event_rx,
            current_agent_id: None,
            agent_running: false,
            spinner_tick: 0,
        }
    }

    /// Spawn an agent with an initial user message and start it.
    fn spawn_agent(&mut self, rt: &tokio::runtime::Handle, message: String) {
        let manager = self.manager.clone();
        let kind = "coder";

        self.chat
            .push_message(ChatMessage::User { text: message.clone() });
        self.agent_running = true;

        let opts = AgentSpawnOpts {
            initial_message: Some(vec![ContentBlock::Text { text: message }]),
            ..Default::default()
        };

        rt.spawn(async move {
            match manager.spawn_by_kind(kind, opts).await {
                Ok(agent_id) => {
                    tracing::info!("Agent spawned: {agent_id}");
                    if let Err(e) = manager.start(&agent_id) {
                        tracing::error!("Failed to start agent: {e}");
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to spawn agent: {e}");
                }
            }
        });
    }

    /// Poll for agent events (non-blocking). Returns true if agent finished.
    fn poll_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                ProcessEvent::Agent { agent_id, event } => {
                    if self.current_agent_id == Some(agent_id) {
                        self.apply_agent_event(&event);
                    }
                }
                ProcessEvent::StateChanged { agent_id, new_status } => {
                    if self.current_agent_id == Some(agent_id) {
                        self.agent_running = match new_status {
                            agentik_core::lifecycle::AgentLifecycleStatus::RUNNING => true,
                            _ => false,
                        };
                    }
                }
                ProcessEvent::ProcessExited { agent_id, .. } => {
                    if self.current_agent_id == Some(agent_id) {
                        self.agent_running = false;
                        finalize_streaming(self.chat.current_messages_mut());
                    }
                }
            }
        }
    }

    fn apply_agent_event(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::TextDelta(token) => {
                append_streaming_assistant(self.chat.current_messages_mut(), token);
            }
            AgentEvent::ThinkingDelta(token) => {
                append_streaming_thinking(self.chat.current_messages_mut(), token);
            }
            other => {
                handle_non_delta_event(self.chat.current_messages_mut(), other);
            }
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

    // ── Load settings ─────────────────────────────────
    let initial_config = settings_io::load_settings(SETTINGS_FILE);
    let rt = tokio::runtime::Runtime::new()?;

    // ── Create runtime ──────────────────────────────
    let runtime = rt.block_on(async {
        let config = RuntimeConfig::with_embedded_skill_server(vec![
            std::path::PathBuf::from("skills"),
        ])
        .with_model_config(initial_config.clone());

        let r = Runtime::new(config).await?;
        r.registry().register(kinds::coder_kind());
        Ok::<_, agentik_runtime::RuntimeError>(r)
    })?;

    if let Some(addr) = runtime.skill_server_addr() {
        tracing::info!("Skill server embedded at {addr}");
    }

    let manager = runtime.process_manager().clone();
    let pool_model_count = rt.block_on(manager.pool_model_names()).len();

    // ── Build app ──────────────────────────────────────
    let mut app = App::new(manager, &initial_config, pool_model_count);

    // ── Terminal setup ────────────────────────────────
    let mut terminal = init_terminal()?;

    // ── Event loop ────────────────────────────────────
    let rt_handle = rt.handle().clone();
    loop {
        // Poll agent events (non-blocking drain).
        app.poll_events();
        app.spinner_tick = app.spinner_tick.wrapping_add(1);

        terminal.draw(|frame| {
            render_app(frame, &mut app);
        })?;

        if crossterm::event::poll(std::time::Duration::from_millis(50))? {
            match crossterm::event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if handle_key(&mut app, key, &rt_handle) {
                        break;
                    }
                }
                Event::Paste(s) => {
                    if app.chat.input_active() {
                        app.chat.push_paste(&s);
                    }
                }
                _ => {}
            }
        }
    }

    // ── Shutdown ──────────────────────────────────────
    restore_terminal()?;
    let results = rt.block_on(runtime.shutdown());
    tracing::info!("Shutdown complete — {} agents exited", results.len());
    Ok(())
}

/// Handle a key event.  Returns `true` if the app should quit.
fn handle_key(
    app: &mut App,
    key: event::KeyEvent,
    rt: &tokio::runtime::Handle,
) -> bool {
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

    // While agent is running, only accept quit.
    if app.agent_running {
        if key.code == KeyCode::Char('q') {
            return true;
        }
        return false;
    }

    // Chat input mode.
    if app.chat.input_active() {
        match key.code {
            KeyCode::Enter => {
                let text = app.chat.take_input_text();
                app.chat.set_input_active(false);
                if !text.is_empty() {
                    app.spawn_agent(rt, text);
                }
            }
            KeyCode::Esc => {
                app.chat.set_input_active(false);
                app.chat.clear_input_text();
            }
            KeyCode::Char(c) => {
                app.chat.input_text_mut().push(c);
            }
            KeyCode::Backspace => {
                app.chat.input_text_mut().pop();
            }
            _ => {}
        }
        return false;
    }

    // Global keys (idle, input not active).
    match key.code {
        KeyCode::Char('q') => return true,
        KeyCode::Char('s') => {
            app.settings.toggle_open();
        }
        KeyCode::Enter | KeyCode::Char('/') => {
            app.chat.set_input_active(true);
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
        _ => SettingsKeyCode::Char('\x00'),
    }
}

fn render_app(frame: &mut ratatui::Frame, app: &mut App) {
    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Min(1), // chat area
            ratatui::layout::Constraint::Length(1), // input / status row
            ratatui::layout::Constraint::Length(1), // bottom status bar
        ])
        .split(frame.area());

    // Chat area.
    let chat_theme = DefaultChatPanelTheme;
    render_chat_panel(frame, &mut app.chat, &chat_theme, chunks[0]);

    // Input / status row.
    let input_status = if app.agent_running {
        ChatInputStatus::Running {
            phase: RunningPhase::Streaming,
            tokens: None,
        }
    } else if app.chat.input_active() {
        ChatInputStatus::InputActive
    } else if app.pool_model_count == 0 {
        ChatInputStatus::EmptyProviders
    } else {
        ChatInputStatus::Idle
    };
    let input_theme = DefaultChatInputTheme;
    render_chat_input(
        frame,
        &mut app.chat,
        &input_status,
        "coder",
        app.spinner_tick,
        &input_theme,
        chunks[1],
        true,
    );

    // Bottom status bar.
    let status_text = format!(
        " {} | {} models | [s] settings | [q] quit",
        if app.settings.is_open() {
            "⚙ Settings"
        } else if app.agent_running {
            "▶ Running"
        } else {
            "● Ready"
        },
        app.pool_model_count,
    );
    let status = ratatui::widgets::Paragraph::new(status_text)
        .style(ratatui::style::Style::default().fg(ratatui::style::Color::Gray));
    frame.render_widget(status, chunks[2]);

    // Settings overlay.
    if app.settings.is_open() {
        let settings_lines = render_settings_panel(&app.settings, &agentik_tui::DefaultSettingsTheme);
        let settings_area = centered_rect(60, 20, frame.area());
        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(
                ratatui::style::Style::default().fg(ratatui::style::Color::Cyan),
            );
        let settings_widget = ratatui::widgets::Paragraph::new(settings_lines).block(block);
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
