use std::io::SeekFrom;
use std::path::Path;

use async_trait::async_trait;
use chrono::Utc;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use tt_adapter_storage_core::file_system::persist_json_file;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::AgentModelRole;
use tt_domain::models::agent::profile::{AgentProfileDefinition, deserialize_profile_definition};
use tt_domain::models::agent::session::{
    AgentSession, AgentSessionMessage, NewAgentSessionMessage,
};
use tt_ports::repositories::agent_run_repository::AgentRunRepository;
use tt_ports::repositories::agent_session_repository::{
    AgentSessionMessageReadQuery, AgentSessionRepository,
};

use super::FileAgentRepository;
use super::run_prune_store::remove_index_file_if_exists;

#[async_trait]
impl AgentSessionRepository for FileAgentRepository {
    async fn load_session_profile(&self) -> Result<Option<AgentProfileDefinition>, DomainError> {
        let path = self.root.join("sessions/profile.json");
        let Some(value) = Self::try_read_json::<serde_json::Value>(&path).await? else {
            return Ok(None);
        };
        deserialize_profile_definition(value)
            .map(Some)
            .map_err(|error| {
                DomainError::InvalidData(format!(
                    "Invalid Agent session profile in {}: {error}",
                    path.display()
                ))
            })
    }

    async fn save_session_profile(
        &self,
        profile: &AgentProfileDefinition,
    ) -> Result<(), DomainError> {
        persist_json_file(&self.root.join("sessions/profile.json"), profile).await
    }

    async fn create_session(&self, session: &AgentSession) -> Result<(), DomainError> {
        let directory = self.session_dir(&session.id)?;
        fs::create_dir_all(self.root.join("sessions"))
            .await
            .map_err(|error| DomainError::file_io("create sessions", &session.id, error))?;
        fs::create_dir(&directory)
            .await
            .map_err(|error| DomainError::file_io("create session", &session.id, error))?;
        for root in ["work", "tmp", "tool-results"] {
            fs::create_dir_all(directory.join("workspace").join(root))
                .await
                .map_err(|error| {
                    DomainError::file_io("create session workspace", &session.id, error)
                })?;
        }
        persist_json_file(&directory.join("session.json"), session).await
    }

    async fn load_session(&self, session_id: &str) -> Result<AgentSession, DomainError> {
        let path = self.session_dir(session_id)?.join("session.json");
        let session: AgentSession = Self::try_read_json(&path).await?.ok_or_else(|| {
            DomainError::NotFound(format!("Agent session not found: {session_id}"))
        })?;
        if session.id != session_id {
            return Err(DomainError::InvalidData(format!(
                "Agent session identity mismatch: {session_id}"
            )));
        }
        Ok(session)
    }

    async fn list_sessions(&self) -> Result<Vec<AgentSession>, DomainError> {
        let directory = self.root.join("sessions");
        let mut entries = match fs::read_dir(&directory).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(DomainError::file_io(
                    "list sessions",
                    directory.display().to_string(),
                    error,
                ));
            }
        };
        let mut sessions = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            DomainError::file_io("list sessions", directory.display().to_string(), error)
        })? {
            let file_type = entry.file_type().await.map_err(|error| {
                DomainError::file_io("inspect session", entry.path().display().to_string(), error)
            })?;
            if file_type.is_symlink() {
                return Err(DomainError::InvalidData(format!(
                    "Session entry is a symlink: {}",
                    entry.path().display()
                )));
            }
            if !file_type.is_dir() {
                continue;
            }
            let session_id = entry
                .file_name()
                .into_string()
                .map_err(|_| DomainError::InvalidData("Session id is not UTF-8".into()))?;
            sessions.push(self.load_session(&session_id).await?);
        }
        sessions.sort_by(|left, right| {
            right
                .last_used_at
                .unwrap_or(right.created_at)
                .cmp(&left.last_used_at.unwrap_or(left.created_at))
                .then_with(|| right.id.cmp(&left.id))
        });
        Ok(sessions)
    }

    async fn rename_session(
        &self,
        session_id: &str,
        title: &str,
    ) -> Result<AgentSession, DomainError> {
        let lock = self
            .workspace_lock(&format!("session-history:{session_id}"))
            .await;
        let _guard = lock.write().await;
        let mut session = self.load_session(session_id).await?;
        session.title = Some(title.to_owned());
        persist_json_file(
            &self.session_dir(session_id)?.join("session.json"),
            &session,
        )
        .await?;
        Ok(session)
    }

    async fn delete_session(&self, session_id: &str) -> Result<(), DomainError> {
        let directory = self.session_dir(session_id)?;
        let lock = self
            .workspace_lock(&format!("session-history:{session_id}"))
            .await;
        let _guard = lock.write().await;
        match fs::symlink_metadata(&directory).await {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(DomainError::InvalidData(format!(
                    "Session path is not a directory: {}",
                    directory.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(DomainError::file_io("inspect session", session_id, error)),
        }
        let run_ids = self.run_ids_in_workspace(&directory).await?;
        // Keep the run directory names available until all external indexes are removed,
        // so a failed deletion can be retried without scanning other Sessions.
        for run_id in &run_ids {
            remove_index_file_if_exists(&self.index_session_run_path(run_id)?, "Session run index")
                .await?;
            remove_index_file_if_exists(
                &self.index_run_summary_path(run_id)?,
                "Session run summary",
            )
            .await?;
        }
        {
            let mut sequences = self.event_sequences.lock().await;
            for run_id in &run_ids {
                sequences.remove(run_id);
            }
        }
        match fs::remove_dir_all(&directory).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(DomainError::file_io("delete session", session_id, error)),
        }
    }

    async fn append_session_message(
        &self,
        session_id: &str,
        entry: NewAgentSessionMessage,
    ) -> Result<AgentSessionMessage, DomainError> {
        let NewAgentSessionMessage {
            run_id,
            message,
            origin,
            compaction,
        } = entry;
        let run = self.load_run(&run_id).await?;
        if run.target.session_id() != Some(session_id) {
            return Err(DomainError::InvalidData(format!(
                "Agent run {run_id} does not belong to session {session_id}"
            )));
        }
        let lock = self
            .workspace_lock(&format!("session-history:{session_id}"))
            .await;
        let _guard = lock.write().await;
        let mut session = self.load_session(session_id).await?;
        let seq = self
            .last_session_seq_locked(session_id)
            .await?
            .checked_add(1)
            .ok_or_else(|| DomainError::InvalidData("Session message sequence exhausted".into()))?;
        let entry = AgentSessionMessage {
            seq,
            run_id,
            created_at: Utc::now(),
            message,
            origin,
            compaction,
        };
        let mut line = serde_json::to_vec(&entry).map_err(|error| {
            DomainError::InvalidData(format!("Invalid session message: {error}"))
        })?;
        line.push(b'\n');
        let path = self.session_dir(session_id)?.join("history.jsonl");
        let result = async {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await?;
            file.write_all(&line).await?;
            file.flush().await
        }
        .await;
        if let Err(error) = result {
            return Err(DomainError::file_io(
                "append session history",
                session_id,
                error,
            ));
        }
        if entry.message.role == AgentModelRole::User && entry.origin.is_none() {
            session.record_user_message(&entry.message, entry.created_at);
            persist_json_file(
                &self.session_dir(session_id)?.join("session.json"),
                &session,
            )
            .await?;
        }
        Ok(entry)
    }

    async fn read_session_messages(
        &self,
        session_id: &str,
        query: AgentSessionMessageReadQuery,
    ) -> Result<Vec<AgentSessionMessage>, DomainError> {
        let lock = self
            .workspace_lock(&format!("session-history:{session_id}"))
            .await;
        let _guard = lock.read().await;
        self.load_session(session_id).await?;
        self.read_session_message_page(session_id, query).await
    }

    async fn session_last_seq(&self, session_id: &str) -> Result<u64, DomainError> {
        let lock = self
            .workspace_lock(&format!("session-history:{session_id}"))
            .await;
        let _guard = lock.write().await;
        self.load_session(session_id).await?;
        self.last_session_seq_locked(session_id).await
    }
}

impl FileAgentRepository {
    async fn last_session_seq_locked(&self, session_id: &str) -> Result<u64, DomainError> {
        let page = self
            .read_session_message_page(
                session_id,
                AgentSessionMessageReadQuery {
                    before_seq: None,
                    limit: 1,
                },
            )
            .await?;
        Ok(page.last().map_or(0, |entry| entry.seq))
    }

    async fn read_session_message_page(
        &self,
        session_id: &str,
        query: AgentSessionMessageReadQuery,
    ) -> Result<Vec<AgentSessionMessage>, DomainError> {
        if query.limit == 0 {
            return Err(DomainError::InvalidData(
                "Session history limit must be positive".into(),
            ));
        }
        let path = self.session_dir(session_id)?.join("history.jsonl");
        let read_error = |error| DomainError::file_io("read session history", session_id, error);
        let Some(mut history) = HistoryFromEnd::open(&path).await.map_err(read_error)? else {
            return Ok(Vec::new());
        };
        if history.pending.last().is_some_and(|byte| *byte != b'\n') {
            return Err(DomainError::InvalidData(format!(
                "Incomplete session history record: {}",
                path.display()
            )));
        }
        let out_of_sequence = |expected: u64, found: &dyn std::fmt::Display| {
            DomainError::InvalidData(format!(
                "Invalid session history sequence in {}: expected {expected}, found {found}",
                path.display()
            ))
        };
        let mut page = Vec::new();
        // The seq of the next record back, known once the last record is read.
        let mut next_seq = None;
        while page.len() < query.limit {
            let record = history.previous().await.map_err(read_error)?;
            let Some(record) = record else {
                if let Some(seq @ 1..) = next_seq {
                    return Err(out_of_sequence(seq, &"the start of the file"));
                }
                break;
            };
            if next_seq == Some(0) {
                return Err(out_of_sequence(0, &"a record before seq 1"));
            }
            // Records after the page are only counted: line n holds seq n.
            if let Some(seq) = next_seq
                && query.before_seq.is_some_and(|before| seq >= before)
            {
                next_seq = Some(seq - 1);
                continue;
            }
            let entry: AgentSessionMessage = serde_json::from_slice(&record).map_err(|error| {
                DomainError::InvalidData(format!(
                    "Invalid session history in {}: {error}",
                    path.display()
                ))
            })?;
            if entry.seq == 0 || next_seq.is_some_and(|seq| seq != entry.seq) {
                return Err(out_of_sequence(next_seq.unwrap_or(1), &entry.seq));
            }
            next_seq = Some(entry.seq - 1);
            if query.before_seq.is_none_or(|before| entry.seq < before) {
                page.push(entry);
            }
        }
        page.reverse();
        Ok(page)
    }
}

/// Reads a JSONL file back from its end, one `\n`-terminated record at a time.
struct HistoryFromEnd {
    file: File,
    /// File offset of `pending`.
    offset: u64,
    /// Bytes not yet returned, ending with the newline of the next record back.
    pending: Vec<u8>,
}

impl HistoryFromEnd {
    const BLOCK: u64 = 64 * 1024;

    async fn open(path: &Path) -> std::io::Result<Option<Self>> {
        let file = match File::open(path).await {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let offset = file.metadata().await?.len();
        let mut history = Self {
            file,
            offset,
            pending: Vec::new(),
        };
        if offset > 0 {
            history.read_block().await?;
        }
        Ok(Some(history))
    }

    /// Prepends the block before `pending`; returns its length. A block is at least as long as `pending`, so the
    /// bytes copied for a long record stay linear in its length.
    async fn read_block(&mut self) -> std::io::Result<usize> {
        let start = self
            .offset
            .saturating_sub(Self::BLOCK.max(self.pending.len() as u64));
        let mut block = vec![0; (self.offset - start) as usize];
        self.file.seek(SeekFrom::Start(start)).await?;
        self.file.read_exact(&mut block).await?;
        let read = block.len();
        block.extend_from_slice(&self.pending);
        self.pending = block;
        self.offset = start;
        Ok(read)
    }

    /// The record before those already returned, without its newline.
    async fn previous(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        // The newline before the record's own ends the record before it.
        let Some(mut unsearched) = self.pending.len().checked_sub(1) else {
            return Ok(None);
        };
        loop {
            if let Some(index) = memchr::memrchr(b'\n', &self.pending[..unsearched]) {
                let mut record = self.pending.split_off(index + 1);
                record.pop();
                return Ok(Some(record));
            }
            if self.offset == 0 {
                let mut record = std::mem::take(&mut self.pending);
                record.pop();
                return Ok(Some(record));
            }
            unsearched = self.read_block().await?;
        }
    }
}
