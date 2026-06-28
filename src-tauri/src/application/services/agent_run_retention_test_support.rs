use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::domain::errors::DomainError;
use crate::domain::models::agent::{
    AgentRun, AgentRunEvent, AgentRunEventLevel, AgentRunStatus, AgentRunSummaryProjection,
};
use crate::domain::models::settings::{SettingsSnapshot, TauriTavernSettings, UserSettings};
use crate::domain::repositories::agent_run_repository::{
    AgentRunEventReadQuery, AgentRunListCursor, AgentRunListQuery, AgentRunRepository,
    AgentRunStorageEntryStats, AgentRunStorageStats, event_belongs_to_invocation,
};
use crate::domain::repositories::settings_repository::SettingsRepository;

pub(crate) struct TestSettingsRepository {
    tauritavern_settings: Mutex<TauriTavernSettings>,
}

impl TestSettingsRepository {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            tauritavern_settings: Mutex::new(TauriTavernSettings::default()),
        })
    }

    pub(crate) async fn store_tauritavern_settings(&self, settings: TauriTavernSettings) {
        *self.tauritavern_settings.lock().await = settings;
    }
}

#[async_trait]
impl SettingsRepository for TestSettingsRepository {
    async fn save_tauritavern_settings(
        &self,
        settings: &TauriTavernSettings,
    ) -> Result<(), DomainError> {
        self.store_tauritavern_settings(settings.clone()).await;
        Ok(())
    }

    async fn load_tauritavern_settings(&self) -> Result<TauriTavernSettings, DomainError> {
        Ok(self.tauritavern_settings.lock().await.clone())
    }

    async fn save_user_settings(&self, _settings: &UserSettings) -> Result<(), DomainError> {
        Err(unused_settings_method("save_user_settings"))
    }

    async fn load_user_settings(&self) -> Result<UserSettings, DomainError> {
        Err(unused_settings_method("load_user_settings"))
    }

    async fn create_snapshot(&self) -> Result<(), DomainError> {
        Err(unused_settings_method("create_snapshot"))
    }

    async fn get_snapshots(&self) -> Result<Vec<SettingsSnapshot>, DomainError> {
        Err(unused_settings_method("get_snapshots"))
    }

    async fn load_snapshot(&self, _name: &str) -> Result<UserSettings, DomainError> {
        Err(unused_settings_method("load_snapshot"))
    }

    async fn restore_snapshot(&self, _name: &str) -> Result<(), DomainError> {
        Err(unused_settings_method("restore_snapshot"))
    }

    async fn get_themes(&self) -> Result<Vec<UserSettings>, DomainError> {
        Err(unused_settings_method("get_themes"))
    }

    async fn get_moving_ui_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
        Err(unused_settings_method("get_moving_ui_presets"))
    }

    async fn get_quick_reply_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
        Err(unused_settings_method("get_quick_reply_presets"))
    }

    async fn get_instruct_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
        Err(unused_settings_method("get_instruct_presets"))
    }

    async fn get_context_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
        Err(unused_settings_method("get_context_presets"))
    }

    async fn get_sysprompt_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
        Err(unused_settings_method("get_sysprompt_presets"))
    }

    async fn get_reasoning_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
        Err(unused_settings_method("get_reasoning_presets"))
    }

    async fn get_koboldai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError> {
        Err(unused_settings_method("get_koboldai_settings"))
    }

    async fn get_novelai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError> {
        Err(unused_settings_method("get_novelai_settings"))
    }

    async fn get_openai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError> {
        Err(unused_settings_method("get_openai_settings"))
    }

    async fn get_textgen_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError> {
        Err(unused_settings_method("get_textgen_settings"))
    }

    async fn get_world_names(&self) -> Result<Vec<String>, DomainError> {
        Err(unused_settings_method("get_world_names"))
    }
}

pub(crate) struct TestAgentRunRepository {
    store: Mutex<TestAgentRunStore>,
}

#[derive(Default)]
struct TestAgentRunStore {
    runs: HashMap<String, AgentRun>,
    events: HashMap<String, Vec<AgentRunEvent>>,
    storage: HashMap<String, AgentRunStorageStats>,
    projections: HashMap<String, AgentRunSummaryProjection>,
}

impl TestAgentRunRepository {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            store: Mutex::new(TestAgentRunStore::default()),
        })
    }

    pub(crate) async fn add_heavy_artifact(&self, run: &AgentRun, bytes: u64) {
        let mut store = self.store.lock().await;
        let stats = store
            .storage
            .get_mut(&run.id)
            .expect("test run storage must exist");
        add_storage_file(&mut stats.total, bytes);
        add_storage_file(&mut stats.heavy_artifacts, bytes);
    }

    pub(crate) async fn append_terminal_event_for_run(&self, run: &AgentRun) {
        let event_type = match run.status {
            AgentRunStatus::Completed => "run_completed",
            AgentRunStatus::PartialSuccess => "run_partial_success",
            AgentRunStatus::Cancelled => "run_cancelled",
            AgentRunStatus::Failed => "run_failed",
            _ => return,
        };

        let mut store = self.store.lock().await;
        let events = store.events.entry(run.id.clone()).or_default();
        let seq = next_event_seq(events);
        events.push(AgentRunEvent {
            seq,
            id: format!("evt_{}_{}", run.id, seq),
            run_id: run.id.clone(),
            timestamp: run.updated_at,
            level: AgentRunEventLevel::Info,
            event_type: event_type.to_string(),
            payload: Value::Null,
        });
    }

    pub(crate) async fn has_run(&self, run_id: &str) -> bool {
        self.store.lock().await.runs.contains_key(run_id)
    }

    pub(crate) async fn heavy_artifact_count(&self, run_id: &str) -> usize {
        self.store
            .lock()
            .await
            .storage
            .get(run_id)
            .map(|stats| stats.heavy_artifacts.file_count)
            .unwrap_or(0)
    }
}

#[async_trait]
impl AgentRunRepository for TestAgentRunRepository {
    async fn create_run(&self, run: &AgentRun) -> Result<(), DomainError> {
        let mut store = self.store.lock().await;
        store.runs.insert(run.id.clone(), run.clone());
        store.storage.insert(
            run.id.clone(),
            AgentRunStorageStats {
                total: AgentRunStorageEntryStats {
                    file_count: 2,
                    byte_count: 2,
                },
                heavy_artifacts: AgentRunStorageEntryStats::default(),
            },
        );
        Ok(())
    }

    async fn load_run(&self, run_id: &str) -> Result<AgentRun, DomainError> {
        self.store
            .lock()
            .await
            .runs
            .get(run_id)
            .cloned()
            .ok_or_else(|| DomainError::NotFound(format!("Agent run not found: {run_id}")))
    }

    async fn list_runs(&self, query: AgentRunListQuery) -> Result<Vec<AgentRun>, DomainError> {
        let mut runs: Vec<_> = self
            .store
            .lock()
            .await
            .runs
            .values()
            .filter(|run| run_matches_list_query(run, &query))
            .cloned()
            .collect();
        sort_runs_newest_first(&mut runs);
        runs.truncate(query.limit);
        Ok(runs)
    }

    async fn list_all_runs(&self) -> Result<Vec<AgentRun>, DomainError> {
        let mut runs: Vec<_> = self.store.lock().await.runs.values().cloned().collect();
        sort_runs_newest_first(&mut runs);
        Ok(runs)
    }

    async fn inspect_run_storage(
        &self,
        run: &AgentRun,
    ) -> Result<AgentRunStorageStats, DomainError> {
        self.store
            .lock()
            .await
            .storage
            .get(&run.id)
            .copied()
            .ok_or_else(|| {
                DomainError::InvalidData(format!("Agent run storage is missing: {}", run.id))
            })
    }

    async fn slim_run_heavy_artifacts(
        &self,
        run: &AgentRun,
    ) -> Result<AgentRunStorageEntryStats, DomainError> {
        let mut store = self.store.lock().await;
        let stats = store.storage.get_mut(&run.id).ok_or_else(|| {
            DomainError::InvalidData(format!("Agent run storage is missing: {}", run.id))
        })?;
        let removed = stats.heavy_artifacts;
        stats.total.file_count = stats
            .total
            .file_count
            .checked_sub(removed.file_count)
            .expect("test storage stats must include heavy artifact file count");
        stats.total.byte_count = stats
            .total
            .byte_count
            .checked_sub(removed.byte_count)
            .expect("test storage stats must include heavy artifact byte count");
        stats.heavy_artifacts = AgentRunStorageEntryStats::default();
        Ok(removed)
    }

    async fn delete_run(&self, run: &AgentRun) -> Result<AgentRunStorageEntryStats, DomainError> {
        let mut store = self.store.lock().await;
        let stats = store.storage.remove(&run.id).ok_or_else(|| {
            DomainError::InvalidData(format!("Agent run storage is missing: {}", run.id))
        })?;
        store.runs.remove(&run.id);
        store.events.remove(&run.id);
        store.projections.remove(&run.id);
        Ok(stats.total)
    }

    async fn load_run_summary_projection(
        &self,
        run_id: &str,
    ) -> Result<Option<AgentRunSummaryProjection>, DomainError> {
        Ok(self.store.lock().await.projections.get(run_id).cloned())
    }

    async fn save_run_summary_projection(
        &self,
        projection: &AgentRunSummaryProjection,
    ) -> Result<(), DomainError> {
        self.store
            .lock()
            .await
            .projections
            .insert(projection.run_id.clone(), projection.clone());
        Ok(())
    }

    async fn save_run(&self, run: &AgentRun) -> Result<(), DomainError> {
        self.store
            .lock()
            .await
            .runs
            .insert(run.id.clone(), run.clone());
        Ok(())
    }

    async fn append_event(
        &self,
        run_id: &str,
        level: AgentRunEventLevel,
        event_type: &str,
        payload: Value,
    ) -> Result<AgentRunEvent, DomainError> {
        let mut store = self.store.lock().await;
        if !store.runs.contains_key(run_id) {
            return Err(DomainError::NotFound(format!(
                "Agent run not found: {run_id}"
            )));
        }
        let events = store.events.entry(run_id.to_string()).or_default();
        let seq = next_event_seq(events);
        let event = AgentRunEvent {
            seq,
            id: format!("evt_{}_{}", run_id, seq),
            run_id: run_id.to_string(),
            timestamp: Utc::now(),
            level,
            event_type: event_type.to_string(),
            payload,
        };
        events.push(event.clone());
        Ok(event)
    }

    async fn read_events(
        &self,
        run_id: &str,
        query: AgentRunEventReadQuery,
    ) -> Result<Vec<AgentRunEvent>, DomainError> {
        let limit = query.limit.clamp(1, 500);
        let mut events = self.read_all_events(run_id).await?;

        if let Some(invocation_id) = query.invocation_id.as_deref() {
            events.retain(|event| event_belongs_to_invocation(event, invocation_id));
        }

        if let Some(before_seq) = query.before_seq {
            events.retain(|event| event.seq < before_seq);
            let start = events.len().saturating_sub(limit);
            return Ok(events.into_iter().skip(start).collect());
        }

        if let Some(after_seq) = query.after_seq {
            events.retain(|event| event.seq > after_seq);
        }

        events.truncate(limit);
        Ok(events)
    }

    async fn read_all_events(&self, run_id: &str) -> Result<Vec<AgentRunEvent>, DomainError> {
        let store = self.store.lock().await;
        if !store.runs.contains_key(run_id) {
            return Err(DomainError::NotFound(format!(
                "Agent run not found: {run_id}"
            )));
        }
        Ok(store.events.get(run_id).cloned().unwrap_or_default())
    }
}

fn unused_settings_method(name: &str) -> DomainError {
    DomainError::InternalError(format!("unused test settings repository method: {name}"))
}

fn add_storage_file(stats: &mut AgentRunStorageEntryStats, bytes: u64) {
    stats.file_count = stats
        .file_count
        .checked_add(1)
        .expect("test storage file count overflow");
    stats.byte_count = stats
        .byte_count
        .checked_add(bytes)
        .expect("test storage byte count overflow");
}

fn next_event_seq(events: &[AgentRunEvent]) -> u64 {
    events.last().map(|event| event.seq + 1).unwrap_or(1)
}

fn run_matches_list_query(run: &AgentRun, query: &AgentRunListQuery) -> bool {
    if query
        .chat_ref
        .as_ref()
        .is_some_and(|chat_ref| &run.chat_ref != chat_ref)
    {
        return false;
    }
    if query
        .stable_chat_id
        .as_ref()
        .is_some_and(|stable_chat_id| &run.stable_chat_id != stable_chat_id)
    {
        return false;
    }
    if query
        .statuses
        .as_ref()
        .is_some_and(|statuses| !statuses.contains(&run.status))
    {
        return false;
    }
    if query
        .before
        .as_ref()
        .is_some_and(|cursor| !run_is_before_cursor(run, cursor))
    {
        return false;
    }
    true
}

fn sort_runs_newest_first(runs: &mut [AgentRun]) {
    runs.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
}

fn run_is_before_cursor(run: &AgentRun, cursor: &AgentRunListCursor) -> bool {
    run.created_at < cursor.created_at
        || (run.created_at == cursor.created_at && run.id.as_str() < cursor.run_id.as_str())
}
