//! MCP servers exposed as shell commands.
//!
//! Three fixed commands cover the whole capability: discover, assert, call.
//! The server is an argument rather than part of the command name, because an
//! MCP server has no stable shell-safe identifier of its own: the registration
//! id is a locally generated UUID and the display name is user-chosen free text.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bashkit::{Builtin, BuiltinContext, ExecResult};
use serde_json::json;
use tt_ports::workspace_shell::{
    WorkspaceShellMcp, WorkspaceShellMcpOutcome, WorkspaceShellMcpServer,
};

/// Shown whenever the server selector is missing or repeated.
const SELECTOR_HELP: &str = "Choose the server with --server <name-or-id>, --name <display-name>, or --id <registration-id>.";

/// Exit code for a malformed command line.
const USAGE_EXIT: i32 = 2;
/// Exit code for an MCP call that failed for a reason other than the timeout.
const CALL_FAILED_EXIT: i32 = 1;

/// Exit code when our own deadline fired. Matches bashkit's `timeout`, and marks
/// a call whose remote effect is unknown rather than known-absent.
const TIMEOUT_EXIT: i32 = 124;

const LLM_HINT: &str =
    "Reach MCP servers: mcp.list, mcp.check, and mcp.invoke with a JSON argument object.";

/// Register the three MCP commands onto `builder`.
///
/// Returns the builder unchanged when the invocation reaches no MCP server, so
/// an invocation without MCP has no `mcp.*` commands at all.
pub(crate) fn register(
    mut builder: bashkit::BashBuilder,
    mcp: Option<&Arc<dyn WorkspaceShellMcp>>,
) -> bashkit::BashBuilder {
    let Some(mcp) = mcp else {
        return builder;
    };
    for (name, action) in [
        ("mcp.list", Action::List),
        ("mcp.check", Action::Check),
        ("mcp.invoke", Action::Invoke),
    ] {
        builder = builder.builtin(
            name,
            Box::new(McpCommand {
                mcp: mcp.clone(),
                action,
            }),
        );
    }
    builder
}

#[derive(Debug, Clone, Copy)]
enum Action {
    List,
    Check,
    Invoke,
}

/// Which field a server selector matches against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectorKind {
    /// Try both, and treat any ambiguity as an error.
    Server,
    Name,
    Id,
}

impl SelectorKind {
    fn label(self) -> &'static str {
        match self {
            Self::Server => "--server",
            Self::Name => "--name",
            Self::Id => "--id",
        }
    }
}

#[derive(Debug)]
struct Parsed {
    kind: SelectorKind,
    value: String,
    tools: Vec<String>,
    json: Option<String>,
    timeout: Option<u64>,
}

struct McpCommand {
    mcp: Arc<dyn WorkspaceShellMcp>,
    action: Action,
}

#[async_trait]
impl Builtin for McpCommand {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let parsed = match parse(ctx.args, self.action) {
            Ok(parsed) => parsed,
            Err(message) => return Ok(ExecResult::err(format!("{message}\n"), USAGE_EXIT)),
        };
        let servers =
            self.mcp.servers().await.map_err(|error| {
                std::io::Error::other(format!("mcp: cannot read servers: {error}"))
            })?;
        match self.action {
            Action::List => Ok(ExecResult::ok(render_list(&servers))),
            Action::Check => Ok(check(&servers, &parsed)),
            Action::Invoke => self.invoke(&servers, &parsed).await,
        }
    }

    fn llm_hint(&self) -> Option<&'static str> {
        Some(LLM_HINT)
    }
}

impl McpCommand {
    async fn invoke(
        &self,
        servers: &[WorkspaceShellMcpServer],
        parsed: &Parsed,
    ) -> bashkit::Result<ExecResult> {
        let server = match resolve(servers, parsed) {
            Ok(server) => server,
            Err(message) => return Ok(ExecResult::err(format!("{message}\n"), USAGE_EXIT)),
        };
        let Some(tool) = parsed.tools.first() else {
            return Ok(ExecResult::err(
                "mcp.invoke: one tool name is required\n".to_string(),
                USAGE_EXIT,
            ));
        };
        let args = match parsed.json.as_deref() {
            None => json!({}),
            Some(text) => match serde_json::from_str::<serde_json::Value>(text) {
                Ok(value @ serde_json::Value::Object(_)) => value,
                Ok(_) => {
                    return Ok(ExecResult::err(
                        "mcp.invoke: arguments must be a JSON object\n".to_string(),
                        USAGE_EXIT,
                    ));
                }
                Err(error) => {
                    return Ok(ExecResult::err(
                        format!("mcp.invoke: arguments are not valid JSON: {error}\n"),
                        USAGE_EXIT,
                    ));
                }
            },
        };
        let call = self.mcp.call(&server.id, tool, args);
        let Some(seconds) = parsed.timeout else {
            return match call.await {
                Ok(outcome) => Ok(report(outcome)),
                Err(error) => Ok(ExecResult::err(
                    format!("mcp.invoke: {error}\n"),
                    CALL_FAILED_EXIT,
                )),
            };
        };
        // Our own deadline fires well before the shell's, so exceeding it is
        // reported as this call's timeout and the script keeps running.
        match tokio::time::timeout(Duration::from_secs(seconds), call).await {
            Ok(Ok(outcome)) => Ok(report(outcome)),
            Ok(Err(error)) => Ok(ExecResult::err(
                format!("mcp.invoke: {error}\n"),
                CALL_FAILED_EXIT,
            )),
            Err(_) => Ok(ExecResult::err(
                format!(
                    "mcp.invoke: no response from `{tool}` within {seconds}s. The remote tool may still be running, so inspect any affected state before retrying.\n"
                ),
                TIMEOUT_EXIT,
            )),
        }
    }
}

/// Turn one call outcome into the shell result, keeping the two failure kinds
/// apart: a refused call is safe to run again, an unconfirmed one is not.
fn report(outcome: WorkspaceShellMcpOutcome) -> ExecResult {
    match outcome {
        WorkspaceShellMcpOutcome::Answered(text) => ExecResult::ok(text),
        WorkspaceShellMcpOutcome::Refused(message) => {
            ExecResult::err(format!("mcp.invoke: {message}\n"), CALL_FAILED_EXIT)
        }
        WorkspaceShellMcpOutcome::Unconfirmed(message) => {
            ExecResult::err(format!("mcp.invoke: {message}\n"), TIMEOUT_EXIT)
        }
    }
}
/// Parse the command line for one MCP command.
///
/// Positional arguments are always tool names; the server is always a flag, so
/// the two can never be confused.
fn parse(args: &[String], action: Action) -> Result<Parsed, String> {
    let mut kind = None;
    let mut value = None;
    let mut tools = Vec::new();
    let mut json = None;
    let mut timeout = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--server" | "--name" | "--id" => {
                let this = match arg.as_str() {
                    "--server" => SelectorKind::Server,
                    "--name" => SelectorKind::Name,
                    _ => SelectorKind::Id,
                };
                if kind.is_some() {
                    return Err(format!(
                        "{}: give exactly one of --server, --name or --id\n{}",
                        command_name(action),
                        SELECTOR_HELP
                    ));
                }
                let Some(next) = rest.next() else {
                    return Err(format!("{} requires a value", arg));
                };
                kind = Some(this);
                value = Some(next.clone());
            }
            "--timeout" if matches!(action, Action::Invoke) => {
                let Some(next) = rest.next() else {
                    return Err("--timeout requires a value in seconds".to_string());
                };
                let seconds = next.parse::<u64>().map_err(|_| {
                    format!("--timeout must be a whole number of seconds, got `{next}`")
                })?;
                if seconds == 0 {
                    return Err("--timeout must be at least 1 second".to_string());
                }
                if seconds >= MAX_TIMEOUT_SECONDS {
                    return Err(format!(
                        "--timeout {seconds} is not allowed: the shell stops the whole command \
                         after {MAX_TIMEOUT_SECONDS}s, so a longer wait would be reported as a \
                         shell timeout rather than this call's. Use a value below {MAX_TIMEOUT_SECONDS}."
                    ));
                }
                timeout = Some(seconds);
            }
            flag if flag.starts_with("--") && flag.len() > 2 => {
                return Err(format!("unknown option `{flag}`"));
            }
            _ => {
                if matches!(action, Action::Invoke) && tools.len() == 1 && json.is_none() {
                    json = Some(arg.clone());
                } else {
                    tools.push(arg.clone());
                }
            }
        }
    }
    let Some(kind) = kind else {
        return Err(format!(
            "{}: one of --server, --name or --id is required\n{}",
            command_name(action),
            SELECTOR_HELP
        ));
    };
    Ok(Parsed {
        kind,
        value: value.unwrap_or_default(),
        tools,
        json,
        timeout,
    })
}

fn command_name(action: Action) -> &'static str {
    match action {
        Action::List => "mcp.list",
        Action::Check => "mcp.check",
        Action::Invoke => "mcp.invoke",
    }
}
const MCP_NONE: &str = "No MCP servers are available to this invocation.";

/// The shell cuts the whole command off here, so a longer wait cannot be honoured.
const MAX_TIMEOUT_SECONDS: u64 = 30;

/// Resolve exactly one server, or explain why the selection is ambiguous.
///
/// Matching more than one server is always an error, whatever the selector:
/// picking one silently would run the call against a server the author may not
/// have meant.
fn resolve<'a>(
    servers: &'a [WorkspaceShellMcpServer],
    parsed: &Parsed,
) -> Result<&'a WorkspaceShellMcpServer, String> {
    let matches = servers
        .iter()
        .filter(|server| match parsed.kind {
            SelectorKind::Server => server.name == parsed.value || server.id == parsed.value,
            SelectorKind::Name => server.name == parsed.value,
            SelectorKind::Id => server.id == parsed.value,
        })
        .collect::<Vec<_>>();
    match matches.len() {
        1 => Ok(matches[0]),
        0 => Err(format!(
            "no MCP server matches {} `{}`{}",
            parsed.kind.label(),
            parsed.value,
            render_servers(servers)
        )),
        count => {
            let mut message = format!(
                "{} `{}` matches {} servers, but a selector must match exactly one. Use --id:",
                parsed.kind.label(),
                parsed.value,
                count
            );
            for server in &matches {
                message.push_str(&format!("\n  {}  {}", server.id, server.name));
            }
            Err(message)
        }
    }
}

fn check(servers: &[WorkspaceShellMcpServer], parsed: &Parsed) -> ExecResult {
    let server = match resolve(servers, parsed) {
        Ok(server) => server,
        Err(message) => return ExecResult::err(format!("{message}\n"), USAGE_EXIT),
    };
    if parsed.tools.is_empty() {
        return ExecResult::err(
            "mcp.check: at least one tool name is required\n".to_string(),
            USAGE_EXIT,
        );
    }
    let missing = parsed
        .tools
        .iter()
        .filter(|tool| !server.tools.contains(tool))
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return ExecResult::ok(format!(
            "{} provides: {}\n",
            server.name,
            parsed.tools.join(", ")
        ));
    }
    let available = if server.tools.is_empty() {
        "(none)".to_string()
    } else {
        server.tools.join(", ")
    };
    ExecResult::err(
        format!(
            "{} is missing {} of {} tools: {}\navailable: {}\n",
            server.name,
            missing.len(),
            parsed.tools.len(),
            missing.join(", "),
            available
        ),
        CALL_FAILED_EXIT,
    )
}

fn render_list(servers: &[WorkspaceShellMcpServer]) -> String {
    if servers.is_empty() {
        return format!("{MCP_NONE}\n");
    }
    let mut out = String::new();
    for server in servers {
        out.push_str(&format!("{}  {}\n", server.id, server.name));
        for tool in &server.tools {
            out.push_str(&format!("  {tool}\n"));
        }
    }
    out
}

fn render_servers(servers: &[WorkspaceShellMcpServer]) -> String {
    if servers.is_empty() {
        return format!("\n{MCP_NONE}");
    }
    let mut out = String::from("\nAvailable servers:");
    for server in servers {
        out.push_str(&format!("\n  {}  {}", server.id, server.name));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(id: &str, name: &str, tools: &[&str]) -> WorkspaceShellMcpServer {
        WorkspaceShellMcpServer {
            id: id.to_string(),
            name: name.to_string(),
            tools: tools.iter().map(|tool| tool.to_string()).collect(),
        }
    }

    fn parsed(kind: SelectorKind, value: &str, tools: &[&str]) -> Parsed {
        Parsed {
            kind,
            value: value.to_string(),
            tools: tools.iter().map(|tool| tool.to_string()).collect(),
            json: None,
            timeout: None,
        }
    }

    #[test]
    fn a_unique_name_resolves_to_its_server() {
        let servers = vec![
            server("id-a", "Alpha", &["search"]),
            server("id-b", "Beta", &[]),
        ];
        let resolved = resolve(&servers, &parsed(SelectorKind::Server, "Alpha", &[])).unwrap();
        assert_eq!(resolved.id, "id-a");
    }

    #[test]
    fn an_id_resolves_when_no_name_matches() {
        let servers = vec![server("id-a", "Alpha", &[])];
        let resolved = resolve(&servers, &parsed(SelectorKind::Server, "id-a", &[])).unwrap();
        assert_eq!(resolved.name, "Alpha");
    }

    /// The case the design calls out: a name that happens to equal another
    /// server's id must not silently win.
    #[test]
    fn a_name_equal_to_another_servers_id_is_rejected_as_ambiguous() {
        let servers = vec![
            server("id-a", "looks-like-an-id", &[]),
            server("looks-like-an-id", "Other", &[]),
        ];
        let error = resolve(
            &servers,
            &parsed(SelectorKind::Server, "looks-like-an-id", &[]),
        )
        .unwrap_err();
        assert!(error.contains("matches 2 servers"), "{error}");
    }

    #[test]
    fn duplicate_names_are_rejected_rather_than_guessed() {
        let servers = vec![server("id-a", "Same", &[]), server("id-b", "Same", &[])];
        for kind in [SelectorKind::Server, SelectorKind::Name] {
            let error = resolve(&servers, &parsed(kind, "Same", &[])).unwrap_err();
            assert!(error.contains("matches 2 servers"), "{error}");
            assert!(error.contains("id-a") && error.contains("id-b"), "{error}");
        }
    }

    #[test]
    fn an_unknown_name_lists_the_available_servers() {
        let servers = vec![server("id-a", "Alpha", &[])];
        let error = resolve(&servers, &parsed(SelectorKind::Server, "missing", &[])).unwrap_err();
        assert!(error.contains("no MCP server matches"), "{error}");
        assert!(error.contains("id-a") && error.contains("Alpha"), "{error}");
    }

    #[test]
    fn a_name_selector_does_not_fall_back_to_ids() {
        let servers = vec![server("id-a", "Alpha", &[])];
        let error = resolve(&servers, &parsed(SelectorKind::Name, "id-a", &[])).unwrap_err();
        assert!(error.contains("no MCP server matches --name"), "{error}");
    }

    #[test]
    fn check_reports_the_tools_that_are_missing() {
        let servers = vec![server("id-a", "Alpha", &["search", "fetch"])];
        let result = check(
            &servers,
            &parsed(SelectorKind::Server, "Alpha", &["search", "list"]),
        );
        assert_eq!(result.exit_code, CALL_FAILED_EXIT);
        let text = result.stderr.text_lossy().into_owned();
        assert!(text.contains("missing 1 of 2"), "{text}");
        assert!(text.contains("list"), "{text}");
    }

    #[test]
    fn check_passes_when_every_requested_tool_is_present() {
        let servers = vec![server("id-a", "Alpha", &["search", "fetch"])];
        let result = check(
            &servers,
            &parsed(SelectorKind::Server, "Alpha", &["search", "fetch"]),
        );
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn a_timeout_at_or_above_the_shell_budget_is_refused() {
        let args = vec![
            "--server".to_string(),
            "Alpha".to_string(),
            "search".to_string(),
            "{}".to_string(),
            "--timeout".to_string(),
            "30".to_string(),
        ];
        let error = parse(&args, Action::Invoke).unwrap_err();
        assert!(error.contains("not allowed"), "{error}");
    }

    #[test]
    fn a_missing_or_repeated_selector_is_refused() {
        let missing = parse(&["search".to_string()], Action::Check).unwrap_err();
        assert!(missing.contains("one of --server"), "{missing}");
        let repeated = parse(
            &[
                "--server".to_string(),
                "Alpha".to_string(),
                "--id".to_string(),
                "id-a".to_string(),
            ],
            Action::Check,
        )
        .unwrap_err();
        assert!(repeated.contains("exactly one"), "{repeated}");
    }

    /// A refused call is safe to run again; an unconfirmed one may already have
    /// taken effect. Collapsing them would let a script retry a side effect.
    #[test]
    fn a_refused_call_and_an_unconfirmed_call_get_different_exit_codes() {
        let refused = report(WorkspaceShellMcpOutcome::Refused("nope".to_string()));
        let unconfirmed = report(WorkspaceShellMcpOutcome::Unconfirmed("lost".to_string()));
        assert_eq!(refused.exit_code, CALL_FAILED_EXIT);
        assert_eq!(unconfirmed.exit_code, TIMEOUT_EXIT);
        assert_ne!(refused.exit_code, unconfirmed.exit_code);
    }

    #[test]
    fn an_answered_call_writes_its_text_and_succeeds() {
        let answered = report(WorkspaceShellMcpOutcome::Answered("hello\n".to_string()));
        assert_eq!(answered.exit_code, 0);
        assert_eq!(answered.stdout.text_lossy(), "hello\n");
    }

    #[test]
    fn invoke_reads_the_tool_then_the_json_argument() {
        let args = vec![
            "--server".to_string(),
            "Alpha".to_string(),
            "search".to_string(),
            r#"{"query":"x"}"#.to_string(),
        ];
        let parsed = parse(&args, Action::Invoke).unwrap();
        assert_eq!(parsed.tools, vec!["search"]);
        assert_eq!(parsed.json.as_deref(), Some(r#"{"query":"x"}"#));
    }
}
