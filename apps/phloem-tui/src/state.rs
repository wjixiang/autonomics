use crate::config_db::{ModelInput, ProviderInput, ProviderRow};
use crate::widgets::input_area::{InputArea, InputState};
use agentik_sdk::types::AgentEvent;

pub const TABS: &[&str] = &["Agent", "Config"];

pub enum MainTabState {
    AgentTab,
    ConfigTab,
}

impl MainTabState {
    pub const fn index(&self) -> usize {
        match self {
            Self::AgentTab => 0,
            Self::ConfigTab => 1,
        }
    }

    pub fn from_index(index: usize) -> Self {
        match index {
            0 => Self::AgentTab,
            _ => Self::ConfigTab,
        }
    }
}

impl Default for MainTabState {
    fn default() -> Self {
        Self::AgentTab
    }
}

// ── Chat data model ─────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ChatLine {
    User(String),
    Assistant(String),
    Thinking(String),
    ToolCall { name: String, input: String },
    ToolResult { ok: bool, content: String },
    Error(String),
    Separator,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentStatus {
    Idle,
    Requesting,
    Streaming,
    Error,
}

/// Whether the agent tab is in browse (j/k scroll) or input (typing) mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Browse,
    Input,
}

/// Mutable state for the Agent tab.
pub struct AgentTabState {
    pub messages: Vec<ChatLine>,
    pub scroll_offset: usize,
    pub status: AgentStatus,
    pub input: InputArea,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub input_mode: InputMode,
    /// When true, `clamp_scroll` forces offset to the bottom each frame.
    pub auto_scroll: bool,
    /// Cached total rendered line count (updated every frame).
    pub content_line_count: usize,
}

impl Default for AgentTabState {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            scroll_offset: 0,
            status: AgentStatus::Idle,
            input: InputArea::new(),
            input_tokens: 0,
            output_tokens: 0,
            input_mode: InputMode::Browse,
            auto_scroll: true,
            content_line_count: 0,
        }
    }
}

impl AgentTabState {
    /// Returns true when the user can type and send messages.
    pub fn can_send(&self) -> bool {
        self.status == AgentStatus::Idle && !self.input.is_empty()
    }

    /// Take the current input text and clear the input field.
    pub fn take_input(&mut self) -> String {
        let text = self.input.value();
        self.input.clear();
        text
    }

    /// Push a user message and a separator after the previous assistant response.
    pub fn push_user_message(&mut self, text: String) {
        self.messages.push(ChatLine::User(text));
    }

    /// Re-enable auto-scroll so the next `clamp_scroll` call pins to the bottom.
    pub fn scroll_to_bottom(&mut self) {
        self.auto_scroll = true;
    }

    /// Clamp scroll_offset based on content and viewport.
    /// When `auto_scroll` is true, forces offset to the maximum (bottom).
    pub fn clamp_scroll(&mut self, viewport_height: u16) {
        let total = self.content_line_count;
        let max_offset = total.saturating_sub(viewport_height as usize);
        if self.auto_scroll {
            self.scroll_offset = max_offset;
        } else {
            self.scroll_offset = self.scroll_offset.min(max_offset);
        }
    }

    /// Returns true when the viewport is showing the bottom of the content.
    pub fn is_at_bottom(&self, viewport_height: u16) -> bool {
        let max_offset = self.content_line_count.saturating_sub(viewport_height as usize);
        self.scroll_offset >= max_offset
    }
}

// ── Event → State mapping ──────────────────────────────

/// Apply an `AgentEvent` to `AgentTabState`, mutating the conversation view.
pub fn apply_event(state: &mut AgentTabState, event: AgentEvent) {
    match event {
        AgentEvent::Requesting => {
            state.status = AgentStatus::Requesting;
        }
        AgentEvent::TextDelta(text) => {
            state.status = AgentStatus::Streaming;
            // Append to last Assistant line, or create a new one
            let last_is_assistant = state
                .messages
                .last()
                .is_some_and(|l| matches!(l, ChatLine::Assistant(_)));
            if last_is_assistant {
                if let Some(ChatLine::Assistant(s)) = state.messages.last_mut() {
                    s.push_str(&text);
                }
            } else {
                state.messages.push(ChatLine::Assistant(text));
            }
            state.scroll_to_bottom();
        }
        AgentEvent::ThinkingDelta(text) => {
            let last_is_thinking = state
                .messages
                .last()
                .is_some_and(|l| matches!(l, ChatLine::Thinking(_)));
            if last_is_thinking {
                if let Some(ChatLine::Thinking(s)) = state.messages.last_mut() {
                    s.push_str(&text);
                }
            } else {
                state.messages.push(ChatLine::Thinking(text));
            }
            state.scroll_to_bottom();
        }
        AgentEvent::UsageUpdate {
            input_tokens,
            output_tokens,
        } => {
            if let Some(t) = input_tokens {
                state.input_tokens = t;
            }
            state.output_tokens = output_tokens;
        }
        AgentEvent::LlmResponse(_text) => {
            // The full aggregated response — we already have it via TextDelta.
            // No-op here; the assistant line was built incrementally.
        }
        AgentEvent::Thinking(_text) => {
            // Aggregated thinking block — already streamed via ThinkingDelta.
        }
        AgentEvent::ToolCall { name, input } => {
            state.messages.push(ChatLine::ToolCall {
                name,
                input: input.to_string(),
            });
            state.scroll_to_bottom();
        }
        AgentEvent::ToolResult { ok, content } => {
            state.messages.push(ChatLine::ToolResult { ok, content });
            state.scroll_to_bottom();
        }
        AgentEvent::Done => {
            state.status = AgentStatus::Idle;
            state.messages.push(ChatLine::Separator);
            state.scroll_to_bottom();
        }
        AgentEvent::Error(msg) => {
            state.status = AgentStatus::Error;
            state.messages.push(ChatLine::Error(msg));
            state.scroll_to_bottom();
        }
        // Streaming protocol events — not surfaced directly to the chat view
        AgentEvent::StreamStart { .. }
        | AgentEvent::ContentBlockStart { .. }
        | AgentEvent::ContentBlockStop { .. }
        | AgentEvent::StreamDelta { .. } => {}
    }
}

// ── Config tab data model ──────────────────────────────

use crate::config_db::ModelRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigPane {
    Providers,
    Models,
}

/// Which form (if any) is open in the Config tab.
pub enum ConfigMode {
    Browsing,
    EditProvider(ProviderForm),
    EditModel(ModelForm),
}

/// Editable provider record. `id == None` means a new row.
pub struct ProviderForm {
    pub id: Option<i64>,
    pub name: InputState,
    pub provider_type: InputState,
    pub base_url: InputState,
    pub api_key: InputState,
    pub auth_method: InputState,
    pub focus: usize,
}

impl ProviderForm {
    pub const FIELDS: &'static [&'static str] = &["Name", "Type", "Base URL", "API Key", "Auth"];

    pub fn new(input: ProviderInput, id: Option<i64>) -> Self {
        let mk = |v: String| {
            let mut s = InputState::new();
            for ch in v.chars() {
                s.insert(ch);
            }
            s
        };
        Self {
            id,
            name: mk(input.name),
            provider_type: mk(input.provider_type),
            base_url: mk(input.base_url),
            api_key: mk(input.api_key),
            auth_method: mk(input.auth_method),
            focus: 0,
        }
    }

    pub fn fields_mut(&mut self) -> [&mut InputState; 5] {
        [
            &mut self.name,
            &mut self.provider_type,
            &mut self.base_url,
            &mut self.api_key,
            &mut self.auth_method,
        ]
    }

    pub fn fields(&self) -> [&InputState; 5] {
        [
            &self.name,
            &self.provider_type,
            &self.base_url,
            &self.api_key,
            &self.auth_method,
        ]
    }

    /// Validate and collect the form into a `ProviderInput`.
    pub fn collect(&self) -> Result<ProviderInput, String> {
        let name = self.name.value().trim().to_string();
        if name.is_empty() {
            return Err("name must not be empty".into());
        }
        Ok(ProviderInput {
            name,
            provider_type: self.provider_type.value().trim().to_string(),
            base_url: self.base_url.value().trim().to_string(),
            api_key: self.api_key.value().trim().to_string(),
            auth_method: self.auth_method.value().trim().to_string(),
        })
    }
}

/// Editable model record. `id == None` means a new row.
pub struct ModelForm {
    pub id: Option<i64>,
    pub model_name: InputState,
    pub provider_index: usize,
    pub context_length: InputState,
    pub max_output_tokens: InputState,
    pub input_token_price: InputState,
    pub output_token_price: InputState,
    pub supports_function_calling: bool,
    pub supports_streaming: bool,
    pub supports_thinking: bool,
    pub vision_ability: bool,
    pub focus: usize,
}

impl ModelForm {
    /// Indices of text-entry fields in the form (used by the render/keys).
    pub const TEXT_FIELD_COUNT: usize = 5;

    pub fn new(input: ModelInput, id: Option<i64>, providers: &[ProviderRow]) -> Self {
        let mk = |v: String| {
            let mut s = InputState::new();
            for ch in v.chars() {
                s.insert(ch);
            }
            s
        };
        let provider_index = providers
            .iter()
            .position(|p| p.id == input.provider_id)
            .unwrap_or(0);
        Self {
            id,
            model_name: mk(input.model_name),
            provider_index,
            context_length: mk(input.context_length.to_string()),
            max_output_tokens: mk(input.max_output_tokens.to_string()),
            input_token_price: mk(format!("{}", input.input_token_price)),
            output_token_price: mk(format!("{}", input.output_token_price)),
            supports_function_calling: input.supports_function_calling,
            supports_streaming: input.supports_streaming,
            supports_thinking: input.supports_thinking,
            vision_ability: input.vision_ability,
            focus: 0,
        }
    }

    pub fn text_fields_mut(&mut self) -> [&mut InputState; 5] {
        [
            &mut self.model_name,
            &mut self.context_length,
            &mut self.max_output_tokens,
            &mut self.input_token_price,
            &mut self.output_token_price,
        ]
    }

    pub fn text_fields(&self) -> [&InputState; 5] {
        [
            &self.model_name,
            &self.context_length,
            &self.max_output_tokens,
            &self.input_token_price,
            &self.output_token_price,
        ]
    }

    /// Validate and collect the form into a `ModelInput`.
    pub fn collect(&self, providers: &[ProviderRow]) -> Result<ModelInput, String> {
        let model_name = self.model_name.value().trim().to_string();
        if model_name.is_empty() {
            return Err("model name must not be empty".into());
        }
        let provider_id = providers
            .get(self.provider_index)
            .map(|p| p.id)
            .ok_or_else(|| "no provider selected".to_string())?;
        let parse_i = |s: &str, field: &str| -> Result<i64, String> {
            s.trim()
                .parse()
                .map_err(|_| format!("{field} must be an integer"))
        };
        let parse_f = |s: &str, field: &str| -> Result<f64, String> {
            s.trim()
                .parse()
                .map_err(|_| format!("{field} must be a number"))
        };
        Ok(ModelInput {
            model_name,
            provider_id,
            context_length: parse_i(self.context_length.value(), "context length")?,
            max_output_tokens: parse_i(self.max_output_tokens.value(), "max output tokens")?,
            input_token_price: parse_f(self.input_token_price.value(), "input price")?,
            output_token_price: parse_f(self.output_token_price.value(), "output price")?,
            vision_ability: self.vision_ability,
            supports_function_calling: self.supports_function_calling,
            supports_streaming: self.supports_streaming,
            supports_thinking: self.supports_thinking,
        })
    }
}

/// Mutable state for the Config tab.
pub struct ConfigTabState {
    pub providers: Vec<ProviderRow>,
    pub models: Vec<ModelRow>,
    pub pane: ConfigPane,
    pub selected_provider: usize,
    pub selected_model: usize,
    pub mode: ConfigMode,
    /// Transient status line message (e.g. "saved", "deleted", validation error).
    pub message: String,
}

impl Default for ConfigTabState {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            models: Vec::new(),
            pane: ConfigPane::Providers,
            selected_provider: 0,
            selected_model: 0,
            mode: ConfigMode::Browsing,
            message: String::new(),
        }
    }
}

impl ConfigTabState {
    pub fn selected_provider_row(&self) -> Option<&ProviderRow> {
        self.providers.get(self.selected_provider)
    }

    pub fn selected_model_row(&self) -> Option<&ModelRow> {
        self.models.get(self.selected_model)
    }

    pub fn move_selection(&mut self, delta: i32) {
        match self.pane {
            ConfigPane::Providers => {
                if self.providers.is_empty() {
                    return;
                }
                let len = self.providers.len() as i32;
                let mut i = self.selected_provider as i32 + delta;
                if i < 0 {
                    i = len - 1;
                } else if i >= len {
                    i = 0;
                }
                self.selected_provider = i as usize;
            }
            ConfigPane::Models => {
                if self.models.is_empty() {
                    return;
                }
                let len = self.models.len() as i32;
                let mut i = self.selected_model as i32 + delta;
                if i < 0 {
                    i = len - 1;
                } else if i >= len {
                    i = 0;
                }
                self.selected_model = i as usize;
            }
        }
    }
}

// ── AppState ───────────────────────────────────────────

/// State container for the TUI.
#[derive(Default)]
pub struct AppState {
    pub main_tab_state: MainTabState,
    pub agent_tab_state: AgentTabState,
    pub config_tab_state: ConfigTabState,
}
