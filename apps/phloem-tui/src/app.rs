use agentik_core::model::model_pool::ModelPool;
use agentik_sdk::AuthMethod;
use agentik_sdk::model::model_pool::ModelPoolConfig;
use agentik_sdk::model::{ModelInfo, ProviderConfig};
use crossterm::event::Event;
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    symbols,
    widgets::{Block, Tabs},
};
use rusqlite::Connection;
use std::sync::Arc;
use uuid::Uuid;

use crate::state::{self, AppState, MainTabState};

pub struct App {
    conn: Arc<Connection>,
    model_pool: ModelPool,
    state: AppState,
}

impl App {
    pub fn new() -> Self {
        let conn = Arc::new(Connection::open("phloem.db").expect("failed to open phloem.db"));

        // Enable FK enforcement (off by default in SQLite) so the
        // models→providers ON DELETE CASCADE rule actually fires.
        conn.pragma_update(None, "foreign_keys", "ON")
            .expect("failed to enable foreign_keys");

        Self::init_database(&conn).expect("failed to initialize database schema");

        let model_pool = Self::build_model_pool(&conn).expect("failed to build model pool");

        Self {
            conn,
            model_pool,
            state: AppState::default(),
        }
    }

    fn build_model_pool(conn: &Connection) -> Result<ModelPool, Box<dyn std::error::Error>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, base_url, api_key, auth_method, provider_type FROM providers",
        )?;
        let providers: Vec<ProviderConfig> = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let auth_method: String = row.get(4)?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?;
                let auth: AuthMethod = auth_method.try_into().map_err(
                    |e: agentik_sdk::types::errors::AnthropicError| {
                        rusqlite::Error::ToSqlConversionFailure(e.into())
                    },
                )?;
                Ok(ProviderConfig {
                    id: uuid,
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
                Ok(ModelInfo {
                    model_name: row.get(0)?,
                    provider_id: Uuid::parse_str(&row.get::<_, String>(1)?)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?,
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
        ModelPool::from_config(config).map_err(Into::into)
    }

    fn init_database(conn: &Connection) -> rusqlite::Result<()> {
        // Provider table — the "master" side. One row per endpoint:
        // a provider type combined with a concrete base URL + credentials.
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

        // Model table — references a provider by id. Holds only capabilities;
        // connection config lives on the provider row.
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
        color_eyre::install()?;
        ratatui::run(|f| self.app(f))?;
        Ok(())
    }
    fn app(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        loop {
            terminal.draw(|f| self.render(f))?;
            let event = crossterm::event::read()?;

            match event {
                Event::FocusGained => (),
                Event::FocusLost => (),
                Event::Key(key_event) => {
                    if key_event.is_press() {
                        match key_event.code {
                            crossterm::event::KeyCode::Backspace => (),
                            crossterm::event::KeyCode::Enter => (),
                            crossterm::event::KeyCode::Left => (),
                            crossterm::event::KeyCode::Right => (),
                            crossterm::event::KeyCode::Up => (),
                            crossterm::event::KeyCode::Down => (),
                            crossterm::event::KeyCode::Home => (),
                            crossterm::event::KeyCode::End => (),
                            crossterm::event::KeyCode::PageUp => (),
                            crossterm::event::KeyCode::PageDown => (),
                            crossterm::event::KeyCode::Tab => {
                                self.state.main_tab_state = self.state.main_tab_state.next();
                            }
                            crossterm::event::KeyCode::BackTab => {
                                self.state.main_tab_state = self.state.main_tab_state.prev();
                            }
                            crossterm::event::KeyCode::Delete => (),
                            crossterm::event::KeyCode::Insert => (),
                            crossterm::event::KeyCode::F(_) => (),
                            crossterm::event::KeyCode::Char(key) => match key {
                                'q' => break Ok(()), // Exit TUI
                                ']' => {
                                    self.state.main_tab_state = self.state.main_tab_state.next();
                                }
                                '[' => {
                                    self.state.main_tab_state = self.state.main_tab_state.prev();
                                }
                                _ => (),
                            },
                            crossterm::event::KeyCode::Null => (),
                            crossterm::event::KeyCode::Esc => (),
                            crossterm::event::KeyCode::CapsLock => (),
                            crossterm::event::KeyCode::ScrollLock => (),
                            crossterm::event::KeyCode::NumLock => (),
                            crossterm::event::KeyCode::PrintScreen => (),
                            crossterm::event::KeyCode::Pause => (),
                            crossterm::event::KeyCode::Menu => (),
                            crossterm::event::KeyCode::KeypadBegin => (),
                            crossterm::event::KeyCode::Media(_media_key_code) => (),
                            crossterm::event::KeyCode::Modifier(_modifier_key_code) => (),
                        }
                    }
                }
                Event::Mouse(_mouse_event) => (),
                Event::Paste(_) => (),
                Event::Resize(_, _) => (),
            }
        }
    }

    fn render(&self, frame: &mut Frame) {
        let first_layout = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints(vec![Constraint::Percentage(10), Constraint::Percentage(90)])
            .split(frame.area());

        let tabs = Tabs::new(state::TABS.to_vec())
            .block(Block::bordered().title("test_tab"))
            .highlight_style(Style::default().yellow())
            .select(self.state.main_tab_state.index())
            .divider(symbols::DOT)
            .padding("->", "<-");

        // Render main tab area
        frame.render_widget(tabs, first_layout[0]);

        match self.state.main_tab_state {
            state::MainTabState::AgentTab => {
                frame.render_widget("hello", first_layout[1]);
            }
            state::MainTabState::ConfigTab => {
                frame.render_widget("hi", first_layout[1]);
            }
        };
        // frame.render_widget("hello world", frame.area());
    }
}
