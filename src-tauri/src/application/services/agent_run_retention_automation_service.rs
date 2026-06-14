use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Notify;
use tokio::time::{Duration, sleep};

use crate::application::dto::agent_dto::{AgentApplyRunPruneDto, AgentRunPruneRetentionDto};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_run_history_service::AgentRunHistoryService;
use crate::domain::repositories::settings_repository::SettingsRepository;

const AGENT_RUN_RETENTION_AUTO_COLD_START_DELAY_SECS: u64 = 45;
const AGENT_RUN_RETENTION_AUTO_INTERVAL_SECS: u64 = 30 * 60;
const AGENT_RUN_RETENTION_AUTO_RETRY_DELAY_SECS: u64 = 60;

pub struct AgentRunRetentionAutomationService {
    settings_repository: Arc<dyn SettingsRepository>,
    run_history_service: Arc<AgentRunHistoryService>,
    notify: Notify,
    started: AtomicBool,
}

impl AgentRunRetentionAutomationService {
    pub fn new(
        settings_repository: Arc<dyn SettingsRepository>,
        run_history_service: Arc<AgentRunHistoryService>,
    ) -> Self {
        Self {
            settings_repository,
            run_history_service,
            notify: Notify::new(),
            started: AtomicBool::new(false),
        }
    }

    pub fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }

        let service = self.clone();
        tauri::async_runtime::spawn(async move {
            service.scheduler_loop().await;
        });
    }

    pub fn notify_settings_changed(&self) {
        self.notify.notify_waiters();
    }

    async fn run_once_if_enabled(&self) -> Result<bool, ApplicationError> {
        let retention = self
            .settings_repository
            .load_tauritavern_settings()
            .await?
            .agent
            .retention;
        if !retention.auto_prune_enabled {
            return Ok(false);
        }

        let Some(result) = self
            .run_history_service
            .try_apply_run_prune_for_automation(AgentApplyRunPruneDto {
                retention: Some(AgentRunPruneRetentionDto::from(retention)),
                detail_limit: 0,
            })
            .await?
        else {
            tracing::debug!(
                "Agent run auto cleanup skipped because prune apply is already running"
            );
            return Ok(false);
        };
        if result.removed_file_count > 0 || result.failed_run_count > 0 {
            tracing::info!(
                slimmed_run_count = result.slimmed_run_count,
                deleted_run_count = result.deleted_run_count,
                failed_run_count = result.failed_run_count,
                removed_file_count = result.removed_file_count,
                removed_byte_count = result.removed_byte_count,
                "Agent run auto cleanup completed"
            );
        }

        Ok(true)
    }

    async fn scheduler_loop(self: Arc<Self>) {
        let mut delay = Duration::from_secs(AGENT_RUN_RETENTION_AUTO_COLD_START_DELAY_SECS);

        loop {
            let enabled = match self.auto_prune_enabled().await {
                Ok(enabled) => enabled,
                Err(error) => {
                    tracing::warn!("Failed to load Agent run retention settings: {}", error);
                    sleep(Duration::from_secs(
                        AGENT_RUN_RETENTION_AUTO_RETRY_DELAY_SECS,
                    ))
                    .await;
                    continue;
                }
            };

            if !enabled {
                self.notify.notified().await;
                delay = Duration::from_secs(AGENT_RUN_RETENTION_AUTO_COLD_START_DELAY_SECS);
                continue;
            }

            let wait = sleep(delay);
            tokio::pin!(wait);

            tokio::select! {
                _ = &mut wait => {}
                _ = self.notify.notified() => {
                    delay = Duration::from_secs(AGENT_RUN_RETENTION_AUTO_COLD_START_DELAY_SECS);
                    continue;
                }
            }

            match self.run_once_if_enabled().await {
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!("Agent run auto cleanup failed: {}", error);
                }
            }

            delay = Duration::from_secs(AGENT_RUN_RETENTION_AUTO_INTERVAL_SECS);
        }
    }

    async fn auto_prune_enabled(&self) -> Result<bool, ApplicationError> {
        Ok(self
            .settings_repository
            .load_tauritavern_settings()
            .await?
            .agent
            .retention
            .auto_prune_enabled)
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use serde_json::Value;
    use std::path::PathBuf;
    use tokio::fs;

    use super::*;
    use crate::application::services::agent_workspace_lifecycle_service::AgentRunActivity;
    use crate::domain::models::agent::{
        AgentChatRef, AgentRun, AgentRunEventLevel, AgentRunPresentation, AgentRunSkillScopeRefs,
        AgentRunStatus,
    };
    use crate::domain::models::settings::{
        AgentRunRetentionSettings, AgentSettings, TauriTavernSettings,
    };
    use crate::domain::repositories::agent_run_repository::AgentRunRepository;
    use crate::domain::repositories::settings_repository::SettingsRepository;
    use crate::infrastructure::repositories::file_agent_repository::FileAgentRepository;
    use crate::infrastructure::repositories::file_settings_repository::FileSettingsRepository;

    struct TestRunActivity;

    #[async_trait]
    impl AgentRunActivity for TestRunActivity {
        async fn active_run_ids(&self) -> Result<Vec<String>, ApplicationError> {
            Ok(Vec::new())
        }

        async fn active_run_ids_for_workspace(
            &self,
            _workspace_id: &str,
        ) -> Result<Vec<String>, ApplicationError> {
            Ok(Vec::new())
        }
    }

    fn instant(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("valid timestamp")
            .with_timezone(&Utc)
    }

    fn temp_root(label: &str) -> PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "tauritavern-agent-run-retention-auto-{label}-{}-{suffix}",
            std::process::id()
        ))
    }

    fn completed_run(id: &str) -> AgentRun {
        AgentRun {
            id: id.to_string(),
            workspace_id: "chat_auto_prune".to_string(),
            stable_chat_id: "stable_auto_prune".to_string(),
            chat_ref: AgentChatRef::Character {
                character_id: "Seraphina".to_string(),
                file_name: "Seraphina.png".to_string(),
            },
            generation_type: "normal".to_string(),
            profile_id: None,
            skill_scope_refs: AgentRunSkillScopeRefs::default(),
            persist_base_state_id: None,
            input_message_count: Some(1),
            presentation: AgentRunPresentation::Background,
            status: AgentRunStatus::Completed,
            created_at: instant("2026-01-01T00:00:00Z"),
            updated_at: instant("2026-01-01T00:05:00Z"),
        }
    }

    async fn seed_heavy_file(root: &std::path::Path, run: &AgentRun) {
        let input_dir = root
            .join("chats")
            .join(&run.workspace_id)
            .join("runs")
            .join(&run.id)
            .join("input");
        fs::create_dir_all(&input_dir)
            .await
            .expect("create heavy input dir");
        fs::write(input_dir.join("prompt_snapshot.json"), b"heavy")
            .await
            .expect("write heavy input file");
    }

    async fn append_terminal_event(repository: &FileAgentRepository, run: &AgentRun) {
        repository
            .append_event(
                run.id.as_str(),
                AgentRunEventLevel::Info,
                "run_completed",
                Value::Null,
            )
            .await
            .expect("append terminal event");
    }

    fn build_service(
        settings_repository: Arc<FileSettingsRepository>,
        run_repository: Arc<FileAgentRepository>,
    ) -> Arc<AgentRunRetentionAutomationService> {
        let history_service = Arc::new(AgentRunHistoryService::new(
            run_repository,
            settings_repository.clone(),
            Arc::new(TestRunActivity),
        ));
        Arc::new(AgentRunRetentionAutomationService::new(
            settings_repository,
            history_service,
        ))
    }

    #[tokio::test]
    async fn run_once_skips_when_auto_prune_is_disabled() {
        let agent_root = temp_root("agent-disabled");
        let settings_root = temp_root("settings-disabled");
        let run_repository = Arc::new(FileAgentRepository::new(agent_root.clone()));
        let settings_repository = Arc::new(FileSettingsRepository::new(settings_root.clone()));

        let service = build_service(settings_repository, run_repository);
        let ran = service
            .run_once_if_enabled()
            .await
            .expect("run once should load default settings");

        assert!(!ran);

        let _ = fs::remove_dir_all(agent_root).await;
        let _ = fs::remove_dir_all(settings_root).await;
    }

    #[tokio::test]
    async fn run_once_applies_retention_when_auto_prune_is_enabled() {
        let agent_root = temp_root("agent-enabled");
        let settings_root = temp_root("settings-enabled");
        let run_repository = Arc::new(FileAgentRepository::new(agent_root.clone()));
        let settings_repository = Arc::new(FileSettingsRepository::new(settings_root.clone()));

        let mut settings = TauriTavernSettings::default();
        settings.agent = AgentSettings {
            retention: AgentRunRetentionSettings {
                auto_prune_enabled: true,
                keep_recent_terminal_runs: 0,
                keep_full_recent_runs: 0,
            },
        };
        settings_repository
            .save_tauritavern_settings(&settings)
            .await
            .expect("save settings");

        let run = completed_run("run_auto_prune_delete");
        run_repository.create_run(&run).await.expect("create run");
        append_terminal_event(&run_repository, &run).await;
        seed_heavy_file(&agent_root, &run).await;

        let service = build_service(settings_repository, run_repository);
        let ran = service
            .run_once_if_enabled()
            .await
            .expect("run once should apply retention");

        assert!(ran);
        assert!(
            !agent_root
                .join("chats")
                .join(&run.workspace_id)
                .join("runs")
                .join(&run.id)
                .exists()
        );

        let _ = fs::remove_dir_all(agent_root).await;
        let _ = fs::remove_dir_all(settings_root).await;
    }
}
