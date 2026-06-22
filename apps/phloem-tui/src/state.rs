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

    pub fn next(&self) -> Self {
        match self {
            Self::AgentTab => Self::ConfigTab,
            Self::ConfigTab => Self::AgentTab,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::AgentTab => Self::ConfigTab,
            Self::ConfigTab => Self::AgentTab,
        }
    }
}

impl Default for MainTabState {
    fn default() -> Self {
        Self::AgentTab
    }
}

/// State container for tracing widgets' states
#[derive(Default)]
pub struct AppState {
    pub main_tab_state: MainTabState,
}
