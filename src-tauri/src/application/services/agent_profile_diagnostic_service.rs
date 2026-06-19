use std::sync::Arc;

use crate::application::errors::ApplicationError;
use crate::application::services::agent_profile_service::{
    AgentProfileResolveInput, AgentProfileService, preset_exists_for_type,
};
use crate::application::services::llm_connection_service::LlmConnectionService;
use crate::domain::models::agent::AgentToolSpec;
use crate::domain::models::agent::profile::{
    AgentModelBindingMode, AgentPresetBindingMode, AgentPresetRef, AgentProfileId,
    ResolvedAgentProfile,
};
use crate::domain::models::agent::profile_diagnostic::{
    AgentProfileDiagnostic, AgentProfileDiagnosticBlock, AgentProfileDiagnosticRepairAction,
    AgentProfileDiagnosticResource, AgentProfileDiagnosticResourceKind,
    AgentProfileDiagnosticSeverity, AgentProfileHealth,
};
use crate::domain::models::preset::PresetType;
use crate::domain::repositories::preset_repository::PresetRepository;

pub struct AgentProfileDiagnosticService {
    profile_service: Arc<AgentProfileService>,
    preset_repository: Arc<dyn PresetRepository>,
    llm_connection_service: Arc<LlmConnectionService>,
}

impl AgentProfileDiagnosticService {
    pub fn new(
        profile_service: Arc<AgentProfileService>,
        preset_repository: Arc<dyn PresetRepository>,
        llm_connection_service: Arc<LlmConnectionService>,
    ) -> Self {
        Self {
            profile_service,
            preset_repository,
            llm_connection_service,
        }
    }

    pub async fn diagnose_profile(
        &self,
        profile_id: &str,
        known_tools: &[AgentToolSpec],
    ) -> Result<AgentProfileHealth, ApplicationError> {
        let profile_id =
            AgentProfileId::parse(profile_id).map_err(ApplicationError::ValidationError)?;
        let profile = match self
            .profile_service
            .resolve_profile_for_preview(AgentProfileResolveInput {
                profile_id: Some(profile_id.as_str()),
                known_tools,
            })
            .await
        {
            Ok(profile) => profile,
            Err(ApplicationError::ValidationError(message)) => {
                return Ok(AgentProfileHealth {
                    profile_id,
                    preview_available: false,
                    prompt_assembly_available: false,
                    direct_run_available: false,
                    sub_agent_available: false,
                    diagnostics: vec![profile_contract_invalid_diagnostic(message)],
                });
            }
            Err(error) => return Err(error),
        };

        let mut diagnostics = Vec::new();
        self.diagnose_preset_binding(&profile, &mut diagnostics)
            .await?;
        self.diagnose_model_binding(&profile, &mut diagnostics)
            .await?;

        Ok(AgentProfileHealth {
            profile_id: profile.id.clone(),
            preview_available: true,
            prompt_assembly_available: !blocks(
                &diagnostics,
                AgentProfileDiagnosticBlock::PromptAssembly,
            ),
            direct_run_available: profile.run.direct_runnable
                && !blocks(&diagnostics, AgentProfileDiagnosticBlock::DirectRun),
            sub_agent_available: profile.delegation.callable
                && profile.delegation.allow_as_subagent
                && !blocks(&diagnostics, AgentProfileDiagnosticBlock::SubAgent),
            diagnostics,
        })
    }

    async fn diagnose_preset_binding(
        &self,
        profile: &ResolvedAgentProfile,
        diagnostics: &mut Vec<AgentProfileDiagnostic>,
    ) -> Result<(), ApplicationError> {
        if profile.preset.mode != AgentPresetBindingMode::Ref {
            return Ok(());
        }
        let ref_ = profile
            .preset
            .ref_
            .as_ref()
            .expect("validated preset ref binding must include preset.ref");
        let preset_type = PresetType::from_api_id(ref_.api_id.as_str())
            .expect("validated preset ref binding must include a supported preset apiId");
        if preset_type != PresetType::OpenAI {
            diagnostics.push(preset_api_unsupported_diagnostic(ref_));
            return Ok(());
        }

        if !preset_exists_for_type(
            self.preset_repository.as_ref(),
            ref_.name.as_str(),
            &preset_type,
        )
        .await?
        {
            diagnostics.push(preset_missing_diagnostic(ref_));
        }
        Ok(())
    }

    async fn diagnose_model_binding(
        &self,
        profile: &ResolvedAgentProfile,
        diagnostics: &mut Vec<AgentProfileDiagnostic>,
    ) -> Result<(), ApplicationError> {
        match profile.model.mode {
            AgentModelBindingMode::CurrentPromptSnapshot => Ok(()),
            AgentModelBindingMode::RequiresConfiguration => {
                diagnostics.push(model_requires_configuration_diagnostic(profile));
                Ok(())
            }
            AgentModelBindingMode::ConnectionRef => {
                let connection_ref = profile
                    .model
                    .connection_ref
                    .as_deref()
                    .expect("validated connectionRef binding must include model.connectionRef")
                    .trim();
                let model_id = profile
                    .model
                    .model_id
                    .as_deref()
                    .expect("validated connectionRef binding must include model.modelId")
                    .trim();
                match self
                    .llm_connection_service
                    .resolve_model_binding(connection_ref, model_id)
                    .await
                {
                    Ok(_) => Ok(()),
                    Err(ApplicationError::NotFound(message)) => {
                        diagnostics.push(model_connection_missing_diagnostic(
                            profile,
                            connection_ref,
                            model_id,
                            message,
                        ));
                        Ok(())
                    }
                    Err(ApplicationError::ValidationError(message)) => {
                        diagnostics.push(model_connection_invalid_diagnostic(
                            profile,
                            connection_ref,
                            model_id,
                            message,
                        ));
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            }
        }
    }
}

fn profile_contract_invalid_diagnostic(message: String) -> AgentProfileDiagnostic {
    AgentProfileDiagnostic {
        code: "agent.profile_contract_invalid".to_string(),
        severity: AgentProfileDiagnosticSeverity::Error,
        path: "$".to_string(),
        message,
        resource: None,
        blocks: vec![
            AgentProfileDiagnosticBlock::Preview,
            AgentProfileDiagnosticBlock::PromptAssembly,
            AgentProfileDiagnosticBlock::DirectRun,
            AgentProfileDiagnosticBlock::SubAgent,
        ],
        repair_actions: vec![AgentProfileDiagnosticRepairAction::OpenJsonEditor],
    }
}

fn preset_api_unsupported_diagnostic(ref_: &AgentPresetRef) -> AgentProfileDiagnostic {
    AgentProfileDiagnostic {
        code: "agent.profile_preset_api_unsupported".to_string(),
        severity: AgentProfileDiagnosticSeverity::Error,
        path: "$.preset.ref.apiId".to_string(),
        message: format!(
            "agent.profile_preset_api_unsupported: independent Agent prompt assembly currently requires an openai preset, got `{}`",
            ref_.api_id
        ),
        resource: Some(preset_resource(ref_)),
        blocks: run_and_prompt_assembly_blocks(),
        repair_actions: vec![
            AgentProfileDiagnosticRepairAction::SelectPreset,
            AgentProfileDiagnosticRepairAction::OpenJsonEditor,
        ],
    }
}

fn preset_missing_diagnostic(ref_: &AgentPresetRef) -> AgentProfileDiagnostic {
    AgentProfileDiagnostic {
        code: "agent.profile_preset_missing".to_string(),
        severity: AgentProfileDiagnosticSeverity::Error,
        path: "$.preset.ref.name".to_string(),
        message: format!(
            "agent.profile_preset_missing: required preset `{}` for apiId `{}` does not exist",
            ref_.name, ref_.api_id
        ),
        resource: Some(preset_resource(ref_)),
        blocks: run_and_prompt_assembly_blocks(),
        repair_actions: vec![
            AgentProfileDiagnosticRepairAction::SelectPreset,
            AgentProfileDiagnosticRepairAction::OpenJsonEditor,
        ],
    }
}

fn model_requires_configuration_diagnostic(
    profile: &ResolvedAgentProfile,
) -> AgentProfileDiagnostic {
    AgentProfileDiagnostic {
        code: "agent.profile_model_requires_configuration".to_string(),
        severity: AgentProfileDiagnosticSeverity::Error,
        path: "$.model.mode".to_string(),
        message: format!(
            "agent.profile_model_requires_configuration: Agent profile `{}` requires a local model selection before it can run",
            profile.id.as_str()
        ),
        resource: Some(AgentProfileDiagnosticResource {
            kind: AgentProfileDiagnosticResourceKind::Model,
            api_id: None,
            name: None,
            id: None,
            model_id: None,
        }),
        blocks: run_and_prompt_assembly_blocks(),
        repair_actions: vec![AgentProfileDiagnosticRepairAction::SelectModel],
    }
}

fn model_connection_missing_diagnostic(
    profile: &ResolvedAgentProfile,
    connection_ref: &str,
    model_id: &str,
    message: String,
) -> AgentProfileDiagnostic {
    AgentProfileDiagnostic {
        code: "agent.profile_model_connection_missing".to_string(),
        severity: AgentProfileDiagnosticSeverity::Error,
        path: "$.model.connectionRef".to_string(),
        message,
        resource: Some(model_connection_resource(connection_ref, model_id)),
        blocks: model_binding_blocks(profile),
        repair_actions: vec![
            AgentProfileDiagnosticRepairAction::SelectModel,
            AgentProfileDiagnosticRepairAction::SetModelRequiresConfiguration,
        ],
    }
}

fn model_connection_invalid_diagnostic(
    profile: &ResolvedAgentProfile,
    connection_ref: &str,
    model_id: &str,
    message: String,
) -> AgentProfileDiagnostic {
    AgentProfileDiagnostic {
        code: "agent.profile_model_connection_invalid".to_string(),
        severity: AgentProfileDiagnosticSeverity::Error,
        path: "$.model.connectionRef".to_string(),
        message,
        resource: Some(model_connection_resource(connection_ref, model_id)),
        blocks: model_binding_blocks(profile),
        repair_actions: vec![
            AgentProfileDiagnosticRepairAction::SelectModel,
            AgentProfileDiagnosticRepairAction::SetModelRequiresConfiguration,
        ],
    }
}

fn preset_resource(ref_: &AgentPresetRef) -> AgentProfileDiagnosticResource {
    AgentProfileDiagnosticResource {
        kind: AgentProfileDiagnosticResourceKind::Preset,
        api_id: Some(ref_.api_id.clone()),
        name: Some(ref_.name.clone()),
        id: None,
        model_id: None,
    }
}

fn model_connection_resource(
    connection_ref: &str,
    model_id: &str,
) -> AgentProfileDiagnosticResource {
    AgentProfileDiagnosticResource {
        kind: AgentProfileDiagnosticResourceKind::LlmConnection,
        api_id: None,
        name: None,
        id: Some(connection_ref.to_string()),
        model_id: Some(model_id.to_string()),
    }
}

fn run_and_prompt_assembly_blocks() -> Vec<AgentProfileDiagnosticBlock> {
    vec![
        AgentProfileDiagnosticBlock::PromptAssembly,
        AgentProfileDiagnosticBlock::DirectRun,
        AgentProfileDiagnosticBlock::SubAgent,
    ]
}

fn model_binding_blocks(profile: &ResolvedAgentProfile) -> Vec<AgentProfileDiagnosticBlock> {
    let mut blocks = vec![
        AgentProfileDiagnosticBlock::DirectRun,
        AgentProfileDiagnosticBlock::SubAgent,
    ];
    if profile.preset.mode == AgentPresetBindingMode::Ref {
        blocks.insert(0, AgentProfileDiagnosticBlock::PromptAssembly);
    }
    blocks
}

fn blocks(diagnostics: &[AgentProfileDiagnostic], block: AgentProfileDiagnosticBlock) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.blocks.contains(&block))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::sync::Arc;

    use async_trait::async_trait;
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::application::services::agent_tools::BuiltinAgentToolRegistry;
    use crate::domain::errors::DomainError;
    use crate::domain::models::agent::profile::{
        AgentModelBindingMode, AgentPresetBindingMode, AgentPresetRef,
    };
    use crate::domain::models::llm_connection::{
        LlmConnectionDefinition, LlmConnectionId, LlmConnectionSummary,
    };
    use crate::domain::models::preset::{DefaultPreset, Preset};
    use crate::domain::repositories::agent_profile_repository::AgentProfileRepository;
    use crate::domain::repositories::agent_profile_storage_health_repository::AgentProfileStorageHealthRepository;
    use crate::domain::repositories::llm_connection_repository::LlmConnectionRepository;
    use crate::infrastructure::repositories::file_agent_profile_repository::FileAgentProfileRepository;

    #[tokio::test]
    async fn default_profile_is_healthy() {
        let root = temp_root("healthy-default");
        let (profile_service, diagnostic_service, registry) = test_services(
            &root,
            TestPresetRepository::default(),
            TestLlmConnectionRepository::default(),
        );

        let health = diagnostic_service
            .diagnose_profile("default-writer", registry.specs())
            .await
            .expect("diagnose default profile");

        assert!(health.preview_available);
        assert!(health.prompt_assembly_available);
        assert!(health.direct_run_available);
        assert!(health.diagnostics.is_empty());

        cleanup(root).await;
        drop(profile_service);
    }

    #[tokio::test]
    async fn dangling_openai_preset_is_diagnostic_not_preview_failure() {
        let root = temp_root("dangling-preset");
        let (profile_service, diagnostic_service, registry) = test_services(
            &root,
            TestPresetRepository::default(),
            TestLlmConnectionRepository::default(),
        );
        let mut profile = profile_service
            .load_profile("default-writer")
            .await
            .expect("load default")
            .expect("default profile");
        profile.id =
            crate::domain::models::agent::profile::AgentProfileId::parse("dangling-writer")
                .expect("profile id");
        profile.preset.mode = AgentPresetBindingMode::Ref;
        profile.preset.ref_ = Some(AgentPresetRef {
            api_id: "openai".to_string(),
            name: "Missing Writer Preset".to_string(),
        });
        profile.preset.required = true;
        profile_service
            .save_profile(profile, registry.specs())
            .await
            .expect("dangling preset profile remains editable");

        let health = diagnostic_service
            .diagnose_profile("dangling-writer", registry.specs())
            .await
            .expect("diagnose dangling profile");

        assert!(health.preview_available);
        assert!(!health.prompt_assembly_available);
        assert!(!health.direct_run_available);
        let diagnostic = only_diagnostic(&health);
        assert_eq!(diagnostic.code, "agent.profile_preset_missing");
        assert!(
            diagnostic
                .blocks
                .contains(&AgentProfileDiagnosticBlock::PromptAssembly)
        );
        assert!(
            diagnostic
                .repair_actions
                .contains(&AgentProfileDiagnosticRepairAction::SelectPreset)
        );

        cleanup(root).await;
    }

    #[tokio::test]
    async fn non_openai_preset_ref_is_diagnostic_for_independent_prompt_assembly() {
        let root = temp_root("unsupported-preset-api");
        let (profile_service, diagnostic_service, registry) = test_services(
            &root,
            TestPresetRepository::default(),
            TestLlmConnectionRepository::default(),
        );
        let mut profile = profile_service
            .load_profile("default-writer")
            .await
            .expect("load default")
            .expect("default profile");
        profile.id = crate::domain::models::agent::profile::AgentProfileId::parse("textgen-writer")
            .expect("profile id");
        profile.preset.mode = AgentPresetBindingMode::Ref;
        profile.preset.ref_ = Some(AgentPresetRef {
            api_id: "textgenerationwebui".to_string(),
            name: "TextGen Preset".to_string(),
        });
        profile.preset.required = true;
        profile_service
            .save_profile(profile, registry.specs())
            .await
            .expect("supported preset type remains editable even if assembly cannot use it");

        let health = diagnostic_service
            .diagnose_profile("textgen-writer", registry.specs())
            .await
            .expect("diagnose textgen preset profile");

        assert!(health.preview_available);
        assert!(!health.prompt_assembly_available);
        let diagnostic = only_diagnostic(&health);
        assert_eq!(diagnostic.code, "agent.profile_preset_api_unsupported");
        assert_eq!(
            diagnostic
                .resource
                .as_ref()
                .and_then(|resource| resource.api_id.as_deref()),
            Some("textgenerationwebui")
        );

        cleanup(root).await;
    }

    #[tokio::test]
    async fn requires_configuration_is_diagnostic_and_blocks_run() {
        let root = temp_root("requires-configuration");
        let (profile_service, diagnostic_service, registry) = test_services(
            &root,
            TestPresetRepository::default(),
            TestLlmConnectionRepository::default(),
        );
        let mut profile = profile_service
            .load_profile("default-writer")
            .await
            .expect("load default")
            .expect("default profile");
        profile.id =
            crate::domain::models::agent::profile::AgentProfileId::parse("imported-writer")
                .expect("profile id");
        profile.model.mode = AgentModelBindingMode::RequiresConfiguration;
        profile.model.connection_ref = None;
        profile.model.model_id = None;
        profile_service
            .save_profile(profile, registry.specs())
            .await
            .expect("requiresConfiguration profile remains editable");

        let health = diagnostic_service
            .diagnose_profile("imported-writer", registry.specs())
            .await
            .expect("diagnose imported profile");

        assert!(health.preview_available);
        assert!(!health.prompt_assembly_available);
        assert!(!health.direct_run_available);
        let diagnostic = only_diagnostic(&health);
        assert_eq!(
            diagnostic.code,
            "agent.profile_model_requires_configuration"
        );
        assert!(
            diagnostic
                .repair_actions
                .contains(&AgentProfileDiagnosticRepairAction::SelectModel)
        );

        cleanup(root).await;
    }

    #[tokio::test]
    async fn missing_connection_blocks_run_without_blocking_current_snapshot_prompt_assembly() {
        let root = temp_root("missing-connection");
        let (profile_service, diagnostic_service, registry) = test_services(
            &root,
            TestPresetRepository::default(),
            TestLlmConnectionRepository::default(),
        );
        let mut profile = profile_service
            .load_profile("default-writer")
            .await
            .expect("load default")
            .expect("default profile");
        profile.id =
            crate::domain::models::agent::profile::AgentProfileId::parse("connection-writer")
                .expect("profile id");
        profile.model.mode = AgentModelBindingMode::ConnectionRef;
        profile.model.connection_ref = Some("missing-connection".to_string());
        profile.model.model_id = Some("test-model".to_string());
        profile_service
            .save_profile(profile, registry.specs())
            .await
            .expect("missing connection profile remains editable");

        let health = diagnostic_service
            .diagnose_profile("connection-writer", registry.specs())
            .await
            .expect("diagnose connection profile");

        assert!(health.preview_available);
        assert!(health.prompt_assembly_available);
        assert!(!health.direct_run_available);
        let diagnostic = only_diagnostic(&health);
        assert_eq!(diagnostic.code, "agent.profile_model_connection_missing");
        assert!(
            !diagnostic
                .blocks
                .contains(&AgentProfileDiagnosticBlock::PromptAssembly)
        );
        assert!(
            diagnostic
                .blocks
                .contains(&AgentProfileDiagnosticBlock::DirectRun)
        );

        cleanup(root).await;
    }

    fn test_services(
        root: &Path,
        preset_repository: TestPresetRepository,
        llm_connection_repository: TestLlmConnectionRepository,
    ) -> (
        Arc<AgentProfileService>,
        AgentProfileDiagnosticService,
        BuiltinAgentToolRegistry,
    ) {
        let profile_repository =
            Arc::new(FileAgentProfileRepository::new(root.join("agent-profiles")));
        let profile_repository_trait: Arc<dyn AgentProfileRepository> = profile_repository.clone();
        let profile_health_repository: Arc<dyn AgentProfileStorageHealthRepository> =
            profile_repository;
        let preset_repository: Arc<dyn PresetRepository> = Arc::new(preset_repository);
        let llm_connection_service = Arc::new(LlmConnectionService::new(Arc::new(
            llm_connection_repository,
        )));
        let profile_service = Arc::new(AgentProfileService::new(
            profile_repository_trait,
            profile_health_repository,
            preset_repository.clone(),
        ));
        let diagnostic_service = AgentProfileDiagnosticService::new(
            profile_service.clone(),
            preset_repository,
            llm_connection_service,
        );
        (
            profile_service,
            diagnostic_service,
            BuiltinAgentToolRegistry::phase2c(),
        )
    }

    fn only_diagnostic(health: &AgentProfileHealth) -> &AgentProfileDiagnostic {
        assert_eq!(health.diagnostics.len(), 1);
        &health.diagnostics[0]
    }

    fn temp_root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "tauritavern-agent-profile-diagnostic-{label}-{}",
            Uuid::new_v4().simple()
        ))
    }

    async fn cleanup(root: std::path::PathBuf) {
        match tokio::fs::remove_dir_all(root).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("cleanup: {error}"),
        }
    }

    #[derive(Default)]
    struct TestPresetRepository {
        user_openai: BTreeSet<String>,
        default_openai: BTreeSet<String>,
    }

    #[async_trait]
    impl PresetRepository for TestPresetRepository {
        async fn save_preset(&self, _preset: &Preset) -> Result<(), DomainError> {
            Ok(())
        }

        async fn delete_preset(
            &self,
            _name: &str,
            _preset_type: &PresetType,
        ) -> Result<(), DomainError> {
            Ok(())
        }

        async fn preset_exists(
            &self,
            name: &str,
            preset_type: &PresetType,
        ) -> Result<bool, DomainError> {
            Ok(*preset_type == PresetType::OpenAI && self.user_openai.contains(name))
        }

        async fn get_preset(
            &self,
            name: &str,
            preset_type: &PresetType,
        ) -> Result<Option<Preset>, DomainError> {
            if *preset_type == PresetType::OpenAI && self.user_openai.contains(name) {
                return Ok(Some(Preset::new(
                    name.to_string(),
                    preset_type.clone(),
                    json!({}),
                )));
            }
            Ok(None)
        }

        async fn list_presets(&self, preset_type: &PresetType) -> Result<Vec<String>, DomainError> {
            if *preset_type != PresetType::OpenAI {
                return Ok(Vec::new());
            }
            Ok(self.user_openai.iter().cloned().collect())
        }

        async fn get_default_preset(
            &self,
            name: &str,
            preset_type: &PresetType,
        ) -> Result<Option<DefaultPreset>, DomainError> {
            if *preset_type == PresetType::OpenAI && self.default_openai.contains(name) {
                return Ok(Some(DefaultPreset {
                    filename: format!("{name}.json"),
                    name: name.to_string(),
                    preset_type: preset_type.clone(),
                    is_default: true,
                    data: json!({}),
                }));
            }
            Ok(None)
        }
    }

    #[derive(Default)]
    struct TestLlmConnectionRepository {
        connection: Option<LlmConnectionDefinition>,
    }

    #[async_trait]
    impl LlmConnectionRepository for TestLlmConnectionRepository {
        async fn list_connections(&self) -> Result<Vec<LlmConnectionSummary>, DomainError> {
            Ok(self
                .connection
                .as_ref()
                .map(|connection| vec![connection.summary()])
                .unwrap_or_default())
        }

        async fn load_connection(
            &self,
            id: &LlmConnectionId,
        ) -> Result<Option<LlmConnectionDefinition>, DomainError> {
            Ok(self
                .connection
                .as_ref()
                .filter(|connection| connection.id == *id)
                .cloned())
        }

        async fn save_connection(
            &self,
            _connection: &LlmConnectionDefinition,
        ) -> Result<(), DomainError> {
            Ok(())
        }

        async fn delete_connection(&self, _id: &LlmConnectionId) -> Result<(), DomainError> {
            Ok(())
        }
    }
}
