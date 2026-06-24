use std::sync::Arc;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::application::dto::agent_dto::{
    AgentPreparePromptAssemblyDto, AgentPreparePromptAssemblyResultDto,
    AgentPromptAssemblyBrokerRequestDto, AgentPromptAssemblyFingerprintDto,
    AgentPromptAssemblyModeDto, AgentPromptAssemblyRequestMetadataDto, AgentPromptAssemblyScopeDto,
};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_profile_service::{
    AgentProfileResolveInput, AgentProfileService, ensure_profile_model_configured,
    materialize_agent_system_prompt,
};
use crate::application::services::llm_connection_service::{
    self, LlmConnectionService, ResolvedLlmModelBinding,
};
use crate::domain::models::agent::AgentToolSpec;
use crate::domain::models::agent::profile::{
    AgentModelBindingMode, AgentPresetBindingMode, AgentPresetRef, ResolvedAgentProfile,
};
use crate::domain::models::preset::{Preset, PresetType};
use crate::domain::repositories::preset_repository::PresetRepository;

const PROMPT_ASSEMBLY_REQUEST_KIND: &str = "tauritavern.agentPromptAssemblyRequest";
const PROMPT_ASSEMBLY_REQUEST_SCHEMA_VERSION: u32 = 1;
const FROZEN_RUN_INPUT_SNAPSHOT_KIND: &str = "tauritavern.agentFrozenRunInputSnapshot";
const FROZEN_RUN_INPUT_SNAPSHOT_SCHEMA_VERSION: u32 = 1;
const CURRENT_MODEL_CONNECTION_SNAPSHOT_KIND: &str = "tauritavern.currentModelConnectionSnapshot";
const CURRENT_MODEL_CONNECTION_SNAPSHOT_SCHEMA_VERSION: u32 = 1;
const MODEL_PROMPT_SETTING_KEYS: &[&str] = &[
    "openai_model",
    "claude_model",
    "google_model",
    "vertexai_model",
    "openrouter_model",
    "ai21_model",
    "mistralai_model",
    "custom_model",
    "cohere_model",
    "perplexity_model",
    "groq_model",
    "siliconflow_model",
    "minimax_model",
    "aws_bedrock_model",
    "electronhub_model",
    "chutes_model",
    "nanogpt_model",
    "deepseek_model",
    "aimlapi_model",
    "xai_model",
    "pollinations_model",
    "cometapi_model",
    "moonshot_model",
    "fireworks_model",
    "azure_openai_model",
    "zai_model",
    "workers_ai_model",
];
const PROMPT_CONNECTION_EXTRA_SETTING_KEYS: &[&str] = &[
    "additional_parameters_by_source",
    "custom_claude_prompt_caching",
    "azure_base_url",
    "azure_deployment_name",
    "azure_api_version",
    "openrouter_use_fallback",
    "openrouter_providers",
    "openrouter_quantizations",
    "openrouter_allow_fallbacks",
    "openrouter_middleout",
];

pub struct PromptAssemblyService {
    profile_service: Arc<AgentProfileService>,
    preset_repository: Arc<dyn PresetRepository>,
    llm_connection_service: Arc<LlmConnectionService>,
}

#[derive(Debug, Clone)]
pub struct AgentInvocationPromptAssemblyContext {
    pub assembly_id: String,
    pub scope: AgentPromptAssemblyScopeDto,
    pub agent_task_prompt: Option<String>,
    pub required_agent_prompt_components: Vec<String>,
}

impl PromptAssemblyService {
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

    pub async fn resolve_profile(
        &self,
        profile_id: Option<&str>,
        known_tools: &[AgentToolSpec],
    ) -> Result<ResolvedAgentProfile, ApplicationError> {
        self.profile_service
            .resolve_profile(AgentProfileResolveInput {
                profile_id,
                known_tools,
            })
            .await
    }

    pub async fn prepare_frontend_prompt_assembly(
        &self,
        dto: AgentPreparePromptAssemblyDto,
        profile: ResolvedAgentProfile,
        visible_tools: &[AgentToolSpec],
    ) -> Result<AgentPreparePromptAssemblyResultDto, ApplicationError> {
        self.prepare_frontend_prompt_assembly_with_context(dto, profile, visible_tools, None)
            .await
    }

    pub async fn prepare_invocation_frontend_prompt_assembly(
        &self,
        dto: AgentPreparePromptAssemblyDto,
        profile: ResolvedAgentProfile,
        visible_tools: &[AgentToolSpec],
        context: AgentInvocationPromptAssemblyContext,
    ) -> Result<AgentPreparePromptAssemblyResultDto, ApplicationError> {
        self.prepare_frontend_prompt_assembly_with_context(
            dto,
            profile,
            visible_tools,
            Some(context),
        )
        .await
    }

    pub fn build_current_model_connection_snapshot(
        &self,
        settings: &Value,
        model: &str,
        secret_id: Option<&str>,
    ) -> Result<Value, ApplicationError> {
        build_current_model_connection_snapshot(settings, model, secret_id)
    }

    pub fn apply_current_model_connection_snapshot(
        &self,
        settings: Value,
        current_model_connection: &Value,
    ) -> Result<Value, ApplicationError> {
        let mut settings = settings;
        apply_current_model_connection_snapshot_to_prompt_settings(
            &mut settings,
            current_model_connection,
        )?;
        Ok(settings)
    }

    async fn prepare_frontend_prompt_assembly_with_context(
        &self,
        dto: AgentPreparePromptAssemblyDto,
        profile: ResolvedAgentProfile,
        visible_tools: &[AgentToolSpec],
        invocation_context: Option<AgentInvocationPromptAssemblyContext>,
    ) -> Result<AgentPreparePromptAssemblyResultDto, ApplicationError> {
        let generation_type = normalize_generation_type(&dto.generation_type)?;
        let frozen_run_input_snapshot =
            normalize_frozen_run_input_snapshot(&dto.frozen_run_input_snapshot, &generation_type)?;
        ensure_profile_model_configured(&profile)?;

        match profile.preset.mode {
            AgentPresetBindingMode::CurrentPromptSnapshot | AgentPresetBindingMode::None => {
                Ok(AgentPreparePromptAssemblyResultDto {
                    mode: AgentPromptAssemblyModeDto::CurrentPromptSnapshot,
                    request: None,
                    assembly: None,
                })
            }
            AgentPresetBindingMode::Ref => {
                let preset_ref = profile.preset.ref_.clone().ok_or_else(|| {
                    ApplicationError::ValidationError(
                        "prompt_assembly.profile_preset_ref_required: preset.ref is required"
                            .to_string(),
                    )
                })?;
                let preset_settings = self.load_openai_preset_settings(&preset_ref).await?;
                let (settings, model_id) = self
                    .resolve_prompt_assembly_settings(
                        &profile,
                        preset_settings.clone(),
                        &frozen_run_input_snapshot,
                    )
                    .await?;
                let fingerprint = AgentPromptAssemblyFingerprintDto {
                    preset_sha256: sha256_value(&preset_settings)?,
                    frozen_run_input_snapshot_sha256: sha256_value(&frozen_run_input_snapshot)?,
                    agent_task_prompt_sha256: invocation_context
                        .as_ref()
                        .and_then(|context| context.agent_task_prompt.as_ref())
                        .map(|prompt| sha256_string(prompt)),
                };
                let metadata = AgentPromptAssemblyRequestMetadataDto {
                    assembly_id: invocation_context
                        .as_ref()
                        .map(|context| context.assembly_id.clone()),
                    schema_version: PROMPT_ASSEMBLY_REQUEST_SCHEMA_VERSION,
                    engine: "frontend-prompt-assembly-broker".to_string(),
                    profile_id: profile.id.as_str().to_string(),
                    preset_ref: preset_ref.clone(),
                    scope: invocation_context
                        .as_ref()
                        .map(|context| context.scope.clone()),
                    fingerprint: fingerprint.clone(),
                };
                let agent_task_prompt = invocation_context
                    .as_ref()
                    .and_then(|context| context.agent_task_prompt.clone());
                let required_agent_prompt_components = invocation_context
                    .as_ref()
                    .map(|context| context.required_agent_prompt_components.clone())
                    .unwrap_or_default();

                Ok(AgentPreparePromptAssemblyResultDto {
                    mode: AgentPromptAssemblyModeDto::FrontendPromptAssembly,
                    request: Some(AgentPromptAssemblyBrokerRequestDto {
                        schema_version: PROMPT_ASSEMBLY_REQUEST_SCHEMA_VERSION,
                        kind: PROMPT_ASSEMBLY_REQUEST_KIND.to_string(),
                        assembly_id: invocation_context
                            .as_ref()
                            .map(|context| context.assembly_id.clone()),
                        scope: invocation_context
                            .as_ref()
                            .map(|context| context.scope.clone()),
                        profile_id: profile.id.as_str().to_string(),
                        generation_type,
                        frozen_run_input_snapshot,
                        settings,
                        model_id,
                        preset_ref,
                        agent_context_policy: profile.context.clone(),
                        agent_system_prompt: materialize_agent_system_prompt(
                            visible_tools,
                            &profile,
                        ),
                        agent_task_prompt,
                        required_agent_prompt_components,
                        json_schema: dto.json_schema,
                        fingerprint,
                    }),
                    assembly: Some(metadata),
                })
            }
        }
    }

    async fn load_openai_preset_settings(
        &self,
        preset_ref: &AgentPresetRef,
    ) -> Result<Value, ApplicationError> {
        let preset_type = PresetType::from_api_id(preset_ref.api_id.as_str()).ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "prompt_assembly.preset_api_invalid: unsupported preset apiId `{}`",
                preset_ref.api_id
            ))
        })?;
        if preset_type != PresetType::OpenAI {
            return Err(ApplicationError::ValidationError(format!(
                "prompt_assembly.openai_preset_required: independent Agent prompt assembly currently requires an openai preset, got `{}`",
                preset_ref.api_id
            )));
        }

        let preset = self
            .preset_repository
            .get_preset(preset_ref.name.as_str(), &preset_type)
            .await?
            .map(|preset| preset.data_with_name());
        if let Some(settings) = preset {
            ensure_json_object(
                &settings,
                "prompt_assembly.preset_data_invalid: preset data must be an object",
            )?;
            return Ok(settings);
        }

        let default_preset = self
            .preset_repository
            .get_default_preset(preset_ref.name.as_str(), &preset_type)
            .await?
            .map(|preset| {
                Preset::new(preset.name, preset.preset_type, preset.data).data_with_name()
            });
        if let Some(settings) = default_preset {
            ensure_json_object(
                &settings,
                "prompt_assembly.preset_data_invalid: preset data must be an object",
            )?;
            return Ok(settings);
        }

        Err(ApplicationError::NotFound(format!(
            "prompt_assembly.preset_not_found: preset `{}` for apiId `{}` does not exist",
            preset_ref.name, preset_ref.api_id
        )))
    }

    async fn resolve_prompt_assembly_settings(
        &self,
        profile: &ResolvedAgentProfile,
        mut settings: Value,
        frozen_run_input_snapshot: &Value,
    ) -> Result<(Value, Option<String>), ApplicationError> {
        match profile.model.mode {
            AgentModelBindingMode::CurrentPromptSnapshot => {
                apply_current_model_connection_to_prompt_settings(
                    &mut settings,
                    frozen_run_input_snapshot,
                )?;
                Ok((settings, None))
            }
            AgentModelBindingMode::RequiresConfiguration => {
                ensure_profile_model_configured(profile).map(|_| (settings, None))
            }
            AgentModelBindingMode::ConnectionRef => {
                let connection_ref = profile
                    .model
                    .connection_ref
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        ApplicationError::ValidationError(
                            "prompt_assembly.model_connection_ref_required: model.connectionRef is required"
                                .to_string(),
                        )
                    })?;
                let model_id = profile
                    .model
                    .model_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        ApplicationError::ValidationError(
                            "prompt_assembly.model_id_required: model.modelId is required"
                                .to_string(),
                        )
                    })?;
                let binding = self
                    .llm_connection_service
                    .resolve_model_binding(connection_ref, model_id)
                    .await?;
                apply_model_binding_to_prompt_settings(&mut settings, &binding)?;
                Ok((settings, Some(binding.model_id)))
            }
        }
    }
}

pub fn attach_frozen_run_input_snapshot(
    mut prompt_snapshot: Value,
    frozen_run_input_snapshot: Option<Value>,
) -> Result<Value, ApplicationError> {
    let Some(frozen_run_input_snapshot) = frozen_run_input_snapshot else {
        return Ok(prompt_snapshot);
    };
    let object = prompt_snapshot.as_object_mut().ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.prompt_snapshot_invalid: promptSnapshot must be an object".to_string(),
        )
    })?;
    object.insert(
        "frozenRunInputSnapshot".to_string(),
        normalize_frozen_run_input_snapshot(&frozen_run_input_snapshot, "")?,
    );
    Ok(prompt_snapshot)
}

fn normalize_frozen_run_input_snapshot(
    value: &Value,
    expected_generation_type: &str,
) -> Result<Value, ApplicationError> {
    let object = value.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.frozen_run_input_snapshot_required: FrozenRunInputSnapshot must be an object"
                .to_string(),
        )
    })?;
    let schema_version = object
        .get("schemaVersion")
        .or_else(|| object.get("schema_version"))
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "agent.frozen_run_input_snapshot_schema_required: schemaVersion is required"
                    .to_string(),
            )
        })?;
    if schema_version != u64::from(FROZEN_RUN_INPUT_SNAPSHOT_SCHEMA_VERSION) {
        return Err(ApplicationError::ValidationError(format!(
            "agent.frozen_run_input_snapshot_schema_unsupported: schemaVersion {schema_version} is unsupported"
        )));
    }
    let kind = string_field(object, "kind")?;
    if kind != FROZEN_RUN_INPUT_SNAPSHOT_KIND {
        return Err(ApplicationError::ValidationError(format!(
            "agent.frozen_run_input_snapshot_kind_invalid: kind must be {FROZEN_RUN_INPUT_SNAPSHOT_KIND}"
        )));
    }
    let generation_type = normalize_generation_type(
        object
            .get("generationType")
            .or_else(|| object.get("generation_type"))
            .and_then(Value::as_str)
            .unwrap_or("normal"),
    )?;
    let expected_generation_type = expected_generation_type.trim();
    if !expected_generation_type.is_empty() && generation_type != expected_generation_type {
        return Err(ApplicationError::ValidationError(
            "prompt_assembly.generation_type_mismatch: generationType must match FrozenRunInputSnapshot.generationType"
                .to_string(),
        ));
    }
    let prompt_inputs = object
        .get("promptInputs")
        .or_else(|| object.get("prompt_inputs"))
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "agent.frozen_run_input_prompt_inputs_required: promptInputs is required"
                    .to_string(),
            )
        })?;
    ensure_json_object(
        prompt_inputs,
        "agent.frozen_run_input_prompt_inputs_invalid: promptInputs must be an object",
    )?;

    let mut normalized = Map::new();
    normalized.insert(
        "schemaVersion".to_string(),
        json!(FROZEN_RUN_INPUT_SNAPSHOT_SCHEMA_VERSION),
    );
    normalized.insert(
        "kind".to_string(),
        Value::String(FROZEN_RUN_INPUT_SNAPSHOT_KIND.to_string()),
    );
    normalized.insert("generationType".to_string(), Value::String(generation_type));
    normalized.insert("promptInputs".to_string(), prompt_inputs.clone());
    let world_info_activation = object
        .get("worldInfoActivation")
        .or_else(|| object.get("world_info_activation"))
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "agent.frozen_run_input_world_info_activation_required: worldInfoActivation is required"
                    .to_string(),
            )
        })?;
    ensure_json_object(
        world_info_activation,
        "agent.frozen_run_input_world_info_activation_invalid: worldInfoActivation must be an object",
    )?;
    normalized.insert(
        "worldInfoActivation".to_string(),
        world_info_activation.clone(),
    );
    let macro_context = object
        .get("macroContext")
        .or_else(|| object.get("macro_context"))
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "agent.frozen_run_input_macro_context_required: macroContext is required"
                    .to_string(),
            )
        })?;
    ensure_json_object(
        macro_context,
        "agent.frozen_run_input_macro_context_invalid: macroContext must be an object",
    )?;
    normalized.insert("macroContext".to_string(), macro_context.clone());
    if let Some(current_model_connection) = object
        .get("currentModelConnection")
        .or_else(|| object.get("current_model_connection"))
    {
        normalized.insert(
            "currentModelConnection".to_string(),
            normalize_current_model_connection_snapshot(current_model_connection)?,
        );
    }
    Ok(Value::Object(normalized))
}

fn normalize_current_model_connection_snapshot(value: &Value) -> Result<Value, ApplicationError> {
    let object = value.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.current_model_connection_snapshot_required: currentModelConnection must be an object"
                .to_string(),
        )
    })?;
    let schema_version = object
        .get("schemaVersion")
        .or_else(|| object.get("schema_version"))
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "agent.current_model_connection_snapshot_schema_required: schemaVersion is required"
                    .to_string(),
            )
        })?;
    if schema_version != u64::from(CURRENT_MODEL_CONNECTION_SNAPSHOT_SCHEMA_VERSION) {
        return Err(ApplicationError::ValidationError(format!(
            "agent.current_model_connection_snapshot_schema_unsupported: schemaVersion {schema_version} is unsupported"
        )));
    }
    let kind = string_field(object, "kind")?;
    if kind != CURRENT_MODEL_CONNECTION_SNAPSHOT_KIND {
        return Err(ApplicationError::ValidationError(format!(
            "agent.current_model_connection_snapshot_kind_invalid: kind must be {CURRENT_MODEL_CONNECTION_SNAPSHOT_KIND}"
        )));
    }
    let settings = object.get("settings").ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.current_model_connection_settings_required: settings is required".to_string(),
        )
    })?;
    let settings_object = settings.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.current_model_connection_settings_invalid: settings must be an object"
                .to_string(),
        )
    })?;
    let source = settings_object
        .get("chat_completion_source")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "agent.current_model_connection_source_required: chat_completion_source cannot be empty"
                    .to_string(),
            )
        })?;
    let model = settings_object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "agent.current_model_connection_model_required: model cannot be empty".to_string(),
            )
        })?;

    let mut normalized_settings = Map::new();
    for key in connection_prompt_setting_keys() {
        if let Some(value) = settings_object.get(key) {
            normalized_settings.insert(key.to_string(), value.clone());
        }
    }
    normalized_settings.insert(
        "chat_completion_source".to_string(),
        Value::String(source.to_string()),
    );
    normalized_settings.insert("model".to_string(), Value::String(model.to_string()));
    if let Some(secret_id) = normalized_settings
        .get("secret_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    {
        normalized_settings.insert("secret_id".to_string(), Value::String(secret_id));
    } else {
        normalized_settings.remove("secret_id");
    }

    let mut normalized = Map::new();
    normalized.insert(
        "schemaVersion".to_string(),
        json!(CURRENT_MODEL_CONNECTION_SNAPSHOT_SCHEMA_VERSION),
    );
    normalized.insert(
        "kind".to_string(),
        Value::String(CURRENT_MODEL_CONNECTION_SNAPSHOT_KIND.to_string()),
    );
    normalized.insert("settings".to_string(), Value::Object(normalized_settings));
    Ok(Value::Object(normalized))
}

fn build_current_model_connection_snapshot(
    settings: &Value,
    model: &str,
    secret_id: Option<&str>,
) -> Result<Value, ApplicationError> {
    let settings_object = settings.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.current_model_connection_settings_invalid: settings must be an object"
                .to_string(),
        )
    })?;
    let source = settings_object
        .get("chat_completion_source")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "agent.current_model_connection_source_required: chat_completion_source cannot be empty"
                    .to_string(),
            )
        })?;
    let model = model.trim();
    if model.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.current_model_connection_model_required: model cannot be empty".to_string(),
        ));
    }
    let model_setting_key = prompt_model_setting_key(source)?;

    let mut snapshot_settings = Map::new();
    for key in connection_prompt_setting_keys() {
        if let Some(value) = settings_object.get(key) {
            snapshot_settings.insert(key.to_string(), value.clone());
        }
    }
    snapshot_settings.insert(
        "chat_completion_source".to_string(),
        Value::String(source.to_string()),
    );
    snapshot_settings.insert("model".to_string(), Value::String(model.to_string()));
    snapshot_settings.insert(
        model_setting_key.to_string(),
        Value::String(model.to_string()),
    );
    if let Some(secret_id) = secret_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    {
        snapshot_settings.insert("secret_id".to_string(), Value::String(secret_id));
    } else {
        snapshot_settings.remove("secret_id");
    }

    normalize_current_model_connection_snapshot(&json!({
        "schemaVersion": CURRENT_MODEL_CONNECTION_SNAPSHOT_SCHEMA_VERSION,
        "kind": CURRENT_MODEL_CONNECTION_SNAPSHOT_KIND,
        "settings": snapshot_settings,
    }))
}

fn normalize_generation_type(value: &str) -> Result<String, ApplicationError> {
    let generation_type = value.trim();
    if generation_type.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.invalid_generation_type: generationType cannot be empty".to_string(),
        ));
    }
    Ok(generation_type.to_string())
}

fn apply_model_binding_to_prompt_settings(
    settings: &mut Value,
    binding: &ResolvedLlmModelBinding,
) -> Result<(), ApplicationError> {
    let object = settings.as_object_mut().ok_or_else(|| {
        ApplicationError::ValidationError(
            "prompt_assembly.preset_data_invalid: preset data must be an object".to_string(),
        )
    })?;

    for key in connection_prompt_setting_keys() {
        object.remove(key);
    }

    object.insert(
        "chat_completion_source".to_string(),
        Value::String(binding.chat_completion_source.clone()),
    );
    object.insert(
        prompt_model_setting_key(&binding.chat_completion_source)?.to_string(),
        Value::String(binding.model_id.clone()),
    );

    if let Some(format) = binding
        .custom_api_format
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert(
            "custom_api_format".to_string(),
            Value::String(format.to_string()),
        );
    }

    Ok(())
}

fn apply_current_model_connection_to_prompt_settings(
    settings: &mut Value,
    frozen_run_input_snapshot: &Value,
) -> Result<(), ApplicationError> {
    let current_model_connection = frozen_run_input_snapshot
        .get("currentModelConnection")
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "prompt_assembly.current_model_connection_required: model.mode=currentPromptSnapshot with preset.ref requires FrozenRunInputSnapshot.currentModelConnection"
                    .to_string(),
            )
        })?;
    apply_current_model_connection_snapshot_to_prompt_settings(settings, current_model_connection)
}

fn apply_current_model_connection_snapshot_to_prompt_settings(
    settings: &mut Value,
    current_model_connection: &Value,
) -> Result<(), ApplicationError> {
    let object = settings.as_object_mut().ok_or_else(|| {
        ApplicationError::ValidationError(
            "prompt_assembly.preset_data_invalid: preset data must be an object".to_string(),
        )
    })?;
    let normalized_snapshot =
        normalize_current_model_connection_snapshot(current_model_connection)?;
    let connection_settings = normalized_snapshot
        .get("settings")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApplicationError::InternalError(
                "prompt_assembly.current_model_connection_normalized_invalid: normalized currentModelConnection settings missing"
                    .to_string(),
            )
        })?;

    for key in connection_prompt_setting_keys() {
        object.remove(key);
    }

    for key in connection_prompt_setting_keys() {
        if let Some(value) = connection_settings.get(key) {
            object.insert(key.to_string(), value.clone());
        }
    }

    Ok(())
}

fn connection_prompt_setting_keys() -> impl Iterator<Item = &'static str> {
    MODEL_PROMPT_SETTING_KEYS
        .iter()
        .copied()
        .chain(llm_connection_service::connection_payload_keys())
        .chain(llm_connection_service::source_specific_payload_keys())
        .chain(PROMPT_CONNECTION_EXTRA_SETTING_KEYS.iter().copied())
}

fn prompt_model_setting_key(source: &str) -> Result<&'static str, ApplicationError> {
    match source {
        "openai" => Ok("openai_model"),
        "openrouter" => Ok("openrouter_model"),
        "custom" => Ok("custom_model"),
        "claude" => Ok("claude_model"),
        "makersuite" => Ok("google_model"),
        "vertexai" => Ok("vertexai_model"),
        "ai21" => Ok("ai21_model"),
        "mistralai" => Ok("mistralai_model"),
        "deepseek" => Ok("deepseek_model"),
        "cohere" => Ok("cohere_model"),
        "perplexity" => Ok("perplexity_model"),
        "groq" => Ok("groq_model"),
        "moonshot" => Ok("moonshot_model"),
        "electronhub" => Ok("electronhub_model"),
        "nanogpt" => Ok("nanogpt_model"),
        "chutes" => Ok("chutes_model"),
        "siliconflow" => Ok("siliconflow_model"),
        "workers_ai" => Ok("workers_ai_model"),
        "zai" => Ok("zai_model"),
        "minimax" => Ok("minimax_model"),
        "aimlapi" => Ok("aimlapi_model"),
        "xai" => Ok("xai_model"),
        "pollinations" => Ok("pollinations_model"),
        "cometapi" => Ok("cometapi_model"),
        "fireworks" => Ok("fireworks_model"),
        "azure_openai" => Ok("azure_openai_model"),
        "aws_bedrock" => Ok("aws_bedrock_model"),
        other => Err(ApplicationError::InternalError(format!(
            "prompt_assembly.model_source_unmapped: no prompt settings model key for source `{other}`"
        ))),
    }
}

fn string_field<'a>(
    object: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, ApplicationError> {
    object.get(field).and_then(Value::as_str).ok_or_else(|| {
        ApplicationError::ValidationError(format!(
            "agent.frozen_run_input_snapshot_invalid: {field} must be a string"
        ))
    })
}

fn ensure_json_object(value: &Value, message: &str) -> Result<(), ApplicationError> {
    if value.as_object().is_none() {
        return Err(ApplicationError::ValidationError(message.to_string()));
    }
    Ok(())
}

fn sha256_value(value: &Value) -> Result<String, ApplicationError> {
    let bytes = serde_json::to_vec(value).map_err(|error| {
        ApplicationError::InternalError(format!("prompt_assembly.fingerprint_failed: {error}"))
    })?;
    Ok(format!("sha256:{}", sha256_bytes(&bytes)))
}

fn sha256_string(value: &str) -> String {
    format!("sha256:{}", sha256_bytes(value.as_bytes()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex_lower(&digest)
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::application::services::llm_connection_service::ResolvedLlmSecretRef;

    fn model_binding(
        source: &str,
        model_id: &str,
        custom_api_format: Option<&str>,
    ) -> ResolvedLlmModelBinding {
        ResolvedLlmModelBinding {
            mode: "connectionRef".to_string(),
            connection_ref: "test-connection".to_string(),
            connection_display_name: "Test Connection".to_string(),
            chat_completion_source: source.to_string(),
            custom_api_format: custom_api_format.map(str::to_string),
            model_id: model_id.to_string(),
            secret_ref: ResolvedLlmSecretRef {
                key: "api_key_deepseek".to_string(),
                id: "secret-1".to_string(),
                label_snapshot: None,
            },
        }
    }

    #[test]
    fn normalizes_frozen_run_input_snapshot() {
        let snapshot = normalize_frozen_run_input_snapshot(
            &json!({
                "schemaVersion": 1,
                "kind": FROZEN_RUN_INPUT_SNAPSHOT_KIND,
                "generationType": "swipe",
                "promptInputs": { "type": "swipe", "messages": [] },
                "worldInfoActivation": { "entries": [] },
                "macroContext": { "names": { "user": "User", "char": "Char" } },
                "currentModelConnection": {
                    "schemaVersion": 1,
                    "kind": CURRENT_MODEL_CONNECTION_SNAPSHOT_KIND,
                    "settings": {
                        "chat_completion_source": "custom",
                        "model": "opencode-model",
                        "custom_model": "opencode-model",
                        "custom_url": "https://opencode.example.test/v1",
                        "custom_api_format": "openai_compat",
                        "secret_id": "opencode-secret"
                    }
                },
            }),
            "swipe",
        )
        .unwrap();

        assert_eq!(snapshot["generationType"], "swipe");
        assert_eq!(snapshot["worldInfoActivation"]["entries"], json!([]));
        assert_eq!(snapshot["macroContext"]["names"]["char"], "Char");
        assert_eq!(
            snapshot["currentModelConnection"]["settings"]["custom_url"],
            "https://opencode.example.test/v1"
        );
        assert_eq!(
            snapshot["currentModelConnection"]["settings"]["secret_id"],
            "opencode-secret"
        );
    }

    #[test]
    fn builds_current_model_connection_snapshot_with_backend_owned_fields() {
        let snapshot = build_current_model_connection_snapshot(
            &json!({
                "chat_completion_source": "aws_bedrock",
                "aws_bedrock_model": "amazon.titan-text-premier-v1:0",
                "aws_bedrock_region": "eu-central-1",
                "aws_bedrock_use_custom_template": true,
                "aws_bedrock_custom_template": "{\"inputText\":{{messages}}}",
                "aws_bedrock_custom_response_path": "results.0.outputText",
                "aws_bedrock_custom_stream_path": "delta.text",
                "additional_parameters_by_source": {
                    "aws_bedrock": {
                        "include_body": "",
                        "exclude_body": "",
                        "include_headers": "X-Trace: frozen"
                    }
                },
                "custom_claude_prompt_caching": true,
                "custom_models_by_source": { "aws_bedrock": ["catalog-only"] },
                "openrouter_group_models": true,
                "openrouter_sort_models": "context",
                "show_external_models": true,
                "additional_parameters_migration_version": 1,
                "bypass_status_check": true
            }),
            "amazon.titan-text-premier-v1:0",
            Some("bedrock-secret"),
        )
        .unwrap();
        let settings = snapshot["settings"].as_object().unwrap();

        assert_eq!(settings["chat_completion_source"], "aws_bedrock");
        assert_eq!(settings["model"], "amazon.titan-text-premier-v1:0");
        assert_eq!(
            settings["aws_bedrock_model"],
            "amazon.titan-text-premier-v1:0"
        );
        assert_eq!(settings["aws_bedrock_region"], "eu-central-1");
        assert_eq!(settings["aws_bedrock_use_custom_template"], true);
        assert_eq!(
            settings["aws_bedrock_custom_response_path"],
            "results.0.outputText"
        );
        assert_eq!(
            settings["additional_parameters_by_source"]["aws_bedrock"]["include_headers"],
            "X-Trace: frozen"
        );
        assert_eq!(settings["custom_claude_prompt_caching"], true);
        assert_eq!(settings["secret_id"], "bedrock-secret");
        assert!(settings.get("custom_models_by_source").is_none());
        assert!(settings.get("openrouter_group_models").is_none());
        assert!(settings.get("openrouter_sort_models").is_none());
        assert!(settings.get("show_external_models").is_none());
        assert!(settings.get("bypass_status_check").is_none());
        assert!(
            settings
                .get("additional_parameters_migration_version")
                .is_none()
        );
    }

    #[test]
    fn builds_current_model_connection_snapshot_with_openrouter_routing_fields() {
        let snapshot = build_current_model_connection_snapshot(
            &json!({
                "chat_completion_source": "openrouter",
                "openrouter_model": "anthropic/claude-sonnet-4",
                "openrouter_use_fallback": true,
                "openrouter_providers": ["anthropic", "openai"],
                "openrouter_quantizations": ["bf16"],
                "openrouter_allow_fallbacks": false,
                "openrouter_middleout": "off",
                "openrouter_group_models": true,
                "openrouter_sort_models": "context",
                "custom_models_by_source": { "openrouter": ["catalog-only"] }
            }),
            "anthropic/claude-sonnet-4",
            Some("openrouter-secret"),
        )
        .unwrap();
        let settings = snapshot["settings"].as_object().unwrap();

        assert_eq!(settings["chat_completion_source"], "openrouter");
        assert_eq!(settings["model"], "anthropic/claude-sonnet-4");
        assert_eq!(settings["openrouter_model"], "anthropic/claude-sonnet-4");
        assert_eq!(settings["openrouter_use_fallback"], true);
        assert_eq!(
            settings["openrouter_providers"],
            json!(["anthropic", "openai"])
        );
        assert_eq!(settings["openrouter_quantizations"], json!(["bf16"]));
        assert_eq!(settings["openrouter_allow_fallbacks"], false);
        assert_eq!(settings["openrouter_middleout"], "off");
        assert_eq!(settings["secret_id"], "openrouter-secret");
        assert!(settings.get("openrouter_group_models").is_none());
        assert!(settings.get("openrouter_sort_models").is_none());
        assert!(settings.get("custom_models_by_source").is_none());
    }

    #[test]
    fn current_model_connection_snapshot_rejects_unmapped_source() {
        let error = build_current_model_connection_snapshot(
            &json!({
                "chat_completion_source": "unsupported",
                "custom_url": "https://example.test/v1"
            }),
            "local-model",
            None,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("prompt_assembly.model_source_unmapped")
        );
    }

    #[test]
    fn rejects_frozen_snapshot_generation_type_mismatch() {
        let error = normalize_frozen_run_input_snapshot(
            &json!({
                "schemaVersion": 1,
                "kind": FROZEN_RUN_INPUT_SNAPSHOT_KIND,
                "generationType": "normal",
                "promptInputs": {},
                "worldInfoActivation": { "entries": [] },
                "macroContext": {},
            }),
            "regenerate",
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("prompt_assembly.generation_type_mismatch")
        );
    }

    #[test]
    fn overlays_connection_ref_model_without_preset_source() {
        let mut settings = json!({
            "name": "Prompt Only",
            "temp_openai": 0.7,
            "custom_url": "https://stale.example.test",
            "openrouter_model": "anthropic/claude"
        });
        let binding = model_binding("deepseek", "deepseek-v4-flash", None);

        apply_model_binding_to_prompt_settings(&mut settings, &binding).unwrap();

        assert_eq!(settings["chat_completion_source"], "deepseek");
        assert_eq!(settings["deepseek_model"], "deepseek-v4-flash");
        assert_eq!(settings["temp_openai"], 0.7);
        assert!(settings.get("custom_url").is_none());
        assert!(settings.get("openrouter_model").is_none());
    }

    #[test]
    fn connection_ref_model_overrides_conflicting_preset_source() {
        let mut settings = json!({
            "chat_completion_source": "openrouter",
            "openrouter_model": "anthropic/claude",
            "deepseek_model": "deepseek-chat"
        });
        let binding = model_binding("deepseek", "deepseek-v4-flash", None);

        apply_model_binding_to_prompt_settings(&mut settings, &binding).unwrap();

        assert_eq!(settings["chat_completion_source"], "deepseek");
        assert_eq!(settings["deepseek_model"], "deepseek-v4-flash");
        assert!(settings.get("openrouter_model").is_none());
    }

    #[test]
    fn custom_connection_ref_sets_custom_format_and_model() {
        let mut settings = json!({
            "chat_completion_source": "deepseek",
            "deepseek_model": "deepseek-v4-flash"
        });
        let binding = model_binding("custom", "local-model", Some("gemini_interactions"));

        apply_model_binding_to_prompt_settings(&mut settings, &binding).unwrap();

        assert_eq!(settings["chat_completion_source"], "custom");
        assert_eq!(settings["custom_model"], "local-model");
        assert_eq!(settings["custom_api_format"], "gemini_interactions");
        assert!(settings.get("deepseek_model").is_none());
    }

    #[test]
    fn current_prompt_snapshot_overlays_connection_settings_from_frozen_snapshot() {
        let mut settings = json!({
            "name": "Prompt Only",
            "temp_openai": 0.7,
            "chat_completion_source": "custom",
            "custom_model": "old-opencode-model",
            "custom_url": "https://opencode.example.test/v1",
            "secret_id": "old-secret",
            "openrouter_providers": ["stale-provider"],
            "openrouter_quantizations": ["stale-quantization"],
            "openrouter_allow_fallbacks": false,
            "openrouter_middleout": "off",
            "additional_parameters_by_source": {
                "custom": {
                    "include_body": "",
                    "exclude_body": "",
                    "include_headers": "X-Preset: stale"
                }
            },
            "custom_claude_prompt_caching": false
        });
        let frozen_run_input_snapshot = normalize_frozen_run_input_snapshot(
            &json!({
                "schemaVersion": 1,
                "kind": FROZEN_RUN_INPUT_SNAPSHOT_KIND,
                "generationType": "normal",
                "promptInputs": {},
                "worldInfoActivation": {},
                "macroContext": {},
                "currentModelConnection": {
                    "schemaVersion": 1,
                    "kind": CURRENT_MODEL_CONNECTION_SNAPSHOT_KIND,
                    "settings": {
                        "chat_completion_source": "custom",
                        "model": "deepseek-chat-through-custom",
                        "custom_model": "deepseek-chat-through-custom",
                        "custom_url": "https://api.deepseek.example/v1",
                        "custom_api_format": "openai_compat",
                        "secret_id": "deepseek-secret",
                        "additional_parameters_by_source": {
                            "custom": {
                                "include_body": "",
                                "exclude_body": "",
                                "include_headers": "X-Run: current"
                            }
                        },
                        "custom_claude_prompt_caching": true
                    }
                }
            }),
            "normal",
        )
        .unwrap();

        apply_current_model_connection_to_prompt_settings(
            &mut settings,
            &frozen_run_input_snapshot,
        )
        .unwrap();

        assert_eq!(settings["chat_completion_source"], "custom");
        assert_eq!(settings["custom_model"], "deepseek-chat-through-custom");
        assert_eq!(settings["custom_url"], "https://api.deepseek.example/v1");
        assert_eq!(settings["custom_api_format"], "openai_compat");
        assert_eq!(settings["secret_id"], "deepseek-secret");
        assert_eq!(
            settings["additional_parameters_by_source"]["custom"]["include_headers"],
            "X-Run: current"
        );
        assert_eq!(settings["custom_claude_prompt_caching"], true);
        assert!(settings.get("openrouter_providers").is_none());
        assert!(settings.get("openrouter_quantizations").is_none());
        assert!(settings.get("openrouter_allow_fallbacks").is_none());
        assert!(settings.get("openrouter_middleout").is_none());
        assert_eq!(settings["temp_openai"], 0.7);
    }

    #[test]
    fn current_prompt_snapshot_removes_stale_secret_when_current_connection_is_keyless() {
        let mut settings = json!({
            "chat_completion_source": "custom",
            "custom_model": "old-model",
            "custom_url": "https://old.example.test/v1",
            "secret_id": "old-secret"
        });
        let frozen_run_input_snapshot = normalize_frozen_run_input_snapshot(
            &json!({
                "schemaVersion": 1,
                "kind": FROZEN_RUN_INPUT_SNAPSHOT_KIND,
                "generationType": "normal",
                "promptInputs": {},
                "worldInfoActivation": {},
                "macroContext": {},
                "currentModelConnection": {
                    "schemaVersion": 1,
                    "kind": CURRENT_MODEL_CONNECTION_SNAPSHOT_KIND,
                    "settings": {
                        "chat_completion_source": "custom",
                        "model": "local-model",
                        "custom_model": "local-model",
                        "custom_url": "http://127.0.0.1:8000/v1",
                        "custom_api_format": "openai_compat"
                    }
                }
            }),
            "normal",
        )
        .unwrap();

        apply_current_model_connection_to_prompt_settings(
            &mut settings,
            &frozen_run_input_snapshot,
        )
        .unwrap();

        assert_eq!(settings["custom_model"], "local-model");
        assert_eq!(settings["custom_url"], "http://127.0.0.1:8000/v1");
        assert!(settings.get("secret_id").is_none());
    }

    #[test]
    fn current_prompt_snapshot_requires_frozen_current_model_connection() {
        let mut settings = json!({
            "chat_completion_source": "custom",
            "custom_model": "old-model"
        });
        let frozen_run_input_snapshot = normalize_frozen_run_input_snapshot(
            &json!({
                "schemaVersion": 1,
                "kind": FROZEN_RUN_INPUT_SNAPSHOT_KIND,
                "generationType": "normal",
                "promptInputs": {},
                "worldInfoActivation": {},
                "macroContext": {}
            }),
            "normal",
        )
        .unwrap();

        let error = apply_current_model_connection_to_prompt_settings(
            &mut settings,
            &frozen_run_input_snapshot,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("prompt_assembly.current_model_connection_required")
        );
    }
}
