use super::*;
use tt_domain::models::agent::{
    AgentDelegationContinuation, AgentInvocationExitPolicy, AgentInvocationKind,
    AgentInvocationStatus, AgentModelRole, AgentTaskStatus, ROOT_AGENT_INVOCATION_ID,
};

mod delegation;
mod execution;
mod extension_tools;
mod legacy_revision;
mod mcp;
mod model_binding;
mod persist;
mod resume;
mod session;
mod shell;
mod skill_workspace;
mod task_details;

fn allow_profile_tool(allow: &mut Vec<String>, name: &str) {
    let id = format!("builtin:{name}");
    if !allow.iter().any(|tool| tool == &id) {
        allow.push(id);
    }
}

fn model_tool_response(tool_calls: Vec<Value>) -> Value {
    json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": tool_calls,
            }
        }]
    })
}

fn model_tool_call(id: &str, name: &str, arguments: Value) -> Value {
    json!({
        "id": id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": serde_json::to_string(&arguments).expect("serialize tool arguments"),
        }
    })
}

fn message_text_for_role(request: &AgentModelRequest, role: AgentModelRole) -> &str {
    request
        .messages
        .iter()
        .find(|message| message.role == role)
        .and_then(|message| {
            message.parts.iter().find_map(|part| match part {
                AgentModelContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
        })
        .expect("message text for role")
}

fn delegated_prompt_snapshot(
    request: &tt_application::dto::agent_dto::AgentPromptAssemblyBrokerRequestDto,
) -> (Value, Value) {
    let preset_content = request.settings["prompts"][0]["content"].as_str().unwrap();
    (
        json!({
            "contextPolicy": request.agent_context_policy,
            "messages": [
                { "role": "system", "parts": [{ "type": "text", "text": format!("{}\n{preset_content}", request.agent_system_prompt) }],
                  "providerMetadata": { "promptComponent": "agentSystemPrompt" } },
                { "role": "user", "parts": [{ "type": "text", "text": request.agent_task_prompt.as_ref().unwrap() }],
                  "providerMetadata": { "promptComponent": "agentTask" } }
            ],
            "generationParameters": {
                "chat_completion_source": request.settings["chat_completion_source"],
                "model": request.settings["model"],
            }
        }),
        request.frozen_run_input_snapshot.clone(),
    )
}

async fn resolve_contract_prompt_assembly(
    fixture: &AgentRuntimeFixture,
    run_id: &str,
    build_snapshot: fn(
        &tt_application::dto::agent_dto::AgentPromptAssemblyBrokerRequestDto,
    ) -> (Value, Value),
) -> tt_application::dto::agent_dto::AgentPromptAssemblyBrokerRequestDto {
    use tt_application::dto::agent_dto::{
        AgentReadPromptAssemblyRequestDto, AgentResolvePromptAssemblyDto,
    };

    let event = wait_for_event(fixture, run_id, "prompt_assembly_requested", 0).await;
    let assembly_id = event.payload["assemblyId"].as_str().unwrap().to_string();
    let request = fixture
        .service
        .read_prompt_assembly_request(AgentReadPromptAssemblyRequestDto {
            run_id: run_id.into(),
            assembly_id: assembly_id.clone(),
        })
        .await
        .unwrap()
        .unwrap();
    let (snapshot, frozen) = build_snapshot(&request);
    fixture
        .service
        .resolve_prompt_assembly(AgentResolvePromptAssemblyDto {
            run_id: run_id.into(),
            assembly_id,
            prompt_snapshot: Some(snapshot),
            frozen_run_input_snapshot: Some(frozen),
            generation_intent: None,
            assembly: None,
            error: None,
        })
        .await
        .unwrap();
    request
}

async fn wait_for_event(
    fixture: &AgentRuntimeFixture,
    run_id: &str,
    event_type: &str,
    after_seq: u64,
) -> tt_domain::models::agent::AgentRunEvent {
    tokio::time::timeout(AGENT_CONTRACT_ASYNC_TIMEOUT, async {
        loop {
            if let Some(event) = read_agent_events(&fixture.agent_repository, run_id)
                .await
                .into_iter()
                .find(|event| event.seq > after_seq && event.event_type == event_type)
            {
                return event;
            }
        }
    })
    .await
    .expect("expected runtime event")
}
