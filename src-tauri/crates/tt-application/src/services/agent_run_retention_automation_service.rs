use std::sync::Arc;

use tokio::sync::Notify;
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;

use crate::dto::agent_dto::{AgentApplyRunPruneDto, AgentRunPruneRetentionDto};
use crate::errors::ApplicationError;
use crate::services::agent_run_history_service::AgentRunHistoryService;
use tt_ports::repositories::settings_repository::SettingsRepository;

const AGENT_RUN_RETENTION_AUTO_COLD_START_DELAY_SECS: u64 = 45;
const AGENT_RUN_RETENTION_AUTO_INTERVAL_SECS: u64 = 30 * 60;
const AGENT_RUN_RETENTION_AUTO_RETRY_DELAY_SECS: u64 = 60;

pub struct AgentRunRetentionAutomationService {
    settings_repository: Arc<dyn SettingsRepository>,
    run_history_service: Arc<AgentRunHistoryService>,
    notify: Notify,
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
        }
    }

    pub async fn run(self: Arc<Self>, cancel: CancellationToken) {
        self.scheduler_loop(cancel).await;
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

    async fn scheduler_loop(self: Arc<Self>, cancel: CancellationToken) {
        let mut delay = Duration::from_secs(AGENT_RUN_RETENTION_AUTO_COLD_START_DELAY_SECS);

        loop {
            if cancel.is_cancelled() {
                break;
            }

            let enabled = match self.auto_prune_enabled().await {
                Ok(enabled) => enabled,
                Err(error) => {
                    tracing::warn!("Failed to load Agent run retention settings: {}", error);
                    tokio::select! {
                        _ = sleep(Duration::from_secs(AGENT_RUN_RETENTION_AUTO_RETRY_DELAY_SECS)) => {}
                        _ = cancel.cancelled() => break,
                    }
                    continue;
                }
            };

            if !enabled {
                tokio::select! {
                    _ = self.notify.notified() => {}
                    _ = cancel.cancelled() => break,
                }
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
                _ = cancel.cancelled() => break,
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

    use super::*;
    use crate::services::agent_run_retention_test_support::{
        TestAgentRunRepository, TestSettingsRepository,
    };
    use crate::services::agent_workspace_lifecycle_service::AgentRunActivity;
    use tt_domain::models::agent::{
        AgentChatRef, AgentRun, AgentRunPresentation, AgentRunSkillScopeRefs, AgentRunStatus,
    };
    use tt_domain::models::settings::{
        AgentRunRetentionSettings, AgentSettings, TauriTavernSettings,
    };
    use tt_ports::repositories::agent_run_repository::AgentRunRepository;

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

    fn build_service(
        settings_repository: Arc<TestSettingsRepository>,
        run_repository: Arc<TestAgentRunRepository>,
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
        let run_repository = TestAgentRunRepository::new();
        let settings_repository = TestSettingsRepository::new();

        let service = build_service(settings_repository, run_repository);
        let ran = service
            .run_once_if_enabled()
            .await
            .expect("run once should load default settings");

        assert!(!ran);
    }

    #[tokio::test]
    async fn run_once_applies_retention_when_auto_prune_is_enabled() {
        let run_repository = TestAgentRunRepository::new();
        let settings_repository = TestSettingsRepository::new();

        let mut settings = TauriTavernSettings::default();
        settings.agent = AgentSettings {
            retention: AgentRunRetentionSettings {
                auto_prune_enabled: true,
                keep_recent_terminal_runs: 0,
                keep_full_recent_runs: 0,
            },
        };
        settings_repository
            .store_tauritavern_settings(settings)
            .await;

        let run = completed_run("run_auto_prune_delete");
        run_repository.create_run(&run).await.expect("create run");
        run_repository.append_terminal_event_for_run(&run).await;
        run_repository.add_heavy_artifact(&run, 5).await;

        let service = build_service(settings_repository, run_repository.clone());
        let ran = service
            .run_once_if_enabled()
            .await
            .expect("run once should apply retention");

        assert!(ran);
        assert!(!run_repository.has_run(&run.id).await);
    }
}
