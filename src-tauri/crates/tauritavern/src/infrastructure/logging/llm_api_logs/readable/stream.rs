use serde_json::Value;

use tt_ports::repositories::chat_completion_repository::ChatCompletionSource;

use super::reasoning::collect_visible_reasoning_texts;

pub(in crate::infrastructure::logging::llm_api_logs) struct StreamReadableCollector {
    source: ChatCompletionSource,
    text_buffer: String,
    reasoning_buffer: String,
}

impl StreamReadableCollector {
    pub(in crate::infrastructure::logging::llm_api_logs) fn new(
        source: ChatCompletionSource,
    ) -> Self {
        Self {
            source,
            text_buffer: String::new(),
            reasoning_buffer: String::new(),
        }
    }

    pub(in crate::infrastructure::logging::llm_api_logs) fn push(&mut self, chunk: &str) {
        let trimmed = chunk.trim();
        if trimmed.is_empty() || trimmed == "[DONE]" {
            return;
        }

        match self.source {
            ChatCompletionSource::Claude => self.push_claude(trimmed),
            ChatCompletionSource::Cohere => self.push_cohere(trimmed),
            ChatCompletionSource::Makersuite | ChatCompletionSource::VertexAi => {
                self.push_gemini_like(trimmed)
            }
            _ => self.push_openai_like(trimmed),
        }
    }

    fn push_openai_like(&mut self, trimmed: &str) {
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            return;
        };

        let choices = value.get("choices").and_then(Value::as_array);
        let Some(choices) = choices else {
            return;
        };

        for choice in choices {
            let delta = choice.get("delta").and_then(Value::as_object);
            let Some(delta) = delta else {
                continue;
            };

            if let Some(text) = delta.get("reasoning_content").and_then(Value::as_str) {
                self.reasoning_buffer.push_str(text);
            }
            if let Some(text) = delta
                .get("thought_summary")
                .or_else(|| delta.get("thinking"))
                .and_then(Value::as_str)
            {
                self.reasoning_buffer.push_str(text);
            }
            if let Some(text) = delta.get("content").and_then(Value::as_str) {
                self.text_buffer.push_str(text);
            }
        }
    }

    fn push_cohere(&mut self, trimmed: &str) {
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            return;
        };

        let message = value
            .get("delta")
            .and_then(Value::as_object)
            .and_then(|delta| delta.get("message"))
            .and_then(Value::as_object);
        let Some(message) = message else {
            return;
        };

        if let Some(text) = message
            .get("content")
            .and_then(Value::as_object)
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str)
        {
            self.text_buffer.push_str(text);
            return;
        }

        if let Some(tool_plan) = message.get("tool_plan").and_then(Value::as_str) {
            self.text_buffer.push_str(tool_plan);
        }
    }

    fn push_claude(&mut self, trimmed: &str) {
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            return;
        };

        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if kind != "content_block_delta" {
            return;
        }

        let Some(delta) = value.get("delta").and_then(Value::as_object) else {
            return;
        };
        let delta_type = delta
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match delta_type {
            "text_delta" => {
                if let Some(text) = delta.get("text").and_then(Value::as_str) {
                    self.text_buffer.push_str(text);
                }
            }
            "thinking_delta" => {
                if let Some(text) = delta
                    .get("thinking")
                    .or_else(|| delta.get("text"))
                    .and_then(Value::as_str)
                {
                    self.reasoning_buffer.push_str(text);
                }
            }
            _ => {}
        }
    }

    fn push_gemini_like(&mut self, trimmed: &str) {
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            return;
        };

        let Some(candidates) = value.get("candidates").and_then(Value::as_array) else {
            return;
        };
        let Some(first) = candidates.first().and_then(Value::as_object) else {
            return;
        };
        let Some(content) = first.get("content").and_then(Value::as_object) else {
            return;
        };
        let Some(parts) = content.get("parts").and_then(Value::as_array) else {
            return;
        };
        for part in parts {
            let Some(part_object) = part.as_object() else {
                continue;
            };
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                if part_object
                    .get("thought")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    self.reasoning_buffer.push_str(text);
                } else {
                    self.text_buffer.push_str(text);
                }
                continue;
            }
            if part_object
                .get("thought")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                for text in collect_visible_reasoning_texts(part_object) {
                    self.reasoning_buffer.push_str(&text);
                }
            }
        }
    }

    pub(in crate::infrastructure::logging::llm_api_logs) fn into_string(self) -> String {
        let reasoning_is_empty = self.reasoning_buffer.trim().is_empty();
        let text_is_empty = self.text_buffer.trim().is_empty();
        match (reasoning_is_empty, text_is_empty) {
            (true, true) => String::new(),
            (true, false) => self.text_buffer,
            (false, true) => format!("[reasoning]\n{}", self.reasoning_buffer),
            (false, false) => format!(
                "[reasoning]\n{}\n\n[assistant]\n{}",
                self.reasoning_buffer, self.text_buffer
            ),
        }
    }
}
