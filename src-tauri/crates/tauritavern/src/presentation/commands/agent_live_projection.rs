use std::collections::BTreeMap;
use std::sync::Arc;

use tauri::{Manager, Resource, ResourceId, State, Webview, ipc::Channel};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tt_application::dto::agent_dto::{
    AgentOpenRunLiveProjectionDto, AgentRunLiveFieldDto, AgentRunLiveResponseDto,
    AgentRunLiveToolCallDto, AgentRunLiveUpdateDto,
};
use tt_application::services::agent_runtime_service::{
    AgentRunLiveCall, AgentRunLiveCallKey, AgentRunLiveProjection, AgentRunLiveResponse,
    ModelAttemptGeneration, ToolCallProjection,
};
use tt_domain::text_metrics::TextMetrics;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

/// A subscriber's handle on a run's live projection. Closing it ends the stream that delivers the projection.
pub(crate) struct AgentRunLiveProjectionResource {
    run_id: String,
    closed: CancellationToken,
}

impl Resource for AgentRunLiveProjectionResource {
    fn close(self: Arc<Self>) {
        self.closed.cancel();
    }
}

/// Registers without awaiting, so no page reload sweep can pass between the page's open and its registration.
#[tauri::command]
pub fn open_agent_run_live_projection(
    dto: AgentOpenRunLiveProjectionDto,
    webview: Webview,
) -> ResourceId {
    log_command("open_agent_run_live_projection");

    webview
        .resources_table()
        .add(AgentRunLiveProjectionResource {
            run_id: dto.run_id,
            closed: CancellationToken::new(),
        })
}

/// Sends a snapshot and then the updates, until the run ends or the handle is closed.
#[tauri::command]
pub async fn stream_agent_run_live_projection(
    rid: ResourceId,
    channel: Channel<AgentRunLiveUpdateDto>,
    webview: Webview,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("stream_agent_run_live_projection");

    let resource = webview
        .resources_table()
        .get::<AgentRunLiveProjectionResource>(rid)
        .map_err(map_command_error(
            "Failed to access agent run live projection",
        ))?;
    let projection = app_state
        .services
        .agent_runtime_service
        .subscribe_live_projection(&resource.run_id)
        .await
        .map_err(map_command_error(
            "Failed to subscribe to agent run live projection",
        ))?;
    stream(projection, &resource.closed, &channel).await
}

/// `projection` is `None` when the run has no live projection, such as after it ended.
async fn stream(
    projection: Option<watch::Receiver<AgentRunLiveProjection>>,
    closed: &CancellationToken,
    channel: &Channel<AgentRunLiveUpdateDto>,
) -> Result<(), CommandError> {
    let send = |update| {
        channel.send(update).map_err(map_command_error(
            "Failed to send agent run live projection update",
        ))
    };
    let Some(mut receiver) = projection else {
        return send(AgentRunLiveUpdateDto::Snapshot {
            calls: Vec::new(),
            responses: Vec::new(),
        });
    };

    let mut presenter = AgentRunLivePresenter::default();
    let snapshot = {
        let projection = receiver.borrow_and_update();
        presenter.snapshot(&projection)
    };
    send(snapshot)?;

    loop {
        tokio::select! {
            biased;
            () = closed.cancelled() => return Ok(()),
            changed = receiver.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
            }
        }
        let updates = {
            let projection = receiver.borrow_and_update();
            presenter.updates(&projection)?
        };
        for update in updates {
            send(update)?;
        }
    }
}

#[derive(Default)]
struct AgentRunLivePresenter {
    calls: BTreeMap<AgentRunLiveCallKey, PresentedCall>,
    responses: BTreeMap<String, PresentedResponse>,
}

struct PresentedResponse {
    generation: ModelAttemptGeneration,
    text_bytes: usize,
    reasoning_bytes: usize,
    tool_count: usize,
}

impl From<&AgentRunLiveResponse> for PresentedResponse {
    fn from(response: &AgentRunLiveResponse) -> Self {
        Self {
            generation: response.generation,
            text_bytes: response.text.len(),
            reasoning_bytes: response.reasoning.len(),
            tool_count: response.tool_ids.len(),
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
    fn snapshot(&mut self, projection: &AgentRunLiveProjection) -> AgentRunLiveUpdateDto {
        self.calls = projection
            .calls
            .iter()
            .map(|(key, call)| (key.clone(), PresentedCall::from(call)))
            .collect();
        self.responses = projection
            .responses
            .iter()
            .map(|(id, response)| (id.clone(), PresentedResponse::from(response)))
            .collect();
        AgentRunLiveUpdateDto::Snapshot {
            calls: projection
                .calls
                .iter()
                .map(|(key, call)| to_call_dto(key, call))
                .collect(),
            responses: projection
                .responses
                .iter()
                .map(|(id, response)| to_response_dto(id, response))
                .collect(),
        }
    }

    fn updates(
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
                    )?;
                }
                _ => updates.push(AgentRunLiveUpdateDto::Replace {
                    call: to_call_dto(key, call),
                }),
            }
            presented.insert(key.clone(), PresentedCall::from(call));
        }
        self.calls = presented;

        for id in self
            .responses
            .keys()
            .filter(|id| !projection.responses.contains_key(*id))
        {
            updates.push(AgentRunLiveUpdateDto::ResponseRemove {
                invocation_id: id.clone(),
            });
        }
        for (id, response) in &projection.responses {
            match self.responses.get(id) {
                Some(previous) if previous.generation == response.generation => {
                    let suffixes = response
                        .text
                        .get(previous.text_bytes..)
                        .zip(response.reasoning.get(previous.reasoning_bytes..))
                        .zip(response.tool_ids.get(previous.tool_count..));
                    let Some(((text, reasoning), tool_ids)) = suffixes else {
                        return Err(CommandError::InternalServerError(format!(
                            "agent.live_projection_cursor_invalid: response shrank for invocation `{id}`"
                        )));
                    };
                    if !text.is_empty() || !reasoning.is_empty() || !tool_ids.is_empty() {
                        updates.push(AgentRunLiveUpdateDto::ResponseAppend {
                            invocation_id: id.clone(),
                            text: text.to_string(),
                            reasoning: reasoning.to_string(),
                            tool_ids: tool_ids.to_vec(),
                        });
                    }
                }
                _ => updates.push(AgentRunLiveUpdateDto::ResponseReplace {
                    response: to_response_dto(id, response),
                }),
            }
        }
        self.responses = projection
            .responses
            .iter()
            .map(|(id, response)| (id.clone(), PresentedResponse::from(response)))
            .collect();
        Ok(updates)
    }
}

fn to_response_dto(
    invocation_id: &str,
    response: &AgentRunLiveResponse,
) -> AgentRunLiveResponseDto {
    AgentRunLiveResponseDto {
        invocation_id: invocation_id.to_string(),
        invocation_exit_policy: response.invocation_exit_policy,
        round: response.generation.round,
        attempt: response.generation.attempt,
        text: response.text.clone(),
        reasoning: response.reasoning.clone(),
        tool_ids: response.tool_ids.clone(),
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
    use std::time::Duration;

    use serde_json::json;
    use tt_domain::models::agent::AgentInvocationExitPolicy;
    use tt_domain::models::tool::ToolId;

    use super::*;

    #[tokio::test]
    async fn closing_ends_a_stream_waiting_for_changes() {
        // The run stays live, so only closing can end the stream.
        let (_live, receiver) = watch::channel(AgentRunLiveProjection::default());
        let closed = CancellationToken::new();
        let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
        let channel = Channel::new(move |body| {
            sent.send(body).expect("the test receives every update");
            Ok(())
        });
        let streaming = tokio::spawn({
            let closed = closed.clone();
            async move { stream(Some(receiver), &closed, &channel).await }
        });
        received.recv().await.expect("the snapshot");

        closed.cancel();

        let result = tokio::time::timeout(Duration::from_secs(5), streaming)
            .await
            .expect("closing ends the stream");
        assert!(matches!(result, Ok(Ok(()))));
    }

    #[test]
    fn response_cursors_survive_unicode_and_coalesced_retry() {
        let mut projection = AgentRunLiveProjection::default();
        projection.responses.insert(
            "inv_root".to_string(),
            AgentRunLiveResponse {
                generation: ModelAttemptGeneration {
                    round: 1,
                    attempt: 1,
                },
                invocation_exit_policy: AgentInvocationExitPolicy::RunFinishAllowed,
                text: "正文".to_string(),
                reasoning: "思考".to_string(),
                tool_ids: Vec::new(),
            },
        );
        let mut presenter = AgentRunLivePresenter::default();
        let snapshot = serde_json::to_value(presenter.snapshot(&projection)).unwrap();
        assert_eq!(
            snapshot["responses"],
            json!([{
                "invocationId": "inv_root", "invocationExitPolicy": "run_finish_allowed", "round": 1, "attempt": 1, "text": "正文", "reasoning": "思考", "toolIds": []
            }])
        );
        projection
            .responses
            .get_mut("inv_root")
            .unwrap()
            .text
            .push_str("一下😀");
        projection
            .responses
            .get_mut("inv_root")
            .unwrap()
            .reasoning
            .push_str("再想😀");
        assert_eq!(
            presenter.updates(&projection).unwrap(),
            vec![AgentRunLiveUpdateDto::ResponseAppend {
                invocation_id: "inv_root".to_string(),
                text: "一下😀".to_string(),
                reasoning: "再想😀".to_string(),
                tool_ids: Vec::new(),
            }]
        );
        projection
            .responses
            .get_mut("inv_root")
            .unwrap()
            .tool_ids
            .push(ToolId::builtin("workspace.read_file").unwrap());
        assert_eq!(
            presenter.updates(&projection).unwrap(),
            vec![AgentRunLiveUpdateDto::ResponseAppend {
                invocation_id: "inv_root".to_string(),
                text: String::new(),
                reasoning: String::new(),
                tool_ids: vec![ToolId::builtin("workspace.read_file").unwrap()],
            }]
        );
        // watch may coalesce clear + retry: a new generation must replace, not append.
        let response = projection.responses.get_mut("inv_root").unwrap();
        response.generation.attempt = 2;
        response.text = "retry".to_string();
        response.reasoning.clear();
        assert_eq!(
            presenter.updates(&projection).unwrap(),
            vec![AgentRunLiveUpdateDto::ResponseReplace {
                response: to_response_dto("inv_root", &projection.responses["inv_root"]),
            }]
        );
        projection.responses.clear();
        assert_eq!(
            presenter.updates(&projection).unwrap(),
            vec![AgentRunLiveUpdateDto::ResponseRemove {
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
                "responses": []
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
