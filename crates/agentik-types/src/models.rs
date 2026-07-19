use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Model {
    #[serde(rename = "claude-3-5-sonnet-latest")]
    Claude3_5SonnetLatest,
    #[serde(rename = "claude-3-5-sonnet-20241022")]
    Claude3_5Sonnet20241022,
    #[serde(rename = "claude-3-5-sonnet-20240620")]
    Claude3_5Sonnet20240620,

    #[serde(rename = "claude-3-5-haiku-latest")]
    Claude3_5HaikuLatest,
    #[serde(rename = "claude-3-5-haiku-20241022")]
    Claude3_5Haiku20241022,

    #[serde(rename = "claude-3-opus-latest")]
    Claude3OpusLatest,
    #[serde(rename = "claude-3-opus-20240229")]
    Claude3Opus20240229,

    #[serde(rename = "claude-3-sonnet-20240229")]
    Claude3Sonnet20240229,

    #[serde(rename = "claude-3-haiku-20240307")]
    Claude3Haiku20240307,

    #[serde(rename = "claude-2.1")]
    Claude2_1,
    #[serde(rename = "claude-2.0")]
    Claude2_0,
}

impl Model {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Claude3_5SonnetLatest => "claude-3-5-sonnet-latest",
            Self::Claude3_5Sonnet20241022 => "claude-3-5-sonnet-20241022",
            Self::Claude3_5Sonnet20240620 => "claude-3-5-sonnet-20240620",
            Self::Claude3_5HaikuLatest => "claude-3-5-haiku-latest",
            Self::Claude3_5Haiku20241022 => "claude-3-5-haiku-20241022",
            Self::Claude3OpusLatest => "claude-3-opus-latest",
            Self::Claude3Opus20240229 => "claude-3-opus-20240229",
            Self::Claude3Sonnet20240229 => "claude-3-sonnet-20240229",
            Self::Claude3Haiku20240307 => "claude-3-haiku-20240307",
            Self::Claude2_1 => "claude-2.1",
            Self::Claude2_0 => "claude-2.0",
        }
    }

    #[must_use]
    pub fn family(&self) -> &'static str {
        match self {
            Self::Claude3_5SonnetLatest
            | Self::Claude3_5Sonnet20241022
            | Self::Claude3_5Sonnet20240620
            | Self::Claude3Sonnet20240229 => "sonnet",
            Self::Claude3_5HaikuLatest
            | Self::Claude3_5Haiku20241022
            | Self::Claude3Haiku20240307 => "haiku",
            Self::Claude3OpusLatest | Self::Claude3Opus20240229 => "opus",
            Self::Claude2_1 | Self::Claude2_0 => "claude-2",
        }
    }

    #[must_use]
    pub fn supports_vision(&self) -> bool {
        !matches!(self, Self::Claude2_1 | Self::Claude2_0)
    }

    #[must_use]
    pub fn supports_tools(&self) -> bool {
        !matches!(self, Self::Claude2_1 | Self::Claude2_0)
    }
}

impl std::fmt::Display for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<Model> for String {
    fn from(model: Model) -> String {
        model.as_str().to_string()
    }
}
