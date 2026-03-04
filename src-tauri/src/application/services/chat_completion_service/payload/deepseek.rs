use serde_json::{Map, Value};

use super::openai;
use super::prompt_post_processing::{
    PromptNames, PromptProcessingType, add_reasoning_content_to_tool_calls, post_process_prompt,
};

pub(super) fn build(mut payload: Map<String, Value>) -> (String, Value) {
    let names = PromptNames::from_payload(&payload);
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let tools_snapshot = payload.get("tools").cloned();

    if let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) {
        let raw = std::mem::take(messages);
        let mut processed = post_process_prompt(raw, PromptProcessingType::SemiTools, &names);

        add_assistant_prefix(&mut processed, tools_snapshot.as_ref(), "prefix");

        if model.to_ascii_lowercase().contains("-reasoner") {
            add_reasoning_content_to_tool_calls(&mut processed);
        }

        payload.insert("messages".to_string(), Value::Array(processed));
    }

    strip_empty_required_arrays_from_tools(&mut payload);

    openai::build(payload)
}

fn add_assistant_prefix(messages: &mut [Value], tools: Option<&Value>, property: &str) {
    if messages.is_empty() {
        return;
    }

    let has_tools = tools
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty());
    let has_tool_messages = messages.iter().any(|message| {
        message
            .as_object()
            .and_then(|object| object.get("role"))
            .and_then(Value::as_str)
            == Some("tool")
    });

    if has_tools || has_tool_messages {
        return;
    }

    let Some(last_message) = messages.last_mut().and_then(Value::as_object_mut) else {
        return;
    };

    if last_message.get("role").and_then(Value::as_str) != Some("assistant") {
        return;
    }

    last_message.insert(property.to_string(), Value::Bool(true));
}

fn strip_empty_required_arrays_from_tools(payload: &mut Map<String, Value>) {
    let Some(tools) = payload.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };

    for tool in tools {
        let should_remove = tool
            .as_object()
            .and_then(|tool| tool.get("function"))
            .and_then(Value::as_object)
            .and_then(|function| function.get("parameters"))
            .and_then(Value::as_object)
            .and_then(|parameters| parameters.get("required"))
            .and_then(Value::as_array)
            .is_some_and(|required| required.is_empty());

        if !should_remove {
            continue;
        }

        if let Some(parameters) = tool
            .as_object_mut()
            .and_then(|tool| tool.get_mut("function"))
            .and_then(Value::as_object_mut)
            .and_then(|function| function.get_mut("parameters"))
            .and_then(Value::as_object_mut)
        {
            parameters.remove("required");
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    #[test]
    fn deepseek_build_marks_assistant_prefill_as_prefix() {
        let payload = json!({
            "model": "deepseek-reasoner",
            "messages": [
                {"role":"user","content":"hi"},
                {"role":"assistant","content":"prefill"}
            ],
            "chat_completion_source": "deepseek"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload);
        let body = upstream.as_object().expect("body must be object");

        let last = body
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.last())
            .and_then(Value::as_object)
            .expect("last message must be object");

        assert_eq!(last.get("role").and_then(Value::as_str), Some("assistant"));
        assert_eq!(last.get("prefix").and_then(Value::as_bool), Some(true));
    }
}
