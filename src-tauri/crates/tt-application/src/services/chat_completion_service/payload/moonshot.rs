use serde_json::{Map, Value};

use crate::errors::ApplicationError;

use super::super::model_capabilities::{VersionedModel, map_low_high_max_reasoning_effort};
use super::openai;
use super::shared::add_assistant_prefix;

const KIMI_FIXED_SAMPLING_FIELDS: &[&str] = &[
    "temperature",
    "top_p",
    "presence_penalty",
    "frequency_penalty",
];

pub(super) fn build(mut payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let kimi = payload
        .get("model")
        .and_then(Value::as_str)
        .and_then(|model| VersionedModel::parse(model, "kimi-k"));
    let fixed_sampling = kimi.as_ref().is_some_and(|kimi| kimi.version >= (2, 5));
    let k3 = kimi.as_ref().is_some_and(|kimi| kimi.version.0 >= 3);
    let always_thinks = k3
        || kimi
            .as_ref()
            .is_some_and(|kimi| kimi.version == (2, 7) && kimi.is("code"));
    let include_reasoning = payload
        .get("include_reasoning")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reasoning_effort = payload.remove("reasoning_effort");
    let reasoning_effort = if k3 {
        reasoning_effort
            .as_ref()
            .and_then(Value::as_str)
            .map(|value| map_low_high_max_reasoning_effort(value, "Moonshot"))
            .transpose()?
            .flatten()
            .map(|effort| Value::String(effort.to_string()))
    } else {
        reasoning_effort
    };
    if let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) {
        add_assistant_prefix(messages, "partial");
    }

    let (endpoint, mut upstream_payload) = openai::build(payload)?;

    if endpoint == "/chat/completions"
        && let Some(body) = upstream_payload.as_object_mut()
    {
        if fixed_sampling {
            for field in KIMI_FIXED_SAMPLING_FIELDS {
                body.remove(*field);
            }
        }
        if let Some(reasoning_effort) = reasoning_effort {
            body.insert("reasoning_effort".to_string(), reasoning_effort);
        }
        if !always_thinks {
            body.insert(
                "thinking".to_string(),
                serde_json::json!({
                    "type": if include_reasoning { "enabled" } else { "disabled" },
                }),
            );
        }
    }

    Ok((endpoint, upstream_payload))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    #[test]
    fn moonshot_sends_only_the_parameters_each_kimi_model_accepts() {
        for (model, include_reasoning, keeps_sampling, thinking, effort) in [
            ("kimi-k3.1-pro-highspeed", true, false, None, "high"),
            ("kimi-k2.7-code", false, false, None, "medium"),
            ("kimi-k2.6", false, false, Some("disabled"), "medium"),
            ("moonshot-v1-8k", true, true, Some("enabled"), "medium"),
        ] {
            let payload = json!({
                "model": model,
                "messages": [{"role": "user", "content": "hello"}],
                "include_reasoning": include_reasoning,
                "reasoning_effort": "medium",
                "temperature": 0.42,
                "top_p": 0.88,
                "presence_penalty": 0.2,
                "frequency_penalty": 0.1,
                "n": 2,
                "chat_completion_source": "moonshot"
            })
            .as_object()
            .cloned()
            .expect("payload must be object");

            let (endpoint, upstream) = build(payload).expect("build should succeed");

            assert_eq!(endpoint, "/chat/completions");
            for field in [
                "temperature",
                "top_p",
                "presence_penalty",
                "frequency_penalty",
            ] {
                assert_eq!(
                    upstream.get(field).is_some(),
                    keeps_sampling,
                    "{model} {field}"
                );
            }
            assert_eq!(upstream["n"], 2, "{model}");
            assert_eq!(
                upstream.pointer("/thinking/type").and_then(Value::as_str),
                thinking,
                "{model}"
            );
            assert_eq!(upstream["reasoning_effort"], effort, "{model}");
        }
    }

    #[test]
    fn moonshot_marks_assistant_prefill_as_partial() {
        let payload = json!({
            "model": "kimi-k3",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "prefill"}
            ],
            "chat_completion_source": "moonshot"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let partial = upstream
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.last())
            .and_then(Value::as_object)
            .and_then(|message| message.get("partial"))
            .and_then(Value::as_bool);

        assert_eq!(partial, Some(true));
    }
}
