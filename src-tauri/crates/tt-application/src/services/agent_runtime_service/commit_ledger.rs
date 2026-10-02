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
    /// What the chat message is known to show. Every confirmed publication sets it, explicit
    /// or automatic: the host checks the sha256 before publishing. A rejected commit may have
    /// left the message half-updated, and the host may have previewed a streamed
    /// `write_file` in the message, so either makes it unconfirmed again.
    /// Not checkpointed, because a resumed or revised run starts a new host bridge; a
    /// revision starts from the completed reply instead (see `record_revision_start`).
    #[serde(skip)]
    shown: ShownMessage,
}

/// What the chat message shows, as far as the run knows.
#[derive(Debug, Default)]
enum ShownMessage {
    /// Nothing the host confirmed: no publication yet, or a rejected commit or a streamed
    /// preview since the last one.
    #[default]
    Unconfirmed,
    /// A confirmed `replace` publication: the message is exactly this file content.
    Replaced { path: String, sha256: String },
    /// A confirmed message that is not exactly one file's content: an `append` publication,
    /// or the completed reply a revision starts from, which the user may have edited.
    Untracked,
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
        self.shown = match mode {
            AgentChatCommitMode::Replace => ShownMessage::Replaced {
                path: file.path.as_str().to_string(),
                sha256: file.sha256.clone(),
            },
            AgentChatCommitMode::Append => ShownMessage::Untracked,
        };
        self.commits.push(CommittedChatMessage {
            path: file.path.as_str().to_string(),
            mode,
            message_id,
            round,
        });
    }

    /// The chat message may no longer show the last confirmed publication; see `shown`.
    pub(super) fn mark_unconfirmed(&mut self) {
        self.shown = ShownMessage::Unconfirmed;
    }

    /// A revision starts from the run's completed reply, which the chat message shows as
    /// published. A text-only turn may keep it without publishing it again.
    pub(super) fn record_revision_start(&mut self) {
        self.shown = ShownMessage::Untracked;
    }

    /// Whether committing `file` with `mode` would publish exactly what the chat shows.
    pub(super) fn already_shows(&self, file: &WorkspaceFile, mode: AgentChatCommitMode) -> bool {
        mode == AgentChatCommitMode::Replace
            && matches!(
                &self.shown,
                ShownMessage::Replaced { path, sha256 }
                    if path == file.path.as_str() && *sha256 == file.sha256
            )
    }

    /// Whether the chat message shows a publication the host confirmed, with no rejected
    /// commit or streamed preview since. This is the `committed` finish condition.
    pub(super) fn shows_confirmed_publication(&self) -> bool {
        !matches!(self.shown, ShownMessage::Unconfirmed)
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
