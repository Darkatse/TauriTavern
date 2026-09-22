use std::collections::VecDeque;

use async_trait::async_trait;
use chrono::Utc;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use tt_adapter_storage_core::file_system::persist_json_file;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::AgentModelMessage;
use tt_domain::models::agent::profile::AgentProfileDefinition;
use tt_domain::models::agent::session::{AgentSession, AgentSessionMessage};
use tt_ports::repositories::agent_run_repository::AgentRunRepository;
use tt_ports::repositories::agent_session_repository::{
    AgentSessionMessageReadQuery, AgentSessionRepository,
};

use super::FileAgentRepository;

#[async_trait]
impl AgentSessionRepository for FileAgentRepository {
    async fn load_session_profile(&self) -> Result<Option<AgentProfileDefinition>, DomainError> {
        Self::try_read_json(&self.root.join("sessions/profile.json")).await
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

    async fn append_session_message(
        &self,
        session_id: &str,
        run_id: &str,
        message: &AgentModelMessage,
    ) -> Result<AgentSessionMessage, DomainError> {
        self.load_session(session_id).await?;
        let run = self.load_run(run_id).await?;
        if run.target.session_id() != Some(session_id) {
            return Err(DomainError::InvalidData(format!(
                "Agent run {run_id} does not belong to session {session_id}"
            )));
        }
        let lock = self
            .workspace_lock(&format!("session-history:{session_id}"))
            .await;
        let _guard = lock.write().await;
        let seq = self
            .last_session_seq_locked(session_id)
            .await?
            .checked_add(1)
            .ok_or_else(|| DomainError::InvalidData("Session message sequence exhausted".into()))?;
        let entry = AgentSessionMessage {
            seq,
            run_id: run_id.to_owned(),
            created_at: Utc::now(),
            message: message.clone(),
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
            self.session_sequences.lock().await.remove(session_id);
            return Err(DomainError::file_io(
                "append session history",
                session_id,
                error,
            ));
        }
        self.session_sequences
            .lock()
            .await
            .insert(session_id.to_owned(), seq);
        Ok(entry)
    }

    async fn read_session_messages(
        &self,
        session_id: &str,
        query: AgentSessionMessageReadQuery,
    ) -> Result<Vec<AgentSessionMessage>, DomainError> {
        self.load_session(session_id).await?;
        let lock = self
            .workspace_lock(&format!("session-history:{session_id}"))
            .await;
        let _guard = lock.read().await;
        self.read_session_message_page(session_id, query).await
    }

    async fn session_last_seq(&self, session_id: &str) -> Result<u64, DomainError> {
        self.load_session(session_id).await?;
        let lock = self
            .workspace_lock(&format!("session-history:{session_id}"))
            .await;
        let _guard = lock.write().await;
        self.last_session_seq_locked(session_id).await
    }
}

impl FileAgentRepository {
    async fn last_session_seq_locked(&self, session_id: &str) -> Result<u64, DomainError> {
        if let Some(seq) = self.session_sequences.lock().await.get(session_id).copied() {
            return Ok(seq);
        }
        let page = self
            .read_session_message_page(
                session_id,
                AgentSessionMessageReadQuery {
                    after_seq: None,
                    before_seq: None,
                    limit: 1,
                },
            )
            .await?;
        let seq = page.last().map_or(0, |entry| entry.seq);
        self.session_sequences
            .lock()
            .await
            .insert(session_id.to_owned(), seq);
        Ok(seq)
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
        if query.after_seq.is_some() && query.before_seq.is_some() {
            return Err(DomainError::InvalidData(
                "Use either beforeSeq or afterSeq for session history".into(),
            ));
        }
        let path = self.session_dir(session_id)?.join("history.jsonl");
        let file = match File::open(&path).await {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(DomainError::file_io(
                    "read session history",
                    session_id,
                    error,
                ));
            }
        };
        let mut reader = BufReader::new(file);
        let limit = query.limit;
        let mut page = VecDeque::with_capacity(limit.min(500));
        let mut seq = 0;
        let mut line = String::new();
        // ponytail: bounded pages scan JSONL; add a byte index only for measured large-session cost.
        loop {
            line.clear();
            if reader
                .read_line(&mut line)
                .await
                .map_err(|error| DomainError::file_io("read session history", session_id, error))?
                == 0
            {
                break;
            }
            if !line.ends_with('\n') {
                return Err(DomainError::InvalidData(format!(
                    "Incomplete session history record: {}",
                    path.display()
                )));
            }
            let entry: AgentSessionMessage = serde_json::from_str(&line).map_err(|error| {
                DomainError::InvalidData(format!(
                    "Invalid session history in {}: {error}",
                    path.display()
                ))
            })?;
            seq += 1;
            if entry.seq != seq {
                return Err(DomainError::InvalidData(format!(
                    "Invalid session history sequence in {}: expected {seq}, found {}",
                    path.display(),
                    entry.seq
                )));
            }
            if query.before_seq.is_some_and(|before| seq >= before) {
                break;
            }
            if query.after_seq.is_some_and(|after| seq <= after) {
                continue;
            }
            if page.len() == limit {
                page.pop_front();
            }
            page.push_back(entry);
            if query.after_seq.is_some() && page.len() == limit {
                break;
            }
        }
        Ok(page.into_iter().collect())
    }
}
