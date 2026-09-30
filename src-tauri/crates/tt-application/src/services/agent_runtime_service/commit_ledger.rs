use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use tt_domain::models::agent::AgentChatCommitMode;
use tt_ports::workspace_fs::WorkspaceFile;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommittedChatMessage {
    path: String,
    mode: AgentChatCommitMode,
    message_id: Option<String>,
    round: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RunCommitLedger {
    commits: Vec<CommittedChatMessage>,
    explicit_count: usize,
    /// Path and sha256 of the file the chat message is known to show. Only a confirmed
    /// explicit `replace` commit sets it: the host previews writes until the first
    /// explicit commit, and a rejected commit may have left the message half-updated.
    /// Not checkpointed, because a resumed or revised run starts a new host bridge.
    #[serde(skip)]
    shown: Option<(String, String)>,
}

impl RunCommitLedger {
    pub(super) fn record(
        &mut self,
        file: &WorkspaceFile,
        mode: AgentChatCommitMode,
        message_id: Option<String>,
        round: usize,
        is_explicit: bool,
    ) {
        self.explicit_count += usize::from(is_explicit);
        self.shown = (is_explicit && mode == AgentChatCommitMode::Replace)
            .then(|| (file.path.as_str().to_string(), file.sha256.clone()));
        self.commits.push(CommittedChatMessage {
            path: file.path.as_str().to_string(),
            mode,
            message_id,
            round,
        });
    }

    pub(super) fn record_rejection(&mut self) {
        self.shown = None;
    }

    /// Whether committing `file` with `mode` would publish exactly what the chat shows.
    pub(super) fn already_shows(&self, file: &WorkspaceFile, mode: AgentChatCommitMode) -> bool {
        mode == AgentChatCommitMode::Replace
            && self
                .shown
                .as_ref()
                .is_some_and(|(path, sha256)| path == file.path.as_str() && *sha256 == file.sha256)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.commits.is_empty()
    }

    pub(super) fn len(&self) -> usize {
        self.commits.len()
    }

    pub(super) fn explicit_count(&self) -> usize {
        self.explicit_count
    }

    pub(super) fn has_explicit_commit(&self) -> bool {
        self.explicit_count() > 0
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
