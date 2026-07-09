use std::time::Duration;

use agentik_sdk::AuthMethod;
use agentik_sdk::model::model_pool::ModelPoolConfig;
use agentik_sdk::model::{ModelInfo, ProviderConfig};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    prelude::{Terminal, Widget},
};
use std::io::{stdout, Stdout, Write};
use ratatui_comfy_tabs::{TabBarAlign, TabDirection, TabNav, TabNavState};
use rusqlite::Connection;
use uuid::Uuid;

use crate::state::{self, AgentStatus, AppState, InputMode, MainTabState};
use crate::widgets::agent_tab_widget::AgentTabWidget;
use agentik_runtime::AgentRuntime;

const POLL_TIMEOUT: Duration = Duration::from_millis(16);

pub struct App {
    state: AppState,
    tab_state: TabNavState,
    agent_runtime: AgentRuntime,
    /// Kept alive to drive the agent's background event loop task.
    _runtime: tokio::runtime::Runtime,
    conn: Connection,
    /// True when state has changed and a re-render is needed.
    dirty: bool,
    /// Set to break the main event loop so `ratatui::run()` can call `restore()`.
    should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        let conn = Connection::open("phloem.db").expect("failed to open phloem.db");

        conn.pragma_update(None, "foreign_keys", "ON")
            .expect("failed to enable foreign_keys");

        Self::init_database(&conn).expect("failed to initialize database schema");

        let model_pool = Self::build_model_pool(&conn).expect("failed to build model pool");

        let runtime = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        let agent_runtime =
            AgentRuntime::new(&runtime, model_pool).expect("failed to create agent runtime");

        let mut state = AppState::default();
        Self::reload_config(&mut state.config_tab_state, &conn);

        Self {
            state,
            tab_state: TabNavState::new(MainTabState::default().index()),
            agent_runtime,
            _runtime: runtime,
            conn,
            dirty: true, // render the initial frame
            should_quit: false,
        }
    }

    /// Re-read providers and models from the database, clamping selections.
    fn reload_config(cs: &mut crate::state::ConfigTabState, conn: &Connection) {
        cs.providers = crate::config_db::ProviderRow::all(conn).unwrap_or_default();
        cs.models = crate::config_db::ModelRow::all(conn).unwrap_or_default();
        if !cs.providers.is_empty() && cs.selected_provider >= cs.providers.len() {
            cs.selected_provider = cs.providers.len() - 1;
        } else if cs.providers.is_empty() {
            cs.selected_provider = 0;
        }
        if !cs.models.is_empty() && cs.selected_model >= cs.models.len() {
            cs.selected_model = cs.models.len() - 1;
        } else if cs.models.is_empty() {
            cs.selected_model = 0;
        }
    }

    fn build_model_pool(
        conn: &Connection,
    ) -> Result<agentik_core::model::model_pool::ModelPool, Box<dyn std::error::Error>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, base_url, api_key, auth_method, provider_type FROM providers",
        )?;
        let providers: Vec<ProviderConfig> = stmt
            .query_map([], |row| {
                // The schema uses an INTEGER autoincrement PK, but ProviderConfig
                // keys providers by Uuid. Map the integer to a deterministic Uuid
                // so the same id always yields the same Uuid (the pool joins
                // models to providers by Uuid equality).
                let id: i64 = row.get(0)?;
                let auth_method: String = row.get(4)?;
                let auth: AuthMethod = auth_method.try_into().map_err(
                    |e: agentik_sdk::types::errors::AnthropicError| {
                        rusqlite::Error::ToSqlConversionFailure(e.into())
                    },
                )?;
                Ok(ProviderConfig {
                    id: Uuid::from_u128(id as u128),
                    name: row.get(1)?,
                    base_url: row.get(2)?,
                    api_key: row.get(3)?,
                    provider_type: row.get(5)?,
                    auth_method: auth,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut stmt = conn.prepare(
            "SELECT model_name, provider_id, context_length, max_output_tokens,
                    vision_ability, supports_function_calling, supports_streaming,
                    supports_thinking, input_token_price, output_token_price
             FROM models",
        )?;
        let models: Vec<ModelInfo> = stmt
            .query_map([], |row| {
                let provider_id: i64 = row.get(1)?;
                Ok(ModelInfo {
                    model_name: row.get(0)?,
                    provider_id: Uuid::from_u128(provider_id as u128),
                    context_length: row.get::<_, i64>(2)? as u64,
                    max_output_tokens: row.get::<_, i64>(3)? as u64,
                    vision_ability: row.get::<_, i32>(4)? != 0,
                    supports_function_calling: row.get::<_, i32>(5)? != 0,
                    supports_streaming: row.get::<_, i32>(6)? != 0,
                    supports_thinking: row.get::<_, i32>(7)? != 0,
                    input_token_price: row.get(8)?,
                    output_token_price: row.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let config = ModelPoolConfig { providers, models };
        agentik_core::model::model_pool::ModelPool::from_config(config).map_err(Into::into)
    }

    fn init_database(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS providers (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                name            TEXT    NOT NULL UNIQUE,
                provider_type   TEXT    NOT NULL,
                base_url        TEXT    NOT NULL,
                api_key         TEXT    NOT NULL,
                auth_method     TEXT    NOT NULL DEFAULT 'Anthropic'
            )",
            (),
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS models (
                id                          INTEGER PRIMARY KEY AUTOINCREMENT,
                model_name                  TEXT    NOT NULL UNIQUE,
                provider_id                 INTEGER NOT NULL,
                context_length              INTEGER NOT NULL DEFAULT 0,
                max_output_tokens           INTEGER NOT NULL DEFAULT 0,
                vision_ability              INTEGER NOT NULL DEFAULT 0,
                supports_function_calling   INTEGER NOT NULL DEFAULT 1,
                supports_streaming          INTEGER NOT NULL DEFAULT 1,
                supports_thinking           INTEGER NOT NULL DEFAULT 0,
                input_token_price           REAL    NOT NULL DEFAULT 0,
                output_token_price          REAL    NOT NULL DEFAULT 0,
                FOREIGN KEY (provider_id) REFERENCES providers(id) ON DELETE CASCADE
            )",
            (),
        )?;

        Ok(())
    }

    pub fn start(&mut self) -> color_eyre::Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(stdout(), crossterm::terminal::EnterAlternateScreen, EnableMouseCapture)?;
        stdout().flush()?;

        let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

        let result = self.app(&mut terminal);

        // Restore terminal on exit (whether normal or error).
        crossterm::execute!(stdout(), DisableMouseCapture, crossterm::terminal::LeaveAlternateScreen)?;
        crossterm::terminal::disable_raw_mode()?;

        result?;
        Ok(())
    }

    fn app(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> std::io::Result<()> {
        loop {
            if self.should_quit {
                break Ok(());
            }
            // ── Event handling phase ──
            // Drain all queued input events before rendering.
            if event::poll(POLL_TIMEOUT)? {
                let mut scroll_delta: i32 = 0;
                loop {
                    let event = event::read()?;
                    let scroll = self.handle_event(&event);
                    scroll_delta += scroll;
                    // Check for more events without blocking.
                    if !event::poll(Duration::ZERO)? {
                        break;
                    }
                }
                // Apply batched scroll delta once.
                if scroll_delta != 0 {
                    self.apply_scroll_delta(scroll_delta);
                }
                self.dirty = true;
            } else {
                // Timeout: drain agent streaming events.
                let had_events = self.drain_agent_events();
                if had_events {
                    self.dirty = true;
                }
            }

            // ── Render phase (only when state changed) ──
            if self.dirty {
                terminal.draw(|f| self.render(f))?;
                self.dirty = false;
            }
        }
    }

    fn drain_agent_events(&mut self) -> bool {
        let mut had_events = false;
        while let Some(event) = self.agent_runtime.poll_event() {
            state::apply_event(&mut self.state.agent_tab_state, event);
            had_events = true;
        }
        had_events
    }

    /// Handle a single event. Returns a scroll delta to be accumulated.
    fn handle_event(&mut self, event: &Event) -> i32 {
        match event {
            Event::Key(key) if key.kind == crossterm::event::KeyEventKind::Press => {
                self.handle_key(key);
                0
            }
            Event::Resize(_, _) | Event::FocusGained | Event::FocusLost => 0,
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Paste(s) => {
                // Only insert paste into the input area when in input mode and agent is idle.
                if matches!(self.state.main_tab_state, MainTabState::AgentTab) {
                    let ts = &mut self.state.agent_tab_state;
                    if ts.input_mode == InputMode::Input && ts.status == state::AgentStatus::Idle {
                        ts.input.insert_str(s);
                    }
                }
                0
            }
            _ => 0,
        }
    }

    /// Handle mouse events: scroll wheel scrolls the chat in Agent tab.
    /// Returns the scroll delta to be batched with other scroll events.
    fn handle_mouse(&mut self, mouse: &MouseEvent) -> i32 {
        if !matches!(self.state.main_tab_state, MainTabState::AgentTab) {
            return 0;
        }

        let lines_per_tick: i32 = 3;

        match mouse.kind {
            MouseEventKind::ScrollDown => {
                let ts = &mut self.state.agent_tab_state;
                ts.auto_scroll = false;
                lines_per_tick
            }
            MouseEventKind::ScrollUp => {
                let ts = &mut self.state.agent_tab_state;
                ts.auto_scroll = false;
                -lines_per_tick
            }
            _ => 0,
        }
    }

    /// Apply a batched scroll delta to the agent tab.
    fn apply_scroll_delta(&mut self, delta: i32) {
        if !matches!(self.state.main_tab_state, MainTabState::AgentTab) {
            return;
        }
        let ts = &mut self.state.agent_tab_state;
        if delta > 0 {
            ts.scroll_offset = ts.scroll_offset.saturating_add(delta as usize);
        } else {
            ts.scroll_offset = ts.scroll_offset.saturating_sub((-delta) as usize);
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) {
        // Ctrl+C: cancel running agent first, then quit on second press
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if !matches!(self.state.agent_tab_state.status, AgentStatus::Idle) {
                self.agent_runtime.cancel();
                return;
            }
            self.should_quit = true;
            return;
        }

        // Tab switching (global)
        match key.code {
            KeyCode::Char(']') => {
                self.tab_state
                    .select_direction_wrapping(TabDirection::Next, state::TABS.len());
                self.sync_tab_state();
                return;
            }
            KeyCode::Char('[') => {
                self.tab_state
                    .select_direction_wrapping(TabDirection::Previous, state::TABS.len());
                self.sync_tab_state();
                return;
            }
            _ => {}
        }

        // Delegate to active tab
        match self.state.main_tab_state {
            MainTabState::AgentTab => {
                self.handle_agent_key(key);
            }
            MainTabState::ConfigTab => {
                self.handle_config_key(key);
            }
        }
    }

    fn handle_agent_key(&mut self, key: &KeyEvent) {
        let ts = &mut self.state.agent_tab_state;

        match ts.input_mode {
            InputMode::Browse => self.handle_browse_key(key),
            InputMode::Input => self.handle_input_key(key),
        }
    }

    /// Key handling in browse mode: j/k scroll, Enter enters input mode.
    fn handle_browse_key(&mut self, key: &KeyEvent) {
        let ts = &mut self.state.agent_tab_state;

        match key.code {
            // j / Down: scroll down (show later content)
            KeyCode::Char('j') | KeyCode::Down => {
                ts.scroll_offset = ts.scroll_offset.saturating_add(1);
                ts.auto_scroll = false;
            }
            // k / Up: scroll up (show earlier content)
            KeyCode::Char('k') | KeyCode::Up => {
                ts.scroll_offset = ts.scroll_offset.saturating_sub(1);
                ts.auto_scroll = false;
            }
            // G (Shift+g): jump to bottom, re-enable auto-scroll
            KeyCode::Char('G') => {
                ts.auto_scroll = true;
            }
            // g: jump to top
            KeyCode::Char('g') => {
                ts.scroll_offset = 0;
                ts.auto_scroll = false;
            }
            // Enter: switch to input mode
            KeyCode::Enter => {
                ts.input_mode = InputMode::Input;
            }
            _ => {}
        }
    }

    /// Key handling in input mode: typing goes to input, Enter sends, Esc exits.
    fn handle_input_key(&mut self, key: &KeyEvent) {
        use crate::widgets::input_area::{history_clear_recall, history_down, history_up};
        let ts = &mut self.state.agent_tab_state;

        match key.code {
            // Esc: exit input mode, clear input AND any in-flight recall
            KeyCode::Esc => {
                ts.input.clear();
                history_clear_recall(&mut ts.input_draft, &mut ts.input_recall);
                ts.input_mode = InputMode::Browse;
            }
            // Enter: send message (if valid), return to browse mode
            KeyCode::Enter => {
                if ts.can_send() {
                    let text = ts.take_input();
                    // Push to in-memory history before clearing the
                    // recall state — `take_input()` already cleared the
                    // textbox, but recall metadata is independent.
                    crate::widgets::input_area::history_push(
                        &mut ts.input_history,
                        text.clone(),
                        ts.input_history_capacity,
                    );
                    history_clear_recall(&mut ts.input_draft, &mut ts.input_recall);
                    ts.push_user_message(text.clone());
                    self.agent_runtime.send_message(text);
                    ts.scroll_to_bottom();
                }
                ts.input_mode = InputMode::Browse;
            }
            // Up/Down: recall history (Up) / advance towards draft (Down)
            KeyCode::Up => {
                if ts.status == state::AgentStatus::Idle {
                    let _ = history_up(
                        &mut ts.input,
                        &ts.input_history,
                        &mut ts.input_draft,
                        &mut ts.input_recall,
                    );
                }
            }
            KeyCode::Down => {
                if ts.status == state::AgentStatus::Idle {
                    let _ = history_down(
                        &mut ts.input,
                        &ts.input_history,
                        &mut ts.input_draft,
                        &mut ts.input_recall,
                    );
                }
            }
            // Any other key: collapse in-progress recall so subsequent
            // edits are treated as user-driven (not as a recalled entry
            // we'd accidentally re-push when sent).
            _ => {
                if ts.status == state::AgentStatus::Idle {
                    if ts.input_recall.is_some() {
                        history_clear_recall(&mut ts.input_draft, &mut ts.input_recall);
                    }
                    ts.input.handle_key(*key);
                }
            }
        }
    }

    fn sync_tab_state(&mut self) {
        self.state.main_tab_state = MainTabState::from_index(self.tab_state.selected);
    }

    fn render(&mut self, frame: &mut Frame) {
        let areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // TabBar
                Constraint::Min(5),    // Content (Widget handles its own layout)
            ])
            .split(frame.area());

        // ── TabBar ──
        let tabs = TabNav::new(state::TABS, self.tab_state.selected)
            .tab_bar_align(TabBarAlign::Center)
            .highlight_style(ratatui::style::Style::default().yellow());
        frame.render_stateful_widget(tabs, areas[0], &mut self.tab_state);

        match self.state.main_tab_state {
            MainTabState::AgentTab => {
                let widget = AgentTabWidget {
                    state: &mut self.state.agent_tab_state,
                };
                frame.render_widget(widget, areas[1]);
            }
            MainTabState::ConfigTab => {
                crate::widgets::config_widget::render_config(
                    &self.state.config_tab_state,
                    areas[1],
                    frame.buffer_mut(),
                );
            }
        }
    }

    // ── Config tab ──────────────────────────────────────

    /// Config tab key handling. Esc/Enter-in-form/'d'-delete need database
    /// access, so they are handled here; everything else is delegated to the
    /// widget's key handler.
    fn handle_config_key(&mut self, key: &KeyEvent) {
        let code = key.code;
        let cs = &mut self.state.config_tab_state;

        // Esc closes any open form.
        if code == KeyCode::Esc && !matches!(cs.mode, state::ConfigMode::Browsing) {
            cs.mode = state::ConfigMode::Browsing;
            cs.message.clear();
            return;
        }

        // Enter inside a form validates and saves.
        if code == KeyCode::Enter {
            if matches!(cs.mode, state::ConfigMode::EditProvider(_)) {
                self.save_provider_form();
                return;
            }
            if matches!(cs.mode, state::ConfigMode::EditModel(_)) {
                self.save_model_form();
                return;
            }
        }

        // 'd' in browsing deletes the selected row.
        if code == KeyCode::Char('d') && matches!(cs.mode, state::ConfigMode::Browsing) {
            self.delete_selected_config();
            return;
        }

        // Everything else (navigation, opening forms, typing) goes to the widget.
        crate::widgets::config_widget::handle_config_key(&mut self.state.config_tab_state, *key);
    }

    /// Take the open provider form out of state, validate, persist, reload.
    fn save_provider_form(&mut self) {
        let conn = &self.conn;
        let cs = &mut self.state.config_tab_state;
        let form = match std::mem::replace(&mut cs.mode, state::ConfigMode::Browsing) {
            state::ConfigMode::EditProvider(f) => f,
            _ => return,
        };

        match form.collect() {
            Err(e) => {
                cs.message = e;
                cs.mode = state::ConfigMode::EditProvider(form);
            }
            Ok(input) => {
                let id = form.id;
                let res = match id {
                    Some(id) => crate::config_db::ProviderRow::update(conn, id, &input),
                    None => crate::config_db::ProviderRow::insert(conn, &input).map(|_| ()),
                };
                match res {
                    Ok(()) => {
                        Self::reload_config(cs, conn);
                        cs.message = match id {
                            Some(_) => "provider updated".to_string(),
                            None => "provider added".to_string(),
                        };
                    }
                    Err(e) => {
                        cs.message = format!("db error: {e}");
                        cs.mode = state::ConfigMode::EditProvider(form);
                    }
                }
            }
        }
    }

    /// Take the open model form out of state, validate, persist, reload.
    fn save_model_form(&mut self) {
        let conn = &self.conn;
        let cs = &mut self.state.config_tab_state;
        let (form, providers) = match std::mem::replace(&mut cs.mode, state::ConfigMode::Browsing) {
            state::ConfigMode::EditModel(f) => (f, cs.providers.clone()),
            _ => return,
        };

        match form.collect(&providers) {
            Err(e) => {
                cs.message = e;
                cs.mode = state::ConfigMode::EditModel(form);
            }
            Ok(input) => {
                let id = form.id;
                let res = match id {
                    Some(id) => crate::config_db::ModelRow::update(conn, id, &input),
                    None => crate::config_db::ModelRow::insert(conn, &input).map(|_| ()),
                };
                match res {
                    Ok(()) => {
                        Self::reload_config(cs, conn);
                        cs.message = match id {
                            Some(_) => "model updated".to_string(),
                            None => "model added".to_string(),
                        };
                    }
                    Err(e) => {
                        cs.message = format!("db error: {e}");
                        cs.mode = state::ConfigMode::EditModel(form);
                    }
                }
            }
        }
    }

    fn delete_selected_config(&mut self) {
        let conn = &self.conn;
        let cs = &mut self.state.config_tab_state;
        match cs.pane {
            state::ConfigPane::Providers => {
                let Some(row) = cs.selected_provider_row().cloned() else {
                    return;
                };
                match crate::config_db::ProviderRow::delete(conn, row.id) {
                    Ok(()) => {
                        Self::reload_config(cs, conn);
                        cs.message = format!("deleted provider '{}'", row.name);
                    }
                    Err(e) => cs.message = format!("db error: {e}"),
                }
            }
            state::ConfigPane::Models => {
                let Some(row) = cs.selected_model_row().cloned() else {
                    return;
                };
                match crate::config_db::ModelRow::delete(conn, row.id) {
                    Ok(()) => {
                        Self::reload_config(cs, conn);
                        cs.message = format!("deleted model '{}'", row.model_name);
                    }
                    Err(e) => cs.message = format!("db error: {e}"),
                }
            }
        }
    }
}
