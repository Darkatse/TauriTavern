use std::collections::{HashMap, HashSet};
use std::ops::Range;

use serde::Serialize;
use serde_json::{Value, json};
use tt_domain::models::agent::session::{
    AgentSessionCompaction, AgentSessionMessage, AgentSessionMessageOrigin, NewAgentSessionMessage,
};
use tt_domain::models::agent::{
    AgentModelContentPart, AgentModelMessage, AgentModelRequest, AgentModelRole, AgentModelTool,
    AgentRunEventLevel, AgentRunStatus,
};
use tt_domain::models::tool::ToolChoice;

use crate::dto::agent_dto::AgentPromptAssemblyBrokerRequestDto;
use crate::dto::tokenization_dto::{OpenAiTokenCountBatchItemDto, OpenAiTokenCountBatchRequestDto};
use crate::errors::ApplicationError;
use crate::services::agent_model_gateway::{call_model_alias, reset_transport_for_resume};
use crate::services::prompt_assembly_service::sha256_value;

use super::prompt_snapshot::AgentPromptRequest;
use super::{AgentCancelReceiver, AgentRuntimeService, PreparedInvocation};

const COMPACT_SHARE: f64 = 0.9;
const KEPT_TOKENS: usize = 20_000;

const COMPACTION_REQUEST: &str = "Context compaction: write a summary for yourself. The earlier part of this conversation is about to leave your context, and you will continue from the summary, with only the latest messages kept as they are. Reply with the summary alone; do not call tools and do not continue the task.

Use these sections:
## Goal
What the user wants: their requests in their own words, and the constraints and preferences they gave.
## Done
What is finished: each change you made to the app, its settings or files, and what it does.
## In progress
What you were doing as you wrote this, and how far it got.
## Findings and decisions
What you found out, what was chosen and why, and what was tried and dropped.
## Current state
What the app showed when you last looked: the open page or panel, the relevant settings, and any errors.
## Next steps
What is left, in order.

Keep exact names, paths, selectors, setting keys, values and error messages. Leave out what you can observe again with your tools: you will check the app before you act on it.";
const EARLIER_SUMMARY: &str = "\n\nYour context begins with an earlier summary. Keep from it what still holds, and add what happened since.";
const COMPACTION_PREFIX: &str = "The earlier part of this conversation was compacted. Below is the summary you wrote as it left your context: your own notes, not a message from the user. The messages after it are as they were. Observe the app again before you rely on the state it describes.\n\n";

#[derive(Debug)]
pub(super) struct SessionInvocation {
    pub assembly_request: AgentPromptAssemblyBrokerRequestDto,
    pub point: usize,
    pub tokenizer_model: String,
    /// Replaced after every model response, so a request rebuilt by compaction is never counted against a stale anchor.
    pub usage_anchor: Option<UsageAnchor>,
}

#[derive(Debug)]
pub(super) struct UsageAnchor {
    tokens: usize,
    messages: usize,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CompactionTrigger {
    Point,
    Overflow,
}

impl SessionInvocation {
    pub fn new(
        assembly_request: AgentPromptAssemblyBrokerRequestDto,
        snapshot: &Value,
    ) -> Result<Self, ApplicationError> {
        let limits = &snapshot["contextLimits"];
        let missing = || {
            ApplicationError::ValidationError(
            "agent.session_context_limits_missing: Session prompt snapshot requires contextLimits with positive context and response sizes and a tokenizerModel".into()
        )
        };
        let context = limits["context"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or_else(missing)? as usize;
        let response = limits["response"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or_else(missing)? as usize;
        let tokenizer_model = limits["tokenizerModel"]
            .as_str()
            .filter(|model| !model.is_empty())
            .ok_or_else(missing)?
            .to_owned();
        if context <= response {
            return Err(ApplicationError::ValidationError("agent.session_context_limits_invalid: contextLimits.context must exceed contextLimits.response".into()));
        }
        Ok(Self {
            assembly_request,
            point: compaction_point(context, response),
            tokenizer_model,
            usage_anchor: None,
        })
    }

    pub fn observe_usage(&mut self, metadata: &Value, messages: usize) {
        let usage = &metadata["usage"];
        self.usage_anchor = usage["prompt_tokens"]
            .as_u64()
            .zip(usage["completion_tokens"].as_u64())
            .map(|(input, output)| UsageAnchor {
                tokens: (input + output) as usize,
                messages,
            });
    }
}

fn compaction_point(context: usize, response: usize) -> usize {
    ((context - response) as f64 * COMPACT_SHARE) as usize
}

pub(super) struct SessionGroup {
    pub range: Range<usize>,
    pub interrupted: bool,
}

pub(super) fn session_context(
    history: &[AgentSessionMessage],
) -> Result<Vec<&AgentSessionMessage>, ApplicationError> {
    let Some(index) = history.iter().rposition(|entry| entry.compaction.is_some()) else {
        return Ok(history.iter().collect());
    };
    let entry = &history[index];
    let mut context = vec![entry];
    if let Some(seq) = entry.compaction.as_ref().unwrap().kept_from_seq {
        let start = history[..index]
            .iter()
            .position(|entry| entry.seq == seq)
            .ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "agent.compaction_kept_missing: Session history entry {seq} was not found"
                ))
            })?;
        context.extend(
            history[start..index]
                .iter()
                .filter(|entry| entry.compaction.is_none()),
        );
    }
    context.extend(&history[index + 1..]);
    Ok(context)
}

pub(super) fn session_groups(
    entries: &[&AgentSessionMessage],
) -> Result<Vec<SessionGroup>, ApplicationError> {
    let mut groups = Vec::new();
    let mut index = 0;
    while index < entries.len() {
        let start = index;
        let message = &entries[index].message;
        if message.role == AgentModelRole::Tool {
            return Err(ApplicationError::ValidationError(
                "agent.tool_history_orphan: tool result has no preceding call".into(),
            ));
        }
        let mut pending = HashSet::new();
        for part in &message.parts {
            if let AgentModelContentPart::ToolCall { call } = part
                && !pending.insert(call.call_id.as_str())
            {
                return Err(ApplicationError::ValidationError(
                    "agent.tool_history_duplicate: repeated call id".into(),
                ));
            }
        }
        index += 1;
        while index < entries.len() && entries[index].message.role == AgentModelRole::Tool {
            let mut results = entries[index]
                .message
                .parts
                .iter()
                .filter_map(|part| match part {
                    AgentModelContentPart::ToolResult { result } => Some(result),
                    _ => None,
                });
            let result = results.next();
            if results.next().is_some()
                || result.is_none_or(|result| !pending.remove(result.call_id.as_str()))
            {
                return Err(ApplicationError::ValidationError(
                    "agent.tool_history_orphan: tool result does not match an unanswered call"
                        .into(),
                ));
            }
            index += 1;
        }
        groups.push(SessionGroup {
            range: start..index,
            interrupted: !pending.is_empty(),
        });
    }
    Ok(groups)
}

pub(super) fn session_history_groups(
    entries: &[&AgentSessionMessage],
) -> Result<Vec<Vec<AgentModelMessage>>, ApplicationError> {
    session_groups(entries)?.into_iter().map(|group| {
        let entries = &entries[group.range];
        if !group.interrupted {
            return Ok(entries.iter().map(|entry| entry.message.clone()).collect());
        }
        let results = entries.iter().flat_map(|entry| &entry.message.parts)
            .filter_map(|part| match part {
                AgentModelContentPart::ToolResult { result } => Some((result.call_id.as_str(), result)),
                _ => None,
            }).collect::<HashMap<_, _>>();
        let mut lines = vec!["The previous turn stopped. Actions without recorded results have an unknown outcome; check the current state before repeating them.".to_string()];
        for entry in entries {
            for part in &entry.message.parts {
                match part {
                    AgentModelContentPart::Text { text } => lines.push(text.clone()),
                    AgentModelContentPart::ToolCall { call } => {
                        let alias = call.provider_metadata.get("modelAlias").and_then(Value::as_str)
                            .ok_or_else(|| ApplicationError::ValidationError(
                                "agent.tool_history_alias_missing: historical call has no modelAlias".into()
                            ))?;
                        let outcome = match results.get(call.call_id.as_str()) {
                            Some(result) => format!("Recorded {}: {}", if result.is_error { "error" } else { "result" }, result.content),
                            None => "Outcome unknown: no recorded result.".into(),
                        };
                        let arguments = serde_json::to_string(&call.arguments)
                            .expect("tool arguments contain JSON values or a string");
                        lines.push(format!("{alias}({arguments})\n{outcome}"));
                    }
                    _ => {}
                }
            }
        }
        Ok(vec![AgentModelMessage {
            role: AgentModelRole::Assistant,
            parts: vec![AgentModelContentPart::Text { text: lines.join("\n\n") }],
            provider_metadata: Value::Null,
        }])
    }).collect()
}

fn message_token_input(
    message: &AgentModelMessage,
    tools: &[AgentModelTool],
) -> Result<Value, ApplicationError> {
    let mut content = String::new();
    for part in &message.parts {
        match part {
            AgentModelContentPart::Text { text }
            | AgentModelContentPart::Reasoning {
                text: Some(text), ..
            } => content.push_str(text),
            AgentModelContentPart::ToolCall { call } => {
                content.push_str(call_model_alias(call, tools)?);
                content.push_str(
                    &serde_json::to_string(&call.arguments)
                        .expect("tool arguments are serializable"),
                );
            }
            AgentModelContentPart::ToolResult { result } => content.push_str(&result.content),
            AgentModelContentPart::Reasoning { text: None, .. }
            | AgentModelContentPart::Native { .. }
            | AgentModelContentPart::ResourceRef { .. }
            | AgentModelContentPart::Media { .. } => {}
        }
        content.push('\n');
    }
    Ok(json!({ "role": message.role, "content": content }))
}

pub(super) fn is_context_overflow_error(error: &ApplicationError) -> bool {
    matches!(error, ApplicationError::ValidationError(message) if message.starts_with("model.context_overflow:"))
}

fn is_output_truncated_error(error: &ApplicationError) -> bool {
    matches!(error, ApplicationError::ValidationError(message) if message.starts_with("model.output_truncated:"))
}

fn context_full(session: &SessionInvocation, tokens: usize) -> ApplicationError {
    ApplicationError::ValidationError(format!(
        "agent.context_full: The context is {tokens} tokens even with its history compacted, at or above the compaction point of {}. Raise the context size of the preset \"{}\", or choose a preset with a larger one.",
        session.point, session.assembly_request.preset_ref.name,
    ))
}

impl AgentRuntimeService {
    pub(super) async fn context_tokens(
        &self,
        request: &AgentModelRequest,
        tokenizer_model: &str,
        anchor: Option<&UsageAnchor>,
    ) -> Result<usize, ApplicationError> {
        let (base, start) = match anchor {
            Some(anchor) => (anchor.tokens, anchor.messages),
            None => (0, 0),
        };
        let mut messages = request.messages[start..]
            .iter()
            .map(|message| message_token_input(message, &request.tools))
            .collect::<Result<Vec<_>, _>>()?;
        if anchor.is_none() {
            for tool in &request.tools {
                let mut content = tool.model_alias.clone();
                if let Some(description) = &tool.description {
                    content.push_str(description);
                }
                content.push_str(&tool.input_schema.to_string());
                messages.push(json!({ "content": content }));
            }
        }
        let counts = self
            .tokenization
            .count_openai_tokens_batch(OpenAiTokenCountBatchRequestDto {
                model: tokenizer_model.to_owned(),
                requests: vec![OpenAiTokenCountBatchItemDto { messages }],
            })
            .await?;
        Ok(base + counts.token_counts[0])
    }

    async fn rebuild_session_request(
        &self,
        prepared: &PreparedInvocation,
        session: &SessionInvocation,
        groups: Vec<Vec<AgentModelMessage>>,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<AgentModelRequest, ApplicationError> {
        let mut assembly = session.assembly_request.clone();
        assembly.frozen_run_input_snapshot["promptInputs"]["sessionHistory"] = json!(groups);
        assembly.fingerprint.frozen_run_input_snapshot_sha256 =
            sha256_value(&assembly.frozen_run_input_snapshot)?;
        let admitted = self
            .request_host_prompt_assembly(
                &prepared.invocation,
                prepared.delegation_task_id.clone(),
                assembly,
                cancel,
            )
            .await?;
        let request = AgentPromptRequest {
            payload: prepared.request.payload.clone(),
            messages: admitted.messages,
        };
        self.prepare_invocation_model_request(
            request,
            &prepared.invocation,
            &prepared.profile,
            &prepared.effective_skills,
            &prepared.request.tools,
            prepared.request.tool_choice.clone(),
        )
        .await
    }

    async fn retained_group_start(
        &self,
        entries: &[&AgentSessionMessage],
        session: &SessionInvocation,
        tools: &[AgentModelTool],
    ) -> Result<usize, ApplicationError> {
        let groups = session_groups(entries)?;
        let requests = groups
            .iter()
            .map(|group| {
                Ok(OpenAiTokenCountBatchItemDto {
                    messages: entries[group.range.clone()]
                        .iter()
                        .map(|entry| message_token_input(&entry.message, tools))
                        .collect::<Result<_, ApplicationError>>()?,
                })
            })
            .collect::<Result<_, ApplicationError>>()?;
        let counts = self
            .tokenization
            .count_openai_tokens_batch(OpenAiTokenCountBatchRequestDto {
                model: session.tokenizer_model.clone(),
                requests,
            })
            .await?;
        let kept_limit = KEPT_TOKENS.min(session.point / 4);
        let mut kept_tokens = 0;
        let mut kept_start = entries.len();
        for (group, tokens) in groups.iter().zip(counts.token_counts).rev() {
            if kept_tokens + tokens > kept_limit {
                break;
            }
            kept_tokens += tokens;
            kept_start = group.range.start;
        }
        Ok(kept_start)
    }

    async fn summarize_session_context(
        &self,
        prepared: &PreparedInvocation,
        session: &SessionInvocation,
        mut entries: Vec<&AgentSessionMessage>,
        round: usize,
        trigger: CompactionTrigger,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<String, ApplicationError> {
        let mut instruction = COMPACTION_REQUEST.to_owned();
        if entries
            .first()
            .is_some_and(|entry| entry.compaction.is_some())
        {
            instruction.push_str(EARLIER_SUMMARY);
        }
        let instruction = AgentModelMessage {
            role: AgentModelRole::User,
            parts: vec![AgentModelContentPart::Text { text: instruction }],
            provider_metadata: Value::Null,
        };
        let mut request = prepared.request.clone();
        request.messages.push(instruction.clone());
        if matches!(trigger, CompactionTrigger::Overflow) {
            reset_transport_for_resume(&mut request);
        }
        let mut tools_off = false;
        loop {
            self.ensure_not_cancelled(cancel)?;
            let exchange = self
                .generate_model_with_retry(
                    &prepared.invocation,
                    round,
                    &request,
                    &prepared.profile.run.model_retry,
                    false,
                    cancel,
                )
                .await;
            match exchange {
                Err(error) if is_context_overflow_error(&error) => {
                    let group = session_groups(&entries)?.into_iter().find(|group| {
                        entries[group.range.start].message.parts.iter()
                            .any(|part| matches!(part, AgentModelContentPart::ToolCall { .. }))
                    }).ok_or_else(|| ApplicationError::ValidationError(
                        "agent.compaction_failed: The context is too long to summarize, even after leaving out its earlier tool results.".into()
                    ))?;
                    entries.drain(group.range);
                    request = self
                        .rebuild_session_request(
                            prepared,
                            session,
                            session_history_groups(&entries)?,
                            cancel,
                        )
                        .await?;
                    request.messages.push(instruction.clone());
                    if tools_off {
                        request.tool_choice = ToolChoice::None;
                    }
                }
                Err(error) if is_output_truncated_error(&error) => {
                    return Err(ApplicationError::ValidationError(format!(
                        "agent.compaction_failed: The summary stopped at the response limit of the preset \"{}\". Raise its max response length.",
                        session.assembly_request.preset_ref.name,
                    )));
                }
                Err(error) => return Err(error),
                Ok(exchange) => {
                    if !exchange.response.tool_calls.is_empty() {
                        if tools_off {
                            return Err(ApplicationError::ValidationError(
                                "agent.compaction_failed: The model wrote no summary.".into(),
                            ));
                        }
                        tools_off = true;
                        request.tool_choice = ToolChoice::None;
                        reset_transport_for_resume(&mut request);
                        continue;
                    }
                    let text = exchange.response.text.trim();
                    if text.is_empty() {
                        return Err(ApplicationError::ValidationError(
                            "agent.compaction_failed: The model wrote no summary.".into(),
                        ));
                    }
                    return Ok(text.to_owned());
                }
            }
        }
    }

    pub(super) async fn compact_session_context(
        &self,
        prepared: &PreparedInvocation,
        session: &SessionInvocation,
        round: usize,
        trigger: CompactionTrigger,
        tokens_before: usize,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<AgentModelRequest, ApplicationError> {
        let invocation = &prepared.invocation;
        let run_id = invocation.run_id.as_str();
        let active = self.active_run_handle(run_id).await?;
        let session_id = active
            .target
            .session_id()
            .expect("Session invocation target");
        let history = self.read_session_history(session_id).await?;
        let current = session_context(&history)?;
        let entries = current
            .iter()
            .copied()
            .filter(|entry| entry.compaction.is_none())
            .collect::<Vec<_>>();
        let kept_start = self
            .retained_group_start(&entries, session, &prepared.request.tools)
            .await?;
        if kept_start == 0 {
            return Err(context_full(session, tokens_before));
        }
        self.transition_status(run_id, AgentRunStatus::CompactingContext)
            .await?;
        self.event(run_id, AgentRunEventLevel::Info, "context_compaction_started", json!({
            "round": round, "invocationId": invocation.id, "trigger": trigger, "tokens": tokens_before,
        })).await?;
        let summary = self
            .summarize_session_context(prepared, session, current, round, trigger, cancel)
            .await?;
        let summary_message = AgentModelMessage {
            role: AgentModelRole::User,
            parts: vec![
                AgentModelContentPart::Text {
                    text: COMPACTION_PREFIX.into(),
                },
                AgentModelContentPart::Text { text: summary },
            ],
            provider_metadata: Value::Null,
        };
        let mut groups = session_history_groups(&entries[kept_start..])?;
        groups.insert(0, vec![summary_message.clone()]);
        let request = self
            .rebuild_session_request(prepared, session, groups, cancel)
            .await?;
        let tokens_after = self
            .context_tokens(&request, &session.tokenizer_model, None)
            .await?;
        if tokens_after >= session.point {
            return Err(context_full(session, tokens_after));
        }
        self.ensure_not_cancelled(cancel)?;
        let kept_from_seq = entries.get(kept_start).map(|entry| entry.seq);
        let record = self
            .record_session_message(
                session_id,
                NewAgentSessionMessage {
                    run_id: run_id.to_owned(),
                    message: summary_message,
                    origin: Some(AgentSessionMessageOrigin {
                        invocation_id: invocation.id.clone(),
                        round,
                    }),
                    compaction: Some(AgentSessionCompaction {
                        kept_from_seq,
                        tokens_before,
                        tokens_after,
                    }),
                },
            )
            .await?;
        self.event(run_id, AgentRunEventLevel::Info, "context_compacted", json!({
            "round": round, "invocationId": invocation.id, "seq": record.seq,
            "tokensBefore": tokens_before, "tokensAfter": tokens_after, "keptFromSeq": kept_from_seq,
        })).await?;
        self.transition_status(run_id, AgentRunStatus::CallingModel)
            .await?;
        Ok(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;

    #[test]
    fn session_history_pairs_out_of_order_results_in_an_interrupted_group() {
        let identities = [
            ("saved", "opaque-call-7a", "builtin:opaque_tool_31"),
            ("pending", "opaque-call-8b", "builtin:opaque_tool_42"),
            ("missing", "opaque-call-9c", "builtin:opaque_tool_53"),
        ];
        let calls = identities.map(|(name, call_id, tool_id)| {
            json!({
                "type": "toolCall",
                "call": {
                    "callId": call_id,
                    "toolId": tool_id,
                    "arguments": { "path": format!("work/{name}.md") },
                    "providerMetadata": { "modelAlias": "workspace_read_file" }
                }
            })
        });
        let result = |index: usize, content: &str, is_error: bool| {
            let (_, call_id, tool_id) = identities[index];
            json!({
                "role": "tool",
                "parts": [{
                    "type": "toolResult",
                    "result": {
                        "callId": call_id,
                        "toolId": tool_id,
                        "content": content,
                        "isError": is_error,
                        "structured": {},
                        "resourceRefs": []
                    }
                }]
            })
        };
        let original_text = "Reading the requested files.";
        let mut parts = vec![json!({ "type": "text", "text": original_text })];
        parts.extend(calls);
        let history = [
            json!({ "role": "assistant", "parts": parts }),
            result(2, "File does not exist.", true),
            result(0, "The saved content.", false),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, message)| AgentSessionMessage {
            seq: index as u64 + 1,
            run_id: "interrupted".into(),
            created_at: Utc::now(),
            message: serde_json::from_value(message).unwrap(),
            origin: None,
            compaction: None,
        })
        .collect::<Vec<_>>();

        let groups = session_history_groups(&session_context(&history).unwrap()).unwrap();
        let [group] = groups.as_slice() else {
            panic!("expected one interrupted group")
        };
        let [message] = group.as_slice() else {
            panic!("expected a protocol-safe explanation")
        };
        assert_eq!(message.role, AgentModelRole::Assistant);
        let [AgentModelContentPart::Text { text }] = message.parts.as_slice() else {
            panic!("interrupted calls must become text instead of replayable tool calls")
        };
        assert!(text.contains(original_text));
        for (_, call_id, tool_id) in identities {
            assert!(!text.contains(call_id));
            assert!(!text.contains(tool_id));
        }
        let outcomes = text
            .split("workspace_read_file(")
            .skip(1)
            .collect::<Vec<_>>();
        assert_eq!(outcomes.len(), 3);
        assert!(outcomes[0].contains("work/saved.md"));
        assert!(outcomes[0].contains("Recorded result: The saved content."));
        assert!(!outcomes[0].contains("File does not exist."));
        assert!(outcomes[1].contains("work/pending.md"));
        assert!(outcomes[1].contains("Outcome unknown"));
        assert!(!outcomes[1].contains("The saved content."));
        assert!(outcomes[2].contains("work/missing.md"));
        assert!(outcomes[2].contains("Recorded error: File does not exist."));
    }
}
