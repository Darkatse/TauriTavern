use serde_json::{Value, json};

use crate::domain::models::agent::{AgentChatCommitMode, WorkspacePath};

#[derive(Debug, Clone)]
pub(super) struct CommittedChatMessage {
    path: String,
    mode: AgentChatCommitMode,
    message_id: Option<String>,
    round: usize,
}

#[derive(Debug, Default)]
pub(super) struct RunCommitLedger {
    commits: Vec<CommittedChatMessage>,
}

impl RunCommitLedger {
    pub(super) fn record(
        &mut self,
        path: &WorkspacePath,
        mode: AgentChatCommitMode,
        message_id: Option<String>,
        round: usize,
    ) {
        self.commits.push(CommittedChatMessage {
            path: path.as_str().to_string(),
            mode,
            message_id,
            round,
        });
    }

    pub(super) fn is_empty(&self) -> bool {
        self.commits.is_empty()
    }

    pub(super) fn len(&self) -> usize {
        self.commits.len()
    }

    pub(super) fn latest_message_id(&self) -> Option<&str> {
        self.commits
            .last()
            .and_then(|message| message.message_id.as_deref())
    }

    pub(super) fn preserved_commits(&self) -> Vec<Value> {
        self.commits
            .iter()
            .map(|message| {
                json!({
                    "path": message.path.as_str(),
                    "mode": message.mode,
                    "messageId": message.message_id.as_deref(),
                    "round": message.round,
                })
            })
            .collect()
    }
}
