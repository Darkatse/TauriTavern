//! Builtin Agent tools reachable from inside a workspace shell.
//!
//! This is the application-side implementation of the shell's tool port. It
//! routes every command through the ordinary dispatcher, so a shell command and
//! a model tool call take the same path, and it exposes only the tools the
//! invocation already admits.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::skill::SkillIndexEntry;
use tt_domain::models::tool::{ToolArguments, ToolId, ToolInvocation};
use tt_ports::workspace_fs::WorkspaceFs;
use tt_ports::workspace_shell::{WorkspaceShellContext, WorkspaceShellTools};

use super::AgentToolDispatcher;
use super::session::AgentToolSession;

/// Builtins a shell command must never reach, whatever the profile admits.
///
/// The first four need `&mut AgentToolSession` read tracking to stay correct;
/// the rest drive the run's control flow or delegation protocol, which a nested
/// command cannot participate in.
const SHELL_DENIED_TOOLS: [&str; 10] = [
    "workspace.shell",
    "workspace.read_file",
    "workspace.write_file",
    "workspace.apply_patch",
    "workspace.commit",
    "workspace.finish",
    "agent.delegate",
    "agent.await",
    "agent.handoff",
    "task.return",
];

/// Names an invocation admits to its shells, in snapshot order.
pub(crate) fn shell_visible_tools(snapshot: &[ToolId]) -> Vec<String> {
    let mut names = Vec::new();
    for tool_id in snapshot {
        if !tool_id.is_builtin() {
            continue;
        }
        let name = tool_id.native_name();
        if SHELL_DENIED_TOOLS.contains(&name) || names.iter().any(|seen| seen == name) {
            continue;
        }
        names.push(name.to_string());
    }
    names
}

/// The builtin tools one invocation exposes to its shells.
pub(crate) struct ShellTools {
    dispatcher: Arc<AgentToolDispatcher>,
    profile: ResolvedAgentProfile,
    run_id: String,
    files: Arc<dyn WorkspaceFs>,
    /// Frozen chat facts the tool handlers read, such as macros.
    context: Arc<WorkspaceShellContext>,
    /// Skills of the owning invocation, which gate skill-file access.
    skills: Arc<[SkillIndexEntry]>,
    names: Vec<String>,
}

impl std::fmt::Debug for ShellTools {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ShellTools")
            .field("run_id", &self.run_id)
            .field("names", &self.names)
            .finish_non_exhaustive()
    }
}

impl ShellTools {
    pub(crate) fn new(
        dispatcher: Arc<AgentToolDispatcher>,
        profile: ResolvedAgentProfile,
        run_id: String,
        files: Arc<dyn WorkspaceFs>,
        context: Arc<WorkspaceShellContext>,
        skills: Arc<[SkillIndexEntry]>,
        names: Vec<String>,
    ) -> Self {
        Self {
            dispatcher,
            profile,
            run_id,
            files,
            context,
            skills,
            names,
        }
    }

    fn session(&self) -> AgentToolSession {
        // A shell call is not part of the model's turn, so it starts from a
        // fresh session: it must not observe or disturb model read tracking.
        // It keeps the invocation's skills, which gate skill-file access in
        // the scoped workspace.
        let mut session = AgentToolSession::new(self.skills.to_vec());
        session.runtime_context = self.context.clone();
        session
    }
}

#[async_trait]
impl WorkspaceShellTools for ShellTools {
    async fn call(&self, name: &str, args: Value) -> Result<String, DomainError> {
        // The adapter registers exactly the names in `visible()`, so a name that
        // reaches here is already admitted; visibility is enforced by construction
        // rather than re-checked. This rejects a non-object payload instead.
        let Value::Object(arguments) = args else {
            return Err(DomainError::InvalidData(
                "tool.invalid_arguments: arguments must be a JSON object".to_string(),
            ));
        };
        let call = ToolInvocation {
            call_id: format!("shell:{}", self.run_id),
            tool_id: ToolId::builtin(name)?,
            arguments: ToolArguments::Object(arguments.clone()),
            provider_metadata: Value::Null,
        };
        let mut session = self.session();
        // Nothing outside the run cancels a shell tool call; the enclosing
        // shell already stops at its own command boundary.
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let outcome = self
            .dispatcher
            .dispatch(
                &self.run_id,
                &call,
                &arguments,
                &mut session,
                &self.profile,
                self.files.clone(),
                cancel_rx,
                None,
            )
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
        if outcome.result.is_error {
            return Err(DomainError::InvalidData(
                outcome
                    .result
                    .error_code
                    .clone()
                    .unwrap_or_else(|| "tool.failed".to_string()),
            ));
        }
        Ok(outcome.result.content)
    }

    fn visible(&self) -> &[String] {
        &self.names
    }
}

#[cfg(test)]
mod tests {
    use tt_domain::models::tool::ToolId;

    use super::shell_visible_tools;

    fn ids(names: &[&str]) -> Vec<ToolId> {
        names
            .iter()
            .map(|name| ToolId::builtin(name).expect("builtin name"))
            .collect()
    }

    #[test]
    fn shell_visibility_keeps_snapshot_order_without_duplicates() {
        let visible = shell_visible_tools(&ids(&[
            "dice.roll",
            "chat.search",
            "dice.roll",
            "worldinfo.read_activated",
        ]));
        assert_eq!(
            visible,
            vec!["dice.roll", "chat.search", "worldinfo.read_activated"]
        );
    }

    /// The documented shell command set, pinned to the real registry.
    ///
    /// Every builtin is either reachable or documented as excluded.
    ///
    /// This pins the registry to the lists in docs/Agent/ToolSystem.md: adding a
    /// builtin, or changing a name, fails here until the commands are classified.
    /// Documenting which builtins are reachable is what keeps a shell author from
    /// guessing, so the two must move together.
    #[test]
    fn every_builtin_is_classified_as_reachable_or_excluded() {
        let registry = crate::services::agent_tools::BuiltinAgentToolRegistry::all();
        let all = registry
            .catalog()
            .iter()
            .map(|descriptor| descriptor.id.native_name().to_string())
            .collect::<Vec<_>>();
        let exposed = shell_visible_tools(
            &all.iter()
                .map(|name| ToolId::builtin(name).expect("builtin name"))
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            exposed,
            vec![
                "chat.read_messages",
                "chat.search",
                "dice.roll",
                "workspace.list_files",
                "workspace.search_files",
                "worldinfo.read_activated",
            ]
        );
        let excluded = all
            .iter()
            .filter(|name| !exposed.contains(name))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            excluded,
            vec![
                "agent.await",
                "agent.delegate",
                "agent.handoff",
                "task.return",
                "workspace.apply_patch",
                "workspace.commit",
                "workspace.finish",
                "workspace.read_file",
                "workspace.shell",
                "workspace.write_file",
            ]
        );
        assert_eq!(exposed.len() + excluded.len(), all.len());
    }
}
