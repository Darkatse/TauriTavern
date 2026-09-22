use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{AgentModelContentPart, AgentModelMessage};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSession {
    pub id: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub last_used_at: Option<DateTime<Utc>>,
}

impl AgentSession {
    pub fn record_user_message(&mut self, message: &AgentModelMessage, at: DateTime<Utc>) {
        self.last_used_at = Some(at);
        if self
            .title
            .as_ref()
            .is_none_or(|title| title.trim().is_empty())
        {
            let text = message
                .parts
                .iter()
                .filter_map(|part| match part {
                    AgentModelContentPart::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .flat_map(str::split_whitespace)
                .flat_map(|word| std::iter::once(' ').chain(word.chars()))
                .skip(1)
                .take(80)
                .collect::<String>();
            self.title = Some(text.trim_end().to_owned());
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionMessage {
    pub seq: u64,
    pub run_id: String,
    pub created_at: DateTime<Utc>,
    pub message: AgentModelMessage,
    /// Invocation output identity; user messages have no model origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<AgentSessionMessageOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionMessageOrigin {
    pub invocation_id: String,
    pub round: usize,
}
