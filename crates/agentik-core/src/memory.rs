use agentik_sdk::model::Model;
use agentik_types::errors::AnthropicError;
use agentik_types::messages::{ContentBlock, Message};
use crate::message_ext::AgentMessageExt;
use agentik_types::Tool;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::prompt::compact;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub items: Vec<MemoryItem>,
}

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("None item inside curruent memory")]
    EmptyMemoryItem,

    #[error("Failed to compact memory: {0}")]
    Compact(#[from] AnthropicError),
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    pub messages: Vec<Message>,
    pub summary: Option<String>,
}

impl Memory {
    pub fn new() -> Self {
        Self {
            items: vec![MemoryItem::default()],
        }
    }

    fn get_last_item(&self) -> Option<&MemoryItem> {
        self.items.last()
    }

    fn add_summary_to_last_item(&mut self, summary: String) -> Result<(), MemoryError> {
        self.items
            .last_mut()
            .ok_or(MemoryError::EmptyMemoryItem)?
            .summary = Some(summary);
        Ok(())
    }

    fn create_mem_item(&mut self) {
        self.items.push(MemoryItem::default());
    }

    pub fn remember(&mut self, mesage: Message) -> Result<(), MemoryError> {
        self.items
            .last_mut()
            .ok_or(MemoryError::EmptyMemoryItem)?
            .messages
            .push(mesage);
        Ok(())
    }

    pub fn render_context(&self) -> Result<&[Message], MemoryError> {
        let res = &self
            .items
            .last()
            .ok_or(MemoryError::EmptyMemoryItem)?
            .messages;

        Ok(res)
    }

    pub async fn compact(&mut self, model: &Model) -> Result<(), MemoryError> {
        let mut messages: Vec<Message> = vec![];
        let compace_prompt = compact::NO_TOOLS_PREAMBLE.to_string() + compact::BASE_COMPACT_PROMPT;
        messages.push(Message::system(compace_prompt));

        let messages_to_compact = self
            .get_last_item()
            .ok_or(MemoryError::EmptyMemoryItem)?
            .messages
            .clone();

        messages.extend(messages_to_compact);

        let summary = model
            .request(messages, &Vec::<Tool>::new())
            .await?;

        self.add_summary_to_last_item(
            summary
                .content
                .iter()
                .map(|c| match c {
                    ContentBlock::Text { text } => text.clone(),
                    _ => "".to_string(),
                })
                .collect::<Vec<String>>()
                .join(""),
        )?;

        self.create_mem_item();

        tracing::debug!("compact finished");
        Ok(())
    }
}
