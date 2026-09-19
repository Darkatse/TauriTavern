use std::collections::BTreeMap;

use tt_application::dto::agent_dto::{
    AgentRunLiveFieldDto, AgentRunLiveReasoningDto, AgentRunLiveToolCallDto, AgentRunLiveUpdateDto,
};
use tt_application::services::agent_runtime_service::{
    AgentRunLiveCall, AgentRunLiveCallKey, AgentRunLiveProjection, AgentRunLiveReasoning,
    ModelAttemptGeneration, ToolCallProjection,
};
use tt_domain::text_metrics::TextMetrics;

use crate::presentation::errors::CommandError;

#[derive(Default)]
pub(super) struct AgentRunLivePresenter {
    calls: BTreeMap<AgentRunLiveCallKey, PresentedCall>,
    reasoning: BTreeMap<String, PresentedReasoning>,
}

struct PresentedReasoning {
    generation: ModelAttemptGeneration,
    text_bytes: usize,
    tool_count: usize,
}

impl From<&AgentRunLiveReasoning> for PresentedReasoning {
    fn from(reasoning: &AgentRunLiveReasoning) -> Self {
        Self {
            generation: reasoning.generation,
            text_bytes: reasoning.text.len(),
            tool_count: reasoning.tool_ids.len(),
        }
    }
}

struct PresentedCall {
    generation: ModelAttemptGeneration,
    fields: PresentedFields,
}

enum PresentedFields {
    WriteFile {
        path_bytes: usize,
        content_bytes: usize,
    },
    ApplyPatch {
        path_bytes: usize,
        old_string_bytes: usize,
        new_string_bytes: usize,
    },
}

impl AgentRunLivePresenter {
    pub(super) fn snapshot(
        &mut self,
        projection: &AgentRunLiveProjection,
    ) -> AgentRunLiveUpdateDto {
        let mut calls = Vec::with_capacity(projection.calls.len());
        let mut presented = BTreeMap::new();
        for (key, call) in &projection.calls {
            calls.push(to_call_dto(key, call));
            presented.insert(key.clone(), PresentedCall::from(call));
        }
        self.calls = presented;
        let reasoning = projection
            .reasoning
            .iter()
            .map(|(id, reasoning)| to_reasoning_dto(id, reasoning))
            .collect();
        self.reasoning = projection
            .reasoning
            .iter()
            .map(|(id, reasoning)| (id.clone(), PresentedReasoning::from(reasoning)))
            .collect();
        AgentRunLiveUpdateDto::Snapshot { calls, reasoning }
    }

    pub(super) fn updates(
        &mut self,
        projection: &AgentRunLiveProjection,
    ) -> Result<Vec<AgentRunLiveUpdateDto>, CommandError> {
        let mut updates = self
            .calls
            .keys()
            .filter(|key| !projection.calls.contains_key(*key))
            .map(|key| AgentRunLiveUpdateDto::Remove {
                invocation_id: key.invocation_id.clone(),
                tool_call_index: key.tool_call_index,
            })
            .collect::<Vec<_>>();
        let mut presented = BTreeMap::new();

        for (key, call) in &projection.calls {
            match self.calls.get(key) {
                Some(previous) if previous.generation == call.generation => {
                    append_projection_updates(
                        &mut updates,
                        key,
                        &previous.fields,
                        &call.projection,
                    )?
                }
                _ => updates.push(AgentRunLiveUpdateDto::Replace {
                    call: to_call_dto(key, call),
                }),
            }
            presented.insert(key.clone(), PresentedCall::from(call));
        }

        self.calls = presented;
        for id in self
            .reasoning
            .keys()
            .filter(|id| !projection.reasoning.contains_key(*id))
        {
            updates.push(AgentRunLiveUpdateDto::ReasoningRemove {
                invocation_id: id.clone(),
            });
        }
        for (id, reasoning) in &projection.reasoning {
            match self.reasoning.get(id) {
                Some(previous) if previous.generation == reasoning.generation => {
                    let Some(text) = reasoning.text.get(previous.text_bytes..) else {
                        return Err(CommandError::InternalServerError(format!(
                            "agent.live_projection_cursor_invalid: reasoning shrank for invocation `{id}`"
                        )));
                    };
                    let Some(tool_ids) = reasoning.tool_ids.get(previous.tool_count..) else {
                        return Err(CommandError::InternalServerError(format!(
                            "agent.live_projection_cursor_invalid: reasoning tool IDs shrank for invocation `{id}`"
                        )));
                    };
                    if !text.is_empty() || !tool_ids.is_empty() {
                        updates.push(AgentRunLiveUpdateDto::ReasoningAppend {
                            invocation_id: id.clone(),
                            text: text.to_string(),
                            tool_ids: tool_ids.to_vec(),
                        });
                    }
                }
                _ => updates.push(AgentRunLiveUpdateDto::ReasoningReplace {
                    reasoning: to_reasoning_dto(id, reasoning),
                }),
            }
        }
        self.reasoning = projection
            .reasoning
            .iter()
            .map(|(id, reasoning)| (id.clone(), PresentedReasoning::from(reasoning)))
            .collect();
        Ok(updates)
    }
}

fn to_reasoning_dto(
    invocation_id: &str,
    reasoning: &AgentRunLiveReasoning,
) -> AgentRunLiveReasoningDto {
    AgentRunLiveReasoningDto {
        invocation_id: invocation_id.to_string(),
        invocation_exit_policy: reasoning.invocation_exit_policy,
        text: reasoning.text.clone(),
        tool_ids: reasoning.tool_ids.clone(),
    }
}

impl From<&AgentRunLiveCall> for PresentedCall {
    fn from(call: &AgentRunLiveCall) -> Self {
        Self {
            generation: call.generation,
            fields: match &call.projection {
                ToolCallProjection::WriteFile { path, content } => PresentedFields::WriteFile {
                    path_bytes: path.len(),
                    content_bytes: content.len(),
                },
                ToolCallProjection::ApplyPatch {
                    path,
                    old_string,
                    new_string,
                } => PresentedFields::ApplyPatch {
                    path_bytes: path.len(),
                    old_string_bytes: old_string.len(),
                    new_string_bytes: new_string.len(),
                },
            },
        }
    }
}

fn to_call_dto(key: &AgentRunLiveCallKey, call: &AgentRunLiveCall) -> AgentRunLiveToolCallDto {
    match &call.projection {
        ToolCallProjection::WriteFile { path, content } => AgentRunLiveToolCallDto::WriteFile {
            invocation_id: key.invocation_id.clone(),
            invocation_exit_policy: call.invocation_exit_policy,
            tool_call_index: key.tool_call_index,
            path: path.clone(),
            content: content.clone(),
            content_words: TextMetrics::from_text(content).words,
        },
        ToolCallProjection::ApplyPatch {
            path,
            old_string,
            new_string,
        } => AgentRunLiveToolCallDto::ApplyPatch {
            invocation_id: key.invocation_id.clone(),
            invocation_exit_policy: call.invocation_exit_policy,
            tool_call_index: key.tool_call_index,
            path: path.clone(),
            old_string: old_string.clone(),
            old_string_words: TextMetrics::from_text(old_string).words,
            new_string: new_string.clone(),
            new_string_words: TextMetrics::from_text(new_string).words,
        },
    }
}

fn append_projection_updates(
    updates: &mut Vec<AgentRunLiveUpdateDto>,
    key: &AgentRunLiveCallKey,
    previous: &PresentedFields,
    current: &ToolCallProjection,
) -> Result<(), CommandError> {
    match (previous, current) {
        (
            PresentedFields::WriteFile {
                path_bytes,
                content_bytes,
            },
            ToolCallProjection::WriteFile { path, content },
        ) => {
            append_field(updates, key, AgentRunLiveFieldDto::Path, *path_bytes, path)?;
            append_field(
                updates,
                key,
                AgentRunLiveFieldDto::Content,
                *content_bytes,
                content,
            )
        }
        (
            PresentedFields::ApplyPatch {
                path_bytes,
                old_string_bytes,
                new_string_bytes,
            },
            ToolCallProjection::ApplyPatch {
                path,
                old_string,
                new_string,
            },
        ) => {
            append_field(updates, key, AgentRunLiveFieldDto::Path, *path_bytes, path)?;
            append_field(
                updates,
                key,
                AgentRunLiveFieldDto::OldString,
                *old_string_bytes,
                old_string,
            )?;
            append_field(
                updates,
                key,
                AgentRunLiveFieldDto::NewString,
                *new_string_bytes,
                new_string,
            )
        }
        _ => Err(CommandError::InternalServerError(format!(
            "agent.live_projection_kind_changed: projection kind changed within one generation for invocation `{}` tool index {}",
            key.invocation_id, key.tool_call_index
        ))),
    }
}

fn append_field(
    updates: &mut Vec<AgentRunLiveUpdateDto>,
    key: &AgentRunLiveCallKey,
    field: AgentRunLiveFieldDto,
    previous_bytes: usize,
    current: &str,
) -> Result<(), CommandError> {
    let Some(text) = current.get(previous_bytes..) else {
        return Err(CommandError::InternalServerError(format!(
            "agent.live_projection_cursor_invalid: {field:?} shrank for invocation `{}` tool index {}",
            key.invocation_id, key.tool_call_index
        )));
    };
    if !text.is_empty() {
        updates.push(AgentRunLiveUpdateDto::Append {
            invocation_id: key.invocation_id.clone(),
            tool_call_index: key.tool_call_index,
            field,
            text: text.to_string(),
            word_delta: TextMetrics::from_text(text).words,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tt_domain::models::agent::AgentInvocationExitPolicy;
    use tt_domain::models::tool::ToolId;

    use super::*;

    #[test]
    fn reasoning_cursors_survive_unicode_and_coalesced_retry() {
        let mut projection = AgentRunLiveProjection::default();
        projection.reasoning.insert(
            "inv_root".to_string(),
            AgentRunLiveReasoning {
                generation: ModelAttemptGeneration {
                    round: 1,
                    attempt: 1,
                },
                invocation_exit_policy: AgentInvocationExitPolicy::RunFinishAllowed,
                text: "思考".to_string(),
                tool_ids: Vec::new(),
            },
        );
        let mut presenter = AgentRunLivePresenter::default();
        let snapshot = serde_json::to_value(presenter.snapshot(&projection)).unwrap();
        assert_eq!(
            snapshot["reasoning"],
            json!([{
                "invocationId": "inv_root", "invocationExitPolicy": "run_finish_allowed", "text": "思考", "toolIds": []
            }])
        );
        projection
            .reasoning
            .get_mut("inv_root")
            .unwrap()
            .text
            .push_str("一下😀");
        assert_eq!(
            presenter.updates(&projection).unwrap(),
            vec![AgentRunLiveUpdateDto::ReasoningAppend {
                invocation_id: "inv_root".to_string(),
                text: "一下😀".to_string(),
                tool_ids: Vec::new(),
            }]
        );
        projection
            .reasoning
            .get_mut("inv_root")
            .unwrap()
            .tool_ids
            .push(ToolId::builtin("workspace.read_file").unwrap());
        assert_eq!(
            presenter.updates(&projection).unwrap(),
            vec![AgentRunLiveUpdateDto::ReasoningAppend {
                invocation_id: "inv_root".to_string(),
                text: String::new(),
                tool_ids: vec![ToolId::builtin("workspace.read_file").unwrap()],
            }]
        );
        // watch may coalesce clear + retry: a new generation must replace, not append.
        let reasoning = projection.reasoning.get_mut("inv_root").unwrap();
        reasoning.generation.attempt = 2;
        reasoning.text = "retry".to_string();
        assert_eq!(
            presenter.updates(&projection).unwrap(),
            vec![AgentRunLiveUpdateDto::ReasoningReplace {
                reasoning: to_reasoning_dto("inv_root", &projection.reasoning["inv_root"]),
            }]
        );
        projection.reasoning.clear();
        assert_eq!(
            presenter.updates(&projection).unwrap(),
            vec![AgentRunLiveUpdateDto::ReasoningRemove {
                invocation_id: "inv_root".to_string(),
            }]
        );
    }

    #[test]
    fn presenter_emits_snapshot_append_replace_and_remove() {
        let key = AgentRunLiveCallKey {
            invocation_id: "inv_root".to_string(),
            tool_call_index: 0,
        };
        let mut presenter = AgentRunLivePresenter::default();
        let snapshot = presenter.snapshot(&write_projection(&key, 1, "out", "你"));
        assert_eq!(
            serde_json::to_value(snapshot).unwrap(),
            json!({
                "type": "snapshot",
                "calls": [{
                    "toolId": "builtin:workspace.write_file",
                    "invocationId": "inv_root",
                    "invocationExitPolicy": "run_finish_allowed",
                    "toolCallIndex": 0,
                    "path": "out",
                    "content": "你",
                    "contentWords": 1
                }],
                "reasoning": []
            })
        );

        assert_eq!(
            presenter
                .updates(&write_projection(&key, 1, "output/a.md", "你好"))
                .unwrap(),
            vec![
                AgentRunLiveUpdateDto::Append {
                    invocation_id: "inv_root".to_string(),
                    tool_call_index: 0,
                    field: AgentRunLiveFieldDto::Path,
                    text: "put/a.md".to_string(),
                    word_delta: 3,
                },
                AgentRunLiveUpdateDto::Append {
                    invocation_id: "inv_root".to_string(),
                    tool_call_index: 0,
                    field: AgentRunLiveFieldDto::Content,
                    text: "好".to_string(),
                    word_delta: 1,
                },
            ]
        );

        let retried = presenter
            .updates(&write_projection(&key, 2, "output/b.md", "retry"))
            .unwrap();
        assert!(matches!(
            retried.as_slice(),
            [AgentRunLiveUpdateDto::Replace { .. }]
        ));

        assert_eq!(
            presenter
                .updates(&AgentRunLiveProjection::default())
                .unwrap(),
            vec![AgentRunLiveUpdateDto::Remove {
                invocation_id: "inv_root".to_string(),
                tool_call_index: 0,
            }]
        );
    }

    fn write_projection(
        key: &AgentRunLiveCallKey,
        attempt: usize,
        path: &str,
        content: &str,
    ) -> AgentRunLiveProjection {
        AgentRunLiveProjection {
            calls: BTreeMap::from([(
                key.clone(),
                AgentRunLiveCall {
                    generation: ModelAttemptGeneration { round: 1, attempt },
                    invocation_exit_policy: AgentInvocationExitPolicy::RunFinishAllowed,
                    projection: ToolCallProjection::WriteFile {
                        path: path.to_string(),
                        content: content.to_string(),
                    },
                },
            )]),
            ..Default::default()
        }
    }
}
