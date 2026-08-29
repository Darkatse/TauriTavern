use serde_json::{Map, Value};

use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_completion_repository::ChatCompletionToolCallDelta;

use super::HttpChatCompletionRepository;

pub(super) async fn consume_generate_content_stream(
    provider_name: &str,
    response: reqwest::Response,
    on_tool_call_delta: &mut (dyn FnMut(ChatCompletionToolCallDelta) + Send),
) -> Result<Value, DomainError> {
    let mut accumulator = GeminiStreamAccumulator::default();

    HttpChatCompletionRepository::consume_sse_response(provider_name, response, |event| {
        accumulator.apply_event(event, on_tool_call_delta)
    })
    .await?;

    accumulator.finish()
}

#[derive(Default)]
struct GeminiStreamAccumulator {
    response: Map<String, Value>,
    candidate: Map<String, Value>,
    content: Map<String, Value>,
    parts: Vec<Value>,
    tool_call_count: usize,
}

impl GeminiStreamAccumulator {
    fn apply_event(
        &mut self,
        raw_event: &[u8],
        on_tool_call_delta: &mut dyn FnMut(ChatCompletionToolCallDelta),
    ) -> Result<(), DomainError> {
        let mut event = match serde_json::from_slice::<Value>(raw_event)
            .map_err(|error| invalid_stream(format!("event is not valid JSON: {error}")))?
        {
            Value::Object(event) => event,
            _ => return Err(invalid_stream("event must be an object")),
        };

        if let Some(error) = event.get("error").filter(|error| !error.is_null()) {
            return Err(invalid_stream(format!(
                "upstream returned an error event: {error}"
            )));
        }

        let candidates = match event.remove("candidates") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(candidates)) => candidates,
            Some(_) => return Err(invalid_stream("candidates must be an array")),
        };
        self.response.extend(event);

        let Some(candidate) = candidates.into_iter().next() else {
            return Ok(());
        };
        let Value::Object(mut candidate) = candidate else {
            return Err(invalid_stream("candidate must be an object"));
        };
        let content = candidate.remove("content");
        self.candidate.extend(candidate);

        let mut content = match content {
            None | Some(Value::Null) => return Ok(()),
            Some(Value::Object(content)) => content,
            Some(_) => return Err(invalid_stream("candidate content must be an object")),
        };
        let parts = match content.remove("parts") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(parts)) => parts,
            Some(_) => return Err(invalid_stream("content parts must be an array")),
        };
        self.content.extend(content);

        for (index, part) in parts.into_iter().enumerate() {
            self.apply_part(part, index == 0, on_tool_call_delta);
        }

        Ok(())
    }

    fn apply_part(
        &mut self,
        part: Value,
        merge_with_previous: bool,
        on_tool_call_delta: &mut dyn FnMut(ChatCompletionToolCallDelta),
    ) {
        let mut part = match part {
            Value::Object(part) => part,
            part => {
                self.parts.push(part);
                return;
            }
        };

        if let Some(function_call) = part.get("functionCall").and_then(Value::as_object) {
            let tool_call_index = self.tool_call_count;
            self.tool_call_count += 1;
            if let (Some(name), Some(arguments)) = (
                function_call.get("name").and_then(Value::as_str),
                function_call.get("args"),
            ) {
                on_tool_call_delta(ChatCompletionToolCallDelta {
                    tool_call_index,
                    name: name.to_string(),
                    arguments_fragment: arguments.to_string(),
                });
            }
            self.parts.push(Value::Object(part));
            return;
        }

        let Some(text) = part.get("text").and_then(Value::as_str).map(str::to_string) else {
            self.parts.push(Value::Object(part));
            return;
        };
        if text.is_empty() && part.len() == 1 {
            return;
        }

        if merge_with_previous
            && let Some(previous) = self.parts.last_mut().and_then(Value::as_object_mut)
            && previous.get("thought").and_then(Value::as_bool)
                == part.get("thought").and_then(Value::as_bool)
            && let Some(Value::String(previous_text)) = previous.get_mut("text")
        {
            previous_text.push_str(&text);
            part.remove("text");
            previous.extend(part);
            return;
        }

        self.parts.push(Value::Object(part));
    }

    fn finish(mut self) -> Result<Value, DomainError> {
        if self
            .candidate
            .get("finishReason")
            .and_then(Value::as_str)
            .is_none()
        {
            return Err(invalid_stream("ended without a finish reason"));
        }

        self.content
            .insert("parts".to_string(), Value::Array(self.parts));
        self.candidate
            .insert("content".to_string(), Value::Object(self.content));
        self.response.insert(
            "candidates".to_string(),
            Value::Array(vec![Value::Object(self.candidate)]),
        );
        Ok(Value::Object(self.response))
    }
}

fn invalid_stream(message: impl std::fmt::Display) -> DomainError {
    DomainError::transient(format!(
        "model.upstream_invalid_response: Gemini stream {message}"
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn stream_accumulates_final_and_projects_tool_call() {
        let events = [
            json!({"candidates":[{"content":{"role":"model","parts":[{"text":"Plan ","thought":true}]}}],"modelVersion":"gemini-test"}),
            json!({"candidates":[{"content":{"role":"model","parts":[{"text":"then act","thought":true,"thoughtSignature":"sig"}]}}]}),
            json!({"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"id":"call_1","name":"write_file","args":{"path":"a.md","content":"hello"}},"thoughtSignature":"call-sig"}]}}]}),
            json!({"candidates":[{"content":{"role":"model","parts":[{"text":""}]},"finishReason":"STOP"}],"usageMetadata":{"totalTokenCount":12}}),
        ];
        let mut accumulator = GeminiStreamAccumulator::default();
        let mut deltas = Vec::new();
        for event in events {
            accumulator
                .apply_event(&serde_json::to_vec(&event).unwrap(), &mut |delta| {
                    deltas.push(delta)
                })
                .unwrap();
        }

        assert_eq!(
            deltas,
            vec![ChatCompletionToolCallDelta {
                tool_call_index: 0,
                name: "write_file".to_string(),
                arguments_fragment: "{\"content\":\"hello\",\"path\":\"a.md\"}".to_string(),
            }]
        );

        let response = accumulator.finish().unwrap();
        assert_eq!(response["modelVersion"], "gemini-test");
        assert_eq!(response["usageMetadata"]["totalTokenCount"], 12);
        assert_eq!(response["candidates"][0]["finishReason"], "STOP");
        assert_eq!(
            response["candidates"][0]["content"]["parts"],
            json!([
                {
                    "text": "Plan then act",
                    "thought": true,
                    "thoughtSignature": "sig"
                },
                {
                    "functionCall": {
                        "id": "call_1",
                        "name": "write_file",
                        "args": { "path": "a.md", "content": "hello" }
                    },
                    "thoughtSignature": "call-sig"
                }
            ])
        );
    }
}
