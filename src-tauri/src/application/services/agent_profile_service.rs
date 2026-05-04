use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::Value;

use crate::application::errors::ApplicationError;
use crate::domain::models::agent::AgentToolSpec;
use crate::domain::models::agent::plan::{AgentPlanMode, AgentPlanPolicy, DEFAULT_AGENT_PLAN_BETA};
use crate::domain::models::agent::profile::{
    AGENT_PROFILE_KIND, AGENT_PROFILE_SCHEMA_VERSION, AgentModelBinding, AgentModelBindingMode,
    AgentOutputArtifactTarget, AgentOutputPolicy, AgentPresetBinding, AgentPresetBindingMode,
    AgentProfileDefinition, AgentProfileId, AgentProfileInstructions, AgentProfileSourceTrace,
    AgentProfileSummary, AgentRunPolicy, AgentSkillPolicy, AgentToolDescriptionOverride,
    AgentToolPolicy, AgentWorkspacePolicy, DEFAULT_AGENT_PROFILE_ID,
    DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_CALL, DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_RUN,
    DEFAULT_AGENT_TOOL_MAX_CALLS_PER_RUN, DEFAULT_AGENT_TOOL_MAX_ROUNDS, ResolvedAgentOutputPolicy,
    ResolvedAgentProfile,
};
use crate::domain::models::agent::{
    AgentRunPresentation, ArtifactSpec, ArtifactTarget, CommitPolicy, WorkspacePath,
    WorkspaceRootCommit, WorkspaceRootLifecycle, WorkspaceRootMount, WorkspaceRootScope,
    WorkspaceRootSpec,
};
use crate::domain::models::preset::PresetType;
use crate::domain::repositories::agent_profile_repository::AgentProfileRepository;
use crate::domain::repositories::preset_repository::PresetRepository;

const WORKSPACE_ROOT_UNIVERSE: [&str; 5] = ["output", "scratch", "plan", "summaries", "persist"];
const MESSAGE_BODY_ARTIFACT_TARGET: ArtifactTarget = ArtifactTarget::MessageBody;

pub struct AgentProfileService {
    profile_repository: Arc<dyn AgentProfileRepository>,
    preset_repository: Arc<dyn PresetRepository>,
}

pub struct AgentProfileResolveInput<'a> {
    pub profile_id: Option<&'a str>,
    pub known_tools: &'a [AgentToolSpec],
}

impl AgentProfileService {
    pub fn new(
        profile_repository: Arc<dyn AgentProfileRepository>,
        preset_repository: Arc<dyn PresetRepository>,
    ) -> Self {
        Self {
            profile_repository,
            preset_repository,
        }
    }

    pub async fn resolve_profile(
        &self,
        input: AgentProfileResolveInput<'_>,
    ) -> Result<ResolvedAgentProfile, ApplicationError> {
        let requested = input
            .profile_id
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let (definition, source) = match requested {
            Some(raw_id) => {
                let id =
                    AgentProfileId::parse(raw_id).map_err(ApplicationError::ValidationError)?;
                match self.profile_repository.load_profile(&id).await? {
                    Some(profile) => (profile, format!("file:{}", id.as_str())),
                    None if id.as_str() == DEFAULT_AGENT_PROFILE_ID => {
                        (default_writer_profile()?, "built_in".to_string())
                    }
                    None => {
                        return Err(ApplicationError::NotFound(format!(
                            "agent.profile_not_found: Agent profile `{}` does not exist",
                            id.as_str()
                        )));
                    }
                }
            }
            None => (default_writer_profile()?, "built_in".to_string()),
        };

        self.resolve_definition(definition, source, input.known_tools)
            .await
    }

    pub async fn list_profiles(&self) -> Result<Vec<AgentProfileSummary>, ApplicationError> {
        let mut profiles = self
            .profile_repository
            .list_profiles()
            .await
            .map_err(ApplicationError::from)?;
        if profiles
            .iter()
            .all(|profile| profile.id.as_str() != DEFAULT_AGENT_PROFILE_ID)
        {
            profiles.insert(0, default_writer_profile()?.summary());
        }
        Ok(profiles)
    }

    pub async fn load_profile(
        &self,
        profile_id: &str,
    ) -> Result<Option<AgentProfileDefinition>, ApplicationError> {
        let id = AgentProfileId::parse(profile_id).map_err(ApplicationError::ValidationError)?;
        let profile = self
            .profile_repository
            .load_profile(&id)
            .await
            .map_err(ApplicationError::from)?;
        if profile.is_none() && id.as_str() == DEFAULT_AGENT_PROFILE_ID {
            return Ok(Some(default_writer_profile()?));
        }
        Ok(profile)
    }

    pub async fn save_profile(
        &self,
        profile: AgentProfileDefinition,
        known_tools: &[AgentToolSpec],
    ) -> Result<(), ApplicationError> {
        self.resolve_definition(
            profile.clone(),
            format!("file:{}", profile.id.as_str()),
            known_tools,
        )
        .await?;
        self.profile_repository
            .save_profile(&profile)
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn delete_profile(&self, profile_id: &str) -> Result<(), ApplicationError> {
        let id = AgentProfileId::parse(profile_id).map_err(ApplicationError::ValidationError)?;
        self.profile_repository
            .delete_profile(&id)
            .await
            .map_err(ApplicationError::from)
    }

    async fn resolve_definition(
        &self,
        definition: AgentProfileDefinition,
        source: String,
        known_tools: &[AgentToolSpec],
    ) -> Result<ResolvedAgentProfile, ApplicationError> {
        validate_profile_header(&definition)?;
        validate_preset_binding(&definition.preset, self.preset_repository.as_ref()).await?;
        validate_model_binding(&definition.model)?;
        validate_instructions(&definition.instructions)?;
        validate_plan_policy(&definition.plan)?;
        validate_tool_policy(&definition.tools, known_tools)?;
        validate_run_policy(&definition.run, &definition.tools)?;
        validate_skill_policy(&definition.skills)?;
        validate_workspace_policy(&definition.workspace)?;
        let output = resolve_output_policy(&definition.output, &definition.workspace)?;

        Ok(ResolvedAgentProfile {
            schema_version: definition.schema_version,
            kind: definition.kind,
            id: definition.id,
            display_name: definition.display_name,
            description: definition.description,
            preset: definition.preset,
            model: definition.model,
            run: definition.run,
            instructions: definition.instructions,
            tools: definition.tools,
            skills: definition.skills,
            workspace: definition.workspace,
            plan: definition.plan,
            output,
            source_trace: AgentProfileSourceTrace {
                profile_source: source,
            },
        })
    }
}

fn default_writer_profile() -> Result<AgentProfileDefinition, ApplicationError> {
    Ok(AgentProfileDefinition {
        schema_version: AGENT_PROFILE_SCHEMA_VERSION,
        kind: AGENT_PROFILE_KIND.to_string(),
        id: AgentProfileId::parse(DEFAULT_AGENT_PROFILE_ID)
            .map_err(ApplicationError::ValidationError)?,
        display_name: "Default Writer".to_string(),
        description: Some("General creative writing Agent profile.".to_string()),
        preset: AgentPresetBinding {
            mode: AgentPresetBindingMode::CurrentPromptSnapshot,
            ref_: None,
            required: false,
        },
        model: AgentModelBinding {
            mode: AgentModelBindingMode::CurrentPromptSnapshot,
        },
        run: AgentRunPolicy {
            presentation: AgentRunPresentation::Foreground,
            model_retry: Default::default(),
        },
        instructions: AgentProfileInstructions {
            agent_system_prompt: None,
        },
        tools: AgentToolPolicy {
            allow: vec![
                "chat.search".to_string(),
                "chat.read_messages".to_string(),
                "worldinfo.read_activated".to_string(),
                "skill.list".to_string(),
                "skill.search".to_string(),
                "skill.read".to_string(),
                "workspace.list_files".to_string(),
                "workspace.search_files".to_string(),
                "workspace.read_file".to_string(),
                "workspace.write_file".to_string(),
                "workspace.apply_patch".to_string(),
                "workspace.commit".to_string(),
                "workspace.finish".to_string(),
            ],
            deny: Vec::new(),
            tool_descriptions: BTreeMap::new(),
            max_rounds: DEFAULT_AGENT_TOOL_MAX_ROUNDS,
            max_calls_per_run: DEFAULT_AGENT_TOOL_MAX_CALLS_PER_RUN,
            max_calls_per_tool: BTreeMap::new(),
        },
        skills: AgentSkillPolicy {
            visible: vec!["*".to_string()],
            deny: Vec::new(),
            max_read_chars_per_call: DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_CALL,
            max_read_chars_per_run: DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_RUN,
        },
        workspace: AgentWorkspacePolicy {
            visible_roots: WORKSPACE_ROOT_UNIVERSE
                .iter()
                .map(|root| root.to_string())
                .collect(),
            writable_roots: WORKSPACE_ROOT_UNIVERSE
                .iter()
                .map(|root| root.to_string())
                .collect(),
        },
        plan: AgentPlanPolicy {
            mode: AgentPlanMode::None,
            beta: DEFAULT_AGENT_PLAN_BETA,
            nodes: Vec::new(),
        },
        output: AgentOutputPolicy {
            artifacts: vec![crate::domain::models::agent::profile::AgentOutputArtifact {
                id: "main".to_string(),
                path: "output/main.md".to_string(),
                kind: "markdown".to_string(),
                target: AgentOutputArtifactTarget::MessageBody,
                required: true,
                assembly_order: 0,
            }],
        },
    })
}

fn validate_profile_header(profile: &AgentProfileDefinition) -> Result<(), ApplicationError> {
    if profile.schema_version != AGENT_PROFILE_SCHEMA_VERSION {
        return Err(ApplicationError::ValidationError(format!(
            "agent.profile_schema_unsupported: schemaVersion {} is unsupported",
            profile.schema_version
        )));
    }
    if profile.kind != AGENT_PROFILE_KIND {
        return Err(ApplicationError::ValidationError(format!(
            "agent.profile_kind_invalid: kind must be {AGENT_PROFILE_KIND}"
        )));
    }
    if profile.display_name.trim().is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.profile_display_name_required: displayName cannot be empty".to_string(),
        ));
    }
    Ok(())
}

async fn validate_preset_binding(
    binding: &AgentPresetBinding,
    preset_repository: &dyn PresetRepository,
) -> Result<(), ApplicationError> {
    match binding.mode {
        AgentPresetBindingMode::CurrentPromptSnapshot | AgentPresetBindingMode::None => Ok(()),
        AgentPresetBindingMode::Ref => {
            let Some(ref_) = binding.ref_.as_ref() else {
                return Err(ApplicationError::ValidationError(
                    "agent.profile_preset_ref_required: preset.ref is required when preset.mode is ref"
                        .to_string(),
                ));
            };
            let preset_type = PresetType::from_api_id(ref_.api_id.as_str()).ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "agent.profile_preset_api_invalid: unsupported preset apiId `{}`",
                    ref_.api_id
                ))
            })?;
            if ref_.name.trim().is_empty() {
                return Err(ApplicationError::ValidationError(
                    "agent.profile_preset_name_required: preset.ref.name cannot be empty"
                        .to_string(),
                ));
            }
            if !binding.required {
                return Ok(());
            }
            let exists = preset_repository
                .preset_exists(ref_.name.as_str(), &preset_type)
                .await?
                || preset_repository
                    .get_default_preset(ref_.name.as_str(), &preset_type)
                    .await?
                    .is_some();
            if !exists {
                return Err(ApplicationError::ValidationError(format!(
                    "agent.profile_preset_missing: required preset `{}` for apiId `{}` does not exist",
                    ref_.name, ref_.api_id
                )));
            }
            Ok(())
        }
    }
}

fn validate_model_binding(binding: &AgentModelBinding) -> Result<(), ApplicationError> {
    match binding.mode {
        AgentModelBindingMode::CurrentPromptSnapshot => Ok(()),
    }
}

fn validate_instructions(instructions: &AgentProfileInstructions) -> Result<(), ApplicationError> {
    if instructions
        .agent_system_prompt
        .as_ref()
        .is_some_and(|prompt| prompt.trim().is_empty())
    {
        return Err(ApplicationError::ValidationError(
            "agent.profile_system_prompt_empty: instructions.agentSystemPrompt cannot be empty"
                .to_string(),
        ));
    }
    Ok(())
}

fn validate_plan_policy(plan: &AgentPlanPolicy) -> Result<(), ApplicationError> {
    if plan.mode != AgentPlanMode::None {
        return Err(ApplicationError::ValidationError(
            "agent.plan_mode_unsupported: Phase 3 foundation only supports plan.mode = none"
                .to_string(),
        ));
    }
    if !plan.nodes.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.plan_nodes_unsupported: plan.nodes must be empty when plan.mode = none"
                .to_string(),
        ));
    }
    Ok(())
}

fn validate_tool_policy(
    policy: &AgentToolPolicy,
    known_tools: &[AgentToolSpec],
) -> Result<(), ApplicationError> {
    if policy.max_rounds == 0 {
        return Err(ApplicationError::ValidationError(
            "agent.profile_max_rounds_invalid: tools.maxRounds must be > 0".to_string(),
        ));
    }
    if policy.max_calls_per_run == 0 {
        return Err(ApplicationError::ValidationError(
            "agent.profile_max_calls_invalid: tools.maxCallsPerRun must be > 0".to_string(),
        ));
    }

    let known = known_tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect::<BTreeSet<_>>();
    let allow = policy
        .allow
        .iter()
        .map(|name| name.as_str())
        .collect::<BTreeSet<_>>();
    let deny = policy
        .deny
        .iter()
        .map(|name| name.as_str())
        .collect::<BTreeSet<_>>();

    if allow.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.profile_tools_empty: tools.allow cannot be empty".to_string(),
        ));
    }
    for name in allow.iter().chain(deny.iter()) {
        if !known.contains(name) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_unknown_tool: unknown tool `{name}`"
            )));
        }
    }
    if !allow.contains("workspace.finish") || deny.contains("workspace.finish") {
        return Err(ApplicationError::ValidationError(
            "agent.profile_finish_required: workspace.finish must be visible".to_string(),
        ));
    }

    let visible = allow.difference(&deny).copied().collect::<BTreeSet<_>>();
    if !visible.contains("workspace.write_file") {
        return Err(ApplicationError::ValidationError(
            "agent.profile_output_writer_required: workspace.write_file must be visible so the Agent can create the required message body artifact"
                .to_string(),
        ));
    }
    for (name, override_) in &policy.tool_descriptions {
        if !visible.contains(name.as_str()) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_tool_description_invisible: `{name}` is not visible"
            )));
        }
        let spec = known_tools
            .iter()
            .find(|tool| tool.name == *name)
            .expect("known tool already checked");
        validate_tool_description_override(spec, override_)?;
    }

    for (name, max) in &policy.max_calls_per_tool {
        if !visible.contains(name.as_str()) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_tool_budget_invisible: `{name}` is not visible"
            )));
        }
        if *max == 0 {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_tool_budget_invalid: maxCallsPerTool.{name} must be > 0"
            )));
        }
    }

    Ok(())
}

fn validate_run_policy(
    run: &AgentRunPolicy,
    tools: &AgentToolPolicy,
) -> Result<(), ApplicationError> {
    if run.model_retry.max_retries > 0 && run.model_retry.interval_ms == 0 {
        return Err(ApplicationError::ValidationError(
            "agent.profile_model_retry_invalid: run.modelRetry.intervalMs must be > 0 when retries are enabled"
                .to_string(),
        ));
    }
    if run.presentation == AgentRunPresentation::Foreground
        && (!tools.allow.iter().any(|name| name == "workspace.commit")
            || tools.deny.iter().any(|name| name == "workspace.commit"))
    {
        return Err(ApplicationError::ValidationError(
            "agent.profile_commit_required: foreground profiles must expose workspace.commit"
                .to_string(),
        ));
    }

    Ok(())
}

fn validate_tool_description_override(
    spec: &AgentToolSpec,
    override_: &AgentToolDescriptionOverride,
) -> Result<(), ApplicationError> {
    if let Some(description) = override_.description.as_ref() {
        if description.trim().is_empty() {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_tool_description_empty: description for `{}` cannot be empty",
                spec.name
            )));
        }
    }
    if override_.properties.is_empty() {
        return Ok(());
    }
    let properties = spec
        .input_schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "agent.profile_tool_properties_invalid: `{}` has no object properties",
                spec.name
            ))
        })?;
    for (property, description) in &override_.properties {
        if !properties.contains_key(property) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_unknown_tool_property: `{}` has no property `{property}`",
                spec.name
            )));
        }
        if description.trim().is_empty() {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_tool_property_description_empty: `{}` property `{property}` cannot be empty",
                spec.name
            )));
        }
    }
    Ok(())
}

fn validate_skill_policy(policy: &AgentSkillPolicy) -> Result<(), ApplicationError> {
    if policy.visible.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.profile_skill_visible_empty: skills.visible cannot be empty".to_string(),
        ));
    }
    if policy.max_read_chars_per_call == 0 || policy.max_read_chars_per_run == 0 {
        return Err(ApplicationError::ValidationError(
            "agent.profile_skill_budget_invalid: skill read budgets must be > 0".to_string(),
        ));
    }
    if policy.max_read_chars_per_call > policy.max_read_chars_per_run {
        return Err(ApplicationError::ValidationError(
            "agent.profile_skill_budget_invalid: maxReadCharsPerCall cannot exceed maxReadCharsPerRun"
                .to_string(),
        ));
    }
    for name in &policy.visible {
        if name == "*" {
            continue;
        }
        validate_skill_name(name)?;
    }
    for name in &policy.deny {
        if name == "*" {
            continue;
        }
        validate_skill_name(name)?;
    }
    Ok(())
}

fn validate_workspace_policy(policy: &AgentWorkspacePolicy) -> Result<(), ApplicationError> {
    let universe = WORKSPACE_ROOT_UNIVERSE.into_iter().collect::<BTreeSet<_>>();
    let visible = policy
        .visible_roots
        .iter()
        .map(|root| root.as_str())
        .collect::<BTreeSet<_>>();
    let writable = policy
        .writable_roots
        .iter()
        .map(|root| root.as_str())
        .collect::<BTreeSet<_>>();

    if visible.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.profile_workspace_visible_empty: workspace.visibleRoots cannot be empty"
                .to_string(),
        ));
    }
    for root in visible.iter().chain(writable.iter()) {
        if !universe.contains(root) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_workspace_root_invalid: `{root}` is not an Agent workspace root"
            )));
        }
    }
    for root in &writable {
        if !visible.contains(root) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_workspace_root_invalid: writable root `{root}` is not visible"
            )));
        }
    }
    Ok(())
}

fn resolve_output_policy(
    policy: &AgentOutputPolicy,
    workspace: &AgentWorkspacePolicy,
) -> Result<ResolvedAgentOutputPolicy, ApplicationError> {
    if policy.artifacts.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.profile_output_empty: output.artifacts cannot be empty".to_string(),
        ));
    }

    let visible = workspace
        .visible_roots
        .iter()
        .map(|root| root.as_str())
        .collect::<BTreeSet<_>>();
    let writable = workspace
        .writable_roots
        .iter()
        .map(|root| root.as_str())
        .collect::<BTreeSet<_>>();

    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut message_body_artifact = None;
    let mut artifacts = Vec::with_capacity(policy.artifacts.len());
    for artifact in &policy.artifacts {
        validate_artifact_id(&artifact.id)?;
        if !ids.insert(artifact.id.as_str()) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_output_duplicate_id: duplicate artifact id `{}`",
                artifact.id
            )));
        }
        if artifact.kind.trim().is_empty() {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_output_kind_required: artifact `{}` kind cannot be empty",
                artifact.id
            )));
        }
        let path = WorkspacePath::parse(&artifact.path)?;
        if !paths.insert(path.as_str().to_string()) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_output_duplicate_path: duplicate artifact path `{}`",
                path.as_str()
            )));
        }
        let root = path.as_str().split('/').next().unwrap_or_default();
        if !visible.contains(root) || !writable.contains(root) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_output_path_denied: artifact `{}` path `{}` must be visible and writable",
                artifact.id,
                path.as_str()
            )));
        }

        let target = match artifact.target {
            AgentOutputArtifactTarget::MessageBody => {
                if message_body_artifact.is_some() {
                    return Err(ApplicationError::ValidationError(
                        "agent.profile_output_duplicate_message_body: only one messageBody artifact is supported"
                            .to_string(),
                    ));
                }
                message_body_artifact = Some((artifact.id.clone(), path.as_str().to_string()));
                MESSAGE_BODY_ARTIFACT_TARGET
            }
        };

        artifacts.push(ArtifactSpec {
            id: artifact.id.clone(),
            path: path.as_str().to_string(),
            kind: artifact.kind.trim().to_string(),
            target,
            required: artifact.required,
            assembly_order: artifact.assembly_order,
        });
    }

    let Some((message_body_artifact_id, message_body_path)) = message_body_artifact else {
        return Err(ApplicationError::ValidationError(
            "agent.profile_output_message_body_missing: output.artifacts must include one messageBody artifact"
                .to_string(),
        ));
    };

    Ok(ResolvedAgentOutputPolicy {
        artifacts,
        message_body_artifact_id,
        message_body_path,
    })
}

pub fn workspace_roots_from_profile(profile: &ResolvedAgentProfile) -> Vec<WorkspaceRootSpec> {
    let visible = profile
        .workspace
        .visible_roots
        .iter()
        .map(|root| root.as_str())
        .collect::<BTreeSet<_>>();
    let writable = profile
        .workspace
        .writable_roots
        .iter()
        .map(|root| root.as_str())
        .collect::<BTreeSet<_>>();

    WORKSPACE_ROOT_UNIVERSE
        .iter()
        .map(|root| {
            if *root == "persist" {
                WorkspaceRootSpec {
                    path: root.to_string(),
                    lifecycle: WorkspaceRootLifecycle::Persistent,
                    scope: WorkspaceRootScope::Chat,
                    mount: WorkspaceRootMount::ProjectedOverlay,
                    visible: visible.contains(*root),
                    writable: writable.contains(*root),
                    commit: WorkspaceRootCommit::OnRunCompleted,
                }
            } else {
                WorkspaceRootSpec {
                    path: root.to_string(),
                    lifecycle: WorkspaceRootLifecycle::Run,
                    scope: WorkspaceRootScope::Run,
                    mount: WorkspaceRootMount::Materialized,
                    visible: visible.contains(*root),
                    writable: writable.contains(*root),
                    commit: WorkspaceRootCommit::Never,
                }
            }
        })
        .collect()
}

pub fn commit_policy_from_profile(_profile: &ResolvedAgentProfile) -> CommitPolicy {
    CommitPolicy {
        default_target: ArtifactTarget::MessageBody,
        combine_template: None,
        store_artifacts_in_extra: true,
    }
}

fn validate_skill_name(name: &str) -> Result<(), ApplicationError> {
    let name = name.trim();
    if name.is_empty()
        || name.len() > 128
        || !name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
    {
        return Err(ApplicationError::ValidationError(format!(
            "agent.profile_skill_name_invalid: invalid Skill name `{name}`"
        )));
    }
    Ok(())
}

fn validate_artifact_id(id: &str) -> Result<(), ApplicationError> {
    let id = id.trim();
    if id.is_empty()
        || id.len() > 128
        || !id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
    {
        return Err(ApplicationError::ValidationError(format!(
            "agent.profile_artifact_id_invalid: invalid artifact id `{id}`"
        )));
    }
    Ok(())
}
