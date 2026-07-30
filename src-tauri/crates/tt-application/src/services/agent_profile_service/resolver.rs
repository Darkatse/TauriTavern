use crate::errors::ApplicationError;
use tt_domain::models::agent::AgentToolSpec;
use tt_domain::models::agent::profile::{
    AgentProfileDefinition, AgentProfileId, AgentProfileSourceTrace, DEFAULT_AGENT_PROFILE_ID,
    ResolvedAgentProfile,
};

use super::defaults::default_writer_profile;
use super::output_policy::resolve_output_policy;
use super::preset_refs::validate_preset_binding;
use super::validation::{
    migrate_profile_schema, normalize_context_policy, validate_delegation_policy,
    validate_instructions, validate_model_binding, validate_plan_policy, validate_profile_header,
    validate_run_policy, validate_skill_policy, validate_tool_policy, validate_workspace_policy,
};
use super::{AgentProfileExternalReferencePolicy, AgentProfileResolveInput, AgentProfileService};

impl AgentProfileService {
    pub async fn resolve_profile(
        &self,
        input: AgentProfileResolveInput<'_>,
    ) -> Result<ResolvedAgentProfile, ApplicationError> {
        let (definition, source) = self.load_definition(input.profile_id).await?;

        self.resolve_definition(
            definition,
            source,
            input.known_tools,
            AgentProfileExternalReferencePolicy::Strict,
        )
        .await
    }

    pub async fn resolve_profile_for_preview(
        &self,
        input: AgentProfileResolveInput<'_>,
    ) -> Result<ResolvedAgentProfile, ApplicationError> {
        let (definition, source) = self.load_definition(input.profile_id).await?;

        self.resolve_definition(
            definition,
            source,
            input.known_tools,
            AgentProfileExternalReferencePolicy::AllowDangling,
        )
        .await
    }

    pub async fn list_resolved_profiles_for_discovery(
        &self,
        known_tools: &[AgentToolSpec],
    ) -> Result<Vec<ResolvedAgentProfile>, ApplicationError> {
        let summaries = self.list_profiles().await?.profiles;
        let mut profiles = Vec::with_capacity(summaries.len());
        for summary in summaries {
            match self
                .resolve_profile(AgentProfileResolveInput {
                    profile_id: Some(summary.id.as_str()),
                    known_tools,
                })
                .await
            {
                Ok(profile) => profiles.push(profile),
                Err(error) if profile_discovery_can_omit(&error) => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(profiles)
    }

    async fn load_definition(
        &self,
        profile_id: Option<&str>,
    ) -> Result<(AgentProfileDefinition, String), ApplicationError> {
        let requested = profile_id.map(str::trim).filter(|value| !value.is_empty());
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
        Ok((definition, source))
    }

    pub(super) async fn resolve_definition(
        &self,
        mut definition: AgentProfileDefinition,
        source: String,
        known_tools: &[AgentToolSpec],
        external_reference_policy: AgentProfileExternalReferencePolicy,
    ) -> Result<ResolvedAgentProfile, ApplicationError> {
        migrate_profile_schema(&mut definition)?;
        validate_profile_header(&definition)?;
        validate_preset_binding(
            &definition.preset,
            self.preset_repository.as_ref(),
            external_reference_policy,
        )
        .await?;
        validate_model_binding(&definition.model)?;
        normalize_context_policy(&mut definition.context)?;
        validate_instructions(&definition.instructions)?;
        validate_plan_policy(&definition.plan)?;
        validate_tool_policy(&definition.tools, known_tools)?;
        validate_delegation_policy(&definition.delegation, &definition.tools)?;
        validate_run_policy(&definition.run, &definition.delegation, &definition.tools)?;
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
            context: definition.context,
            delegation: definition.delegation,
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

fn profile_discovery_can_omit(error: &ApplicationError) -> bool {
    matches!(
        error,
        ApplicationError::ValidationError(_) | ApplicationError::NotFound(_)
    )
}
