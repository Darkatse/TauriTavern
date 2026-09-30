use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::watch;
use tt_domain::errors::DomainError;
use tt_domain::frozen_macros::FrozenMacros;

use crate::workspace_fs::WorkspaceFs;

pub struct WorkspaceShellRequest {
    pub command: String,
    pub workdir: String,
    pub files: Arc<dyn WorkspaceFs>,
    pub context: Arc<WorkspaceShellContext>,
    /// Optional read-only access to the current run's character chat. `None` means the
    /// capability is withheld from this run altogether — no character chat target, or a
    /// Profile that does not grant it — and the JavaScript `chat` API reports
    /// `chat.unsupported` for every call.
    pub chat: Option<Arc<dyn ChatMessageSource>>,
    pub cancel: watch::Receiver<bool>,
}

/// Line range requested from a single message, matching `chat.read_messages`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageRange {
    /// 1-based first line to return.
    pub start_line: usize,
    /// Optional number of lines to return from `start_line`.
    pub line_count: Option<usize>,
}

/// Maximum bytes a single script-visible message may return.
///
/// Sized against the script runtime's own limits, not the shell's stdout ceiling:
/// one read must not dominate the QuickJS heap (32 MiB) or the execution budget
/// (30 seconds). A message beyond this budget yields
/// [`ChatMessageOutcome::MessageTooLarge`], steering the script to read it in
/// line ranges. Long text belongs in a workspace file, not in output.
pub const SCRIPT_MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// Maximum bytes one script call may materialize from the chat.
///
/// The budget is charged per returned message as its selected text plus
/// [`SCRIPT_MESSAGE_ENTRY_BYTES`], so it bounds the batch size as well as the text:
/// a script cannot trade many tiny messages for an unbounded number of result
/// objects. A call that does not fit yields [`ChatMessageOutcome::CallTooLarge`].
///
/// Sized against the script runtime's own limits, like
/// [`SCRIPT_MAX_MESSAGE_BYTES`]: the QuickJS heap is 32 MiB and also holds the
/// script's own state, wide strings, and the per-message conversion peaks.
pub const SCRIPT_MAX_CALL_BYTES: usize = 8 * 1024 * 1024;

/// Bytes charged for one requested message on top of its selected text.
///
/// One message becomes a JavaScript object with a dozen fields and a `ref` string,
/// so entry count costs heap independently of the text. Charging this overhead is
/// what lets [`SCRIPT_MAX_CALL_BYTES`] bound a batch of near-empty messages instead
/// of only the text it returns.
pub const SCRIPT_MESSAGE_ENTRY_BYTES: usize = 256;

/// One successfully read message, already macro-rendered and range-selected.
#[derive(Debug, Clone)]
pub struct ChatMessageRead {
    pub index: usize,
    pub role: &'static str,
    pub name: Option<String>,
    pub send_date: Option<String>,
    /// Selected text after line-range selection and macro rendering.
    pub text: String,
    /// `chat:current#{index}:L{start}-L{end}`, identical to `chat.read_messages`.
    pub ref_id: String,
    pub start_line: usize,
    pub end_line: usize,
    pub total_lines: usize,
    pub total_bytes: usize,
    /// True when the returned window is shorter than the caller asked for: the
    /// byte budget ended it early, or a single line was clipped by it. Starting
    /// at an explicit line is pagination, not a preview.
    pub preview: bool,
}

/// Outcome of reading one or more messages. Over-limit and lookup failures are
/// reported as variants so the script decides how to recover; nothing is silently
/// truncated or substituted.
#[derive(Debug, Clone)]
pub enum ChatMessageOutcome {
    /// All requested messages were read. `total_messages` is the frozen upper bound.
    Found {
        total_messages: usize,
        messages: Vec<ChatMessageRead>,
    },
    /// The chat payload has no visible message at one of the requested indexes.
    MessageNotFound { index: usize, total_messages: usize },
    /// The chat payload itself is missing.
    ChatNotFound,
    /// A requested line range is invalid for that message.
    InvalidRange { index: usize, message: String },
    /// A single selected message exceeds the script-visible byte budget.
    MessageTooLarge {
        index: usize,
        total_bytes: usize,
        max_bytes: usize,
    },
    /// The requested messages together exceed the per-call byte budget.
    ///
    /// `used_bytes` is the total the call would have needed and `index` is the
    /// request that did not fit, both measured by [`SCRIPT_MAX_CALL_BYTES`]'s
    /// accounting rule. Nothing is returned, so the script narrows the request.
    CallTooLarge {
        index: usize,
        used_bytes: usize,
        max_bytes: usize,
    },
    /// The run has no character chat to read: a group chat, or no character chat
    /// target. A source reports this for a run it cannot serve; a caller that withholds
    /// the capability altogether passes no source at all, see
    /// [`WorkspaceShellRequest::chat`].
    Unsupported,
}

/// Read-only, character-chat-only access to chat messages by absolute index.
///
/// Implementations must read from the frozen run input view, never beyond
/// `input_message_count`, and never return more messages than the caller requested.
/// A run without a frozen input count has no trustworthy bound, so implementations
/// must fail with the `agent.chat_input_count_missing` [`DomainError`] rather than
/// fall back to the live chat length, which would expose history the run was not
/// built from. Reporting [`ChatMessageOutcome::Unsupported`] there instead would
/// leave a caller unable to tell a damaged record from an absent capability.
///
/// A caller that withholds the capability altogether, such as a Profile that does not
/// grant it, passes no source at all — [`WorkspaceShellRequest::chat`] is `None` for
/// that case — while [`ChatMessageOutcome::Unsupported`] is what a source reports for
/// a run it cannot serve.
#[async_trait]
pub trait ChatMessageSource: Send + Sync {
    /// Read the requested messages in one pass. `requests` pairs an absolute
    /// 0-based message index with an optional line range.
    async fn read(
        &self,
        requests: &[(usize, Option<MessageRange>)],
    ) -> Result<ChatMessageOutcome, DomainError>;
}

/// Frozen host facts, separate from the ability to execute ordinary workspace code.
#[derive(Debug)]
pub struct WorkspaceShellContext {
    pub frozen_macros: Arc<FrozenMacros>,
    pub host: Result<serde_json::Value, String>,
}

impl Default for WorkspaceShellContext {
    fn default() -> Self {
        Self {
            frozen_macros: Arc::default(),
            host: Err(
                "This run has no JavaScript host context. Workspace files remain available.".into(),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceShellExit {
    Exited(i32),
    Cancelled,
    TimedOut,
    Failed,
}

#[derive(Debug)]
pub struct WorkspaceShellResult {
    pub stdout: String,
    pub stderr: String,
    pub exit: WorkspaceShellExit,
    pub output_truncated: bool,
}

/// Execute one independent shell against the supplied workspace. Completed
/// file operations remain visible even when a later command fails or is cancelled.
/// Request cancellation through `request.cancel` and keep awaiting `execute`:
/// it stops further interpreter scheduling and awaits started JavaScript and
/// file operations before returning.
/// Dropping this future is not a cancellation mechanism.
#[async_trait]
pub trait WorkspaceShell: Send + Sync {
    async fn execute(
        &self,
        request: WorkspaceShellRequest,
    ) -> Result<WorkspaceShellResult, DomainError>;
}
