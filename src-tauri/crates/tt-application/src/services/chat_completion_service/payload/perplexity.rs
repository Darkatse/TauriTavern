use serde_json::{Map, Value, json};

use crate::errors::ApplicationError;

use super::openai;
use super::prompt_post_processing::{PromptNames, PromptProcessingType, post_process_prompt};

/// Perplexity takes alternating user and assistant turns after the system prompt, and a bare JSON schema.
pub(super) fn build(mut payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let names = PromptNames::from_payload(&payload);
    if let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) {
        *messages = post_process_prompt(
            std::mem::take(messages),
            PromptProcessingType::Strict,
            &names,
        );
    }

    let schema = payload
        .remove("json_schema")
        .and_then(|mut json_schema| json_schema.get_mut("value").map(Value::take))
        .filter(|schema| !schema.is_null());

    let (endpoint, mut body) = openai::build(payload)?;
    if let Some(schema) = schema
        && let Some(object) = body.as_object_mut()
    {
        object.insert(
            "response_format".to_string(),
            json!({ "type": "json_schema", "json_schema": { "schema": schema } }),
        );
    }

    Ok((endpoint, body))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    #[test]
    fn perplexity_gets_alternating_turns_and_a_bare_schema() {
        let payload = json!({
            "chat_completion_source": "perplexity",
            "model": "sonar",
            "messages": [
                { "role": "system", "content": "rules" },
                { "role": "assistant", "content": "greeting" },
                { "role": "user", "content": "one" },
                { "role": "user", "content": "two" }
            ],
            "json_schema": {
                "name": "answer",
                "strict": true,
                "value": { "type": "object" }
            }
        });

        let (endpoint, body) = build(payload.as_object().cloned().unwrap()).unwrap();

        assert_eq!(endpoint, "/chat/completions");
        let roles = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["role"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(roles, ["system", "user", "assistant", "user"]);
        assert_eq!(
            body["response_format"],
            json!({ "type": "json_schema", "json_schema": { "schema": { "type": "object" } } })
        );
        assert_eq!(body.get("json_schema"), None::<&Value>);
    }
}
