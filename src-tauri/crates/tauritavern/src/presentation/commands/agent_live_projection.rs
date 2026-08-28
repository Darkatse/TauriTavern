use std::collections::BTreeMap;

use tt_application::dto::agent_dto::{
    AgentRunLiveFieldDto, AgentRunLiveToolCallDto, AgentRunLiveUpdateDto,
};
use tt_application::services::agent_runtime_service::{
    AgentRunLiveCall, AgentRunLiveCallKey, AgentRunLiveProjection, ModelAttemptGeneration,
    ToolCallProjection,
};
use tt_domain::text_metrics::TextMetrics;

use crate::presentation::errors::CommandError;

#[derive(Default)]
pub(super) struct AgentRunLivePresenter {
    calls: BTreeMap<AgentRunLiveCallKey, PresentedCall>,
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
        AgentRunLiveUpdateDto::Snapshot { calls }
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
                None => updates.push(AgentRunLiveUpdateDto::Replace {
                    call: to_call_dto(key, call),
                }),
                Some(previous) if previous.generation != call.generation => {
                    updates.push(AgentRunLiveUpdateDto::Replace {
                        call: to_call_dto(key, call),
                    });
                }
                Some(previous) => append_projection_updates(
                    &mut updates,
                    key,
                    &previous.fields,
                    &call.projection,
                )?,
            }
            presented.insert(key.clone(), PresentedCall::from(call));
        }

        self.calls = presented;
        Ok(updates)
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

    use super::*;

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
                }]
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
        }
    }
}
