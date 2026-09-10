use serde_json::{Map, Value};

use crate::errors::ApplicationError;

use super::super::model_capabilities::map_xai_reasoning_effort;
use super::openai;
use super::prompt_post_processing::{PromptNames, prefix_if_missing};
use super::shared::message_content_to_text;

pub(super) fn build(mut payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let names = PromptNames::from_payload(&payload);
    let reasoning_effort = map_xai_reasoning_effort(
        payload
            .get("reasoning_effort")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )?;

    if let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) {
        fold_names_into_content(messages, &names);
    }

    let (endpoint, mut upstream_payload) = openai::build(payload)?;

    if let (Some(reasoning_effort), Some(body)) =
        (reasoning_effort, upstream_payload.as_object_mut())
    {
        body.insert(
            "reasoning_effort".to_string(),
            Value::String(reasoning_effort.to_string()),
        );
    }

    Ok((endpoint, upstream_payload))
}

/// xAI rejects `name` values that are not plain identifiers, so character
/// names move into the message content the way the upstream converter does.
fn fold_names_into_content(messages: &mut [Value], names: &PromptNames) {
    for message in messages {
        let Some(object) = message.as_object_mut() else {
            continue;
        };
        let Some((role, name)) = object
            .get("role")
            .and_then(Value::as_str)
            .zip(object.get("name").and_then(Value::as_str))
            .map(|(role, name)| (role.to_string(), name.to_string()))
        else {
            continue;
        };
        if role == "user" {
            continue;
        }

        let content = message_content_to_text(object.get("content"));
        let prefixed = match (role.as_str(), name.as_str()) {
            ("assistant", _) | ("system", "example_assistant") => {
                Some(prefix_if_missing(&content, &names.char_name, |content| {
                    names.starts_with_group_name(content)
                }))
            }
            ("system", "example_user") => {
                Some(prefix_if_missing(&content, &names.user_name, |_| false))
            }
            _ => None,
        };

        if let Some(prefixed) = prefixed {
            object.insert("content".to_string(), Value::String(prefixed));
        }
        object.remove("name");
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    fn build_messages(messages: Value) -> Vec<Value> {
        let payload = json!({
            "model": "grok-4",
            "messages": messages,
            "char_name": "Aqua",
            "user_name": "Kazuma",
            "chat_completion_source": "xai"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("payload should build");
        upstream
            .get("messages")
            .and_then(Value::as_array)
            .cloned()
            .expect("messages must be an array")
    }

    #[test]
    fn xai_folds_names_into_content() {
        let messages = build_messages(json!([
            {"role": "system", "name": "example_user", "content": "hi"},
            {"role": "system", "name": "example_assistant", "content": "hello"},
            {"role": "assistant", "name": "Aqua", "content": "again"},
            {"role": "user", "name": "Kazuma", "content": "kept"}
        ]));

        assert_eq!(messages[0]["content"], "Kazuma: hi");
        assert_eq!(messages[1]["content"], "Aqua: hello");
        assert_eq!(messages[2]["content"], "Aqua: again");
        assert_eq!(messages[3]["content"], "kept");
        assert_eq!(messages[3]["name"], "Kazuma");
        for message in &messages[..3] {
            assert!(message.get("name").is_none());
        }
    }

    #[test]
    fn xai_keeps_multimodal_content_without_a_prefix_rule() {
        let messages = build_messages(json!([{
            "role": "system",
            "name": "nucleus",
            "content": [{"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}]
        }]));

        assert!(messages[0]["content"].is_array());
        assert!(messages[0].get("name").is_none());
    }

    #[test]
    fn xai_maps_reasoning_effort_to_the_supported_levels() {
        for (requested, expected) in [
            ("min", Some("minimal")),
            ("low", Some("low")),
            ("medium", Some("medium")),
            ("xhigh", Some("high")),
            ("max", Some("high")),
            ("auto", None),
        ] {
            let payload = json!({
                "model": "grok-4",
                "messages": [{"role": "user", "content": "hello"}],
                "reasoning_effort": requested,
                "chat_completion_source": "xai"
            })
            .as_object()
            .cloned()
            .expect("payload must be object");

            let (_, upstream) = build(payload).expect("payload should build");
            assert_eq!(
                upstream.get("reasoning_effort").and_then(Value::as_str),
                expected,
                "effort {requested}"
            );
        }
    }
}
