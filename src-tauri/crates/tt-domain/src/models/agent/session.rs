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
    /// Model output and compaction records carry their origin; the user's own messages have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<AgentSessionMessageOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction: Option<AgentSessionCompaction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionMessageOrigin {
    pub invocation_id: String,
    pub round: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionCompaction {
    /// The first entry kept as it was, the start of a whole group; none when no group fit.
    pub kept_from_seq: Option<u64>,
    pub tokens_before: usize,
    pub tokens_after: usize,
}

/// What is appended to a Session's history; the store assigns `seq` and `created_at`.
pub struct NewAgentSessionMessage {
    pub run_id: String,
    pub message: AgentModelMessage,
    pub origin: Option<AgentSessionMessageOrigin>,
    pub compaction: Option<AgentSessionCompaction>,
}
