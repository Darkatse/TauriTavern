//! Builtin Agent tools exposed as shell commands.
//!
//! An invocation's visible tools become `builtin.<tool>` commands, so a
//! script or shell expression can reach the same tools the model calls.
//! Registration is the only gate: a tool the invocation cannot see is never
//! registered, and an unregistered name is an ordinary `command not found`.

use std::sync::Arc;

use async_trait::async_trait;
use bashkit::{Builtin, BuiltinContext, ExecResult};
use tt_ports::workspace_shell::WorkspaceShellTools;

/// Prefix that keeps these commands clear of bashkit's own builtins and shell
/// utilities, and marks where the command came from.
const PREFIX: &str = "builtin.";

/// Exit code for arguments that are not one JSON object.
const USAGE_EXIT: i32 = 2;
/// Exit code for a tool that ran and reported an error.
const TOOL_FAILED_EXIT: i32 = 1;

const LLM_HINT: &str = "Run one Agent tool by native name with a single JSON object argument.";

/// Register one command per visible tool onto `builder`.
///
/// Returns the builder unchanged when the invocation exposes no tools.
pub(crate) fn register(
    mut builder: bashkit::BashBuilder,
    tools: Option<&Arc<dyn WorkspaceShellTools>>,
) -> bashkit::BashBuilder {
    let Some(tools) = tools else {
        return builder;
    };
    for name in tools.visible() {
        builder = builder.builtin(
            format!("{PREFIX}{name}"),
            Box::new(ToolCommand {
                tools: tools.clone(),
                name: name.clone(),
            }),
        );
    }
    builder
}

struct ToolCommand {
    tools: Arc<dyn WorkspaceShellTools>,
    name: String,
}

#[async_trait]
impl Builtin for ToolCommand {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let Some(argument) = ctx.args.first() else {
            return Ok(ExecResult::err(
                format!(
                    "{}: expected one JSON argument, as in {} '{{}}'\n",
                    self.command(),
                    self.command()
                ),
                USAGE_EXIT,
            ));
        };
        // The shell already split quoting; one argument is the whole payload.
        let args = match serde_json::from_str::<serde_json::Value>(argument) {
            Ok(value @ serde_json::Value::Object(_)) => value,
            Ok(_) => {
                return Ok(ExecResult::err(
                    format!("{}: arguments must be a JSON object\n", self.command()),
                    USAGE_EXIT,
                ));
            }
            Err(error) => {
                return Ok(ExecResult::err(
                    format!("{}: invalid JSON arguments: {error}\n", self.command()),
                    USAGE_EXIT,
                ));
            }
        };
        match self.tools.call(&self.name, args).await {
            Ok(text) => Ok(ExecResult::ok(text)),
            Err(error) => Ok(ExecResult::err(
                format!("{}: {error}\n", self.command()),
                TOOL_FAILED_EXIT,
            )),
        }
    }

    fn llm_hint(&self) -> Option<&'static str> {
        Some(LLM_HINT)
    }
}

impl ToolCommand {
    fn command(&self) -> String {
        format!("{PREFIX}{}", self.name)
    }
}
