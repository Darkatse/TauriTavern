use chrono::Utc;
use serde_json::{Value, json};

use tt_domain::errors::DomainError;

fn default_header() -> Value {
    json!({
        "chat_metadata": {},
        "user_name": "unused",
        "character_name": "unused",
    })
}

fn make_message(name: &str, is_user: bool, mes: &str) -> Value {
    json!({
        "name": name,
        "is_user": is_user,
        "send_date": Utc::now().to_rfc3339(),
        "mes": mes,
        "extra": {},
    })
}

fn is_js_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().map(|v| v != 0.0).unwrap_or(false),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

fn flatten_chub_line(line: &str) -> Result<(String, bool), DomainError> {
    let Ok(mut value) = serde_json::from_str::<Value>(line) else {
        return Ok((line.to_string(), false));
    };

    if value.is_null() {
        return Err(DomainError::InvalidData(
            "Failed to flatten Chub chat data".to_string(),
        ));
    }

    let mut changed = false;
    if let Some(object) = value.as_object_mut() {
        if let Some(mes_value) = object.get_mut("mes") {
            let message_value = mes_value
                .as_object()
                .and_then(|message| message.get("message"))
                .filter(|message| is_js_truthy(Some(message)))
                .cloned();
            if let Some(message_value) = message_value {
                *mes_value = message_value;
                changed = true;
            }
        }

        if let Some(swipes_value) = object.get_mut("swipes")
            && let Some(swipes) = swipes_value.as_array_mut()
        {
            for swipe in swipes {
                let message_value = swipe
                    .as_object()
                    .and_then(|entry| entry.get("message"))
                    .filter(|message| is_js_truthy(Some(message)))
                    .cloned();
                if let Some(message_value) = message_value {
                    *swipe = message_value;
                    changed = true;
                }
            }
        }
    }

    let serialized = serde_json::to_string(&value).map_err(|e| {
        DomainError::InternalError(format!("Failed to serialize flattened chat line: {}", e))
    })?;
    Ok((serialized, changed))
}

fn flatten_chub_jsonl(data: &str) -> Result<(String, bool), DomainError> {
    let mut changed = false;
    let mut lines = Vec::new();

    for line in data.split('\n') {
        let (flattened, line_changed) = flatten_chub_line(line)?;
        changed |= line_changed;
        lines.push(flattened);
    }

    Ok((lines.join("\n"), changed))
}

fn import_ooba_payload(
    user_name: &str,
    character_name: &str,
    data: &Value,
) -> Result<Vec<Value>, DomainError> {
    let messages = data
        .get("data_visible")
        .and_then(Value::as_array)
        .ok_or_else(|| DomainError::InvalidData("Invalid Ooba chat format".to_string()))?;

    let mut payload = vec![default_header()];
    for pair in messages {
        let Some(items) = pair.as_array() else {
            continue;
        };

        if let Some(user_message) = items.first().and_then(Value::as_str) {
            if !user_message.is_empty() {
                payload.push(make_message(user_name, true, user_message));
            }
        }

        if let Some(character_message) = items.get(1).and_then(Value::as_str) {
            if !character_message.is_empty() {
                payload.push(make_message(character_name, false, character_message));
            }
        }
    }

    Ok(payload)
}

fn import_agnai_payload(
    user_name: &str,
    character_name: &str,
    data: &Value,
) -> Result<Vec<Value>, DomainError> {
    let messages = data
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| DomainError::InvalidData("Invalid Agnai chat format".to_string()))?;

    let mut payload = vec![default_header()];
    for message in messages {
        // Match SillyTavern upstream semantics: `!!message.userId`
        let is_user = is_js_truthy(message.get("userId"));
        let text = message
            .get("msg")
            .and_then(Value::as_str)
            .unwrap_or_default();
        payload.push(make_message(
            if is_user { user_name } else { character_name },
            is_user,
            text,
        ));
    }

    Ok(payload)
}

fn import_cai_payloads(
    user_name: &str,
    character_name: &str,
    data: &Value,
) -> Result<Vec<Vec<Value>>, DomainError> {
    let histories = data
        .get("histories")
        .and_then(Value::as_object)
        .and_then(|entry| entry.get("histories"))
        .and_then(Value::as_array)
        .ok_or_else(|| DomainError::InvalidData("Invalid CAI chat format".to_string()))?;

    let payloads = histories
        .iter()
        .map(|history| {
            let mut payload = vec![default_header()];
            let messages = history
                .get("msgs")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for message in messages {
                let is_user = message
                    .get("src")
                    .and_then(Value::as_object)
                    .and_then(|src| src.get("is_human"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let text = message
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                payload.push(make_message(
                    if is_user { user_name } else { character_name },
                    is_user,
                    text,
                ));
            }
            payload
        })
        .collect();

    Ok(payloads)
}

fn import_kobold_payload(data: &Value) -> Result<Vec<Value>, DomainError> {
    let settings = data
        .get("savedsettings")
        .and_then(Value::as_object)
        .ok_or_else(|| DomainError::InvalidData("Invalid Kobold chat format".to_string()))?;

    let user_name = settings
        .get("chatname")
        .and_then(Value::as_str)
        .unwrap_or("User");
    let character_name = settings
        .get("chatopponent")
        .and_then(Value::as_str)
        .unwrap_or("Character")
        .split("||$||")
        .next()
        .unwrap_or("Character");

    const INPUT_TOKEN: &str = "{{[INPUT]}}";
    const OUTPUT_TOKEN: &str = "{{[OUTPUT]}}";

    let mut payload = vec![default_header()];
    if let Some(prompt) = data.get("prompt").and_then(Value::as_str) {
        let is_user = prompt.contains(INPUT_TOKEN);
        let message = prompt
            .replace(INPUT_TOKEN, "")
            .replace(OUTPUT_TOKEN, "")
            .trim()
            .to_string();
        payload.push(make_message(
            if is_user { user_name } else { character_name },
            is_user,
            &message,
        ));
    }

    let actions = data
        .get("actions")
        .and_then(Value::as_array)
        .ok_or_else(|| DomainError::InvalidData("Invalid Kobold chat format".to_string()))?;
    for action in actions {
        let text = action.as_str().unwrap_or_default();
        let is_user = text.contains(INPUT_TOKEN);
        let message = text
            .replace(INPUT_TOKEN, "")
            .replace(OUTPUT_TOKEN, "")
            .trim()
            .to_string();
        payload.push(make_message(
            if is_user { user_name } else { character_name },
            is_user,
            &message,
        ));
    }

    Ok(payload)
}

fn import_risu_payload(
    user_name: &str,
    character_name: &str,
    data: &Value,
) -> Result<Vec<Value>, DomainError> {
    let messages = data
        .get("data")
        .and_then(Value::as_object)
        .and_then(|entry| entry.get("message"))
        .and_then(Value::as_array)
        .ok_or_else(|| DomainError::InvalidData("Invalid RisuAI chat format".to_string()))?;

    let mut payload = vec![default_header()];
    for message in messages {
        let is_user = message
            .get("role")
            .and_then(Value::as_str)
            .map(|role| role == "user")
            .unwrap_or(false);
        let send_date = message
            .get("time")
            .and_then(Value::as_i64)
            .map(|epoch_ms| {
                chrono::DateTime::<Utc>::from_timestamp_millis(epoch_ms)
                    .unwrap_or_else(Utc::now)
                    .to_rfc3339()
            })
            .unwrap_or_else(|| Utc::now().to_rfc3339());

        payload.push(json!({
            "name": message
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(if is_user { user_name } else { character_name }),
            "is_user": is_user,
            "send_date": send_date,
            "mes": message.get("data").and_then(Value::as_str).unwrap_or_default(),
            "extra": {},
        }));
    }

    Ok(payload)
}

/// Import one or more chat payloads from JSON formats supported by SillyTavern.
pub fn import_chat_payloads_from_json(
    data: &Value,
    user_name: &str,
    character_name: &str,
) -> Result<Vec<Vec<Value>>, DomainError> {
    if data.get("savedsettings").is_some() {
        return Ok(vec![import_kobold_payload(data)?]);
    }

    if data.get("histories").is_some() {
        return import_cai_payloads(user_name, character_name, data);
    }

    if data.get("data_visible").and_then(Value::as_array).is_some() {
        return Ok(vec![import_ooba_payload(user_name, character_name, data)?]);
    }

    if data.get("messages").and_then(Value::as_array).is_some() {
        return Ok(vec![import_agnai_payload(user_name, character_name, data)?]);
    }

    if data.get("type").and_then(Value::as_str) == Some("risuChat") {
        return Ok(vec![import_risu_payload(user_name, character_name, data)?]);
    }

    Err(DomainError::InvalidData(
        "Unsupported chat import JSON format".to_string(),
    ))
}

/// Import a SillyTavern JSONL payload (with Chub flattening compatibility).
pub fn import_chat_jsonl_bytes(data: &str) -> Result<Vec<u8>, DomainError> {
    let header_line = data.split('\n').next().unwrap_or_default();
    let header: Value = serde_json::from_str(header_line).map_err(|e| {
        DomainError::InvalidData(format!("Unsupported chat import JSONL format: {}", e))
    })?;
    let is_valid_header = header
        .get("user_name")
        .or_else(|| header.get("name"))
        .or_else(|| header.get("chat_metadata"))
        .is_some();
    if !is_valid_header {
        return Err(DomainError::InvalidData(
            "Unsupported chat import JSONL format".to_string(),
        ));
    }

    let Ok((flattened, changed)) = flatten_chub_jsonl(data) else {
        return Ok(data.as_bytes().to_vec());
    };

    if changed {
        Ok(flattened.into_bytes())
    } else {
        Ok(data.as_bytes().to_vec())
    }
}

/// Export a JSONL chat payload to plain text.
pub fn export_payload_to_plain_text(payload: &[Value]) -> String {
    if payload.is_empty() {
        return String::new();
    }

    let header = payload.first().and_then(Value::as_object);
    let header_user_name = header
        .and_then(|entry| entry.get("user_name"))
        .and_then(Value::as_str)
        .unwrap_or("User");
    let header_character_name = header
        .and_then(|entry| entry.get("character_name"))
        .and_then(Value::as_str)
        .unwrap_or("Character");

    let mut output = String::new();
    for message in payload.iter().skip(1) {
        if message
            .get("is_system")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }

        let Some(raw_text) = message
            .get("extra")
            .and_then(Value::as_object)
            .and_then(|extra| extra.get("display_text"))
            .and_then(Value::as_str)
            .or_else(|| message.get("mes").and_then(Value::as_str))
        else {
            continue;
        };

        let is_user = message
            .get("is_user")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let name = message
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(if is_user {
                header_user_name
            } else {
                header_character_name
            });

        let normalized = raw_text.replace("\r\n", "\n").replace('\r', "\n");
        output.push_str(name);
        output.push_str(": ");
        output.push_str(&normalized);
        output.push_str("\n\n");
    }

    output
}

#[cfg(test)]
mod tests {
    use super::import_chat_payloads_from_json;
    use serde_json::json;

    #[test]
    fn import_agnai_treats_string_user_id_as_user_message() {
        let payload = json!({
            "messages": [
                { "userId": "u-1", "msg": "Hello" },
                { "msg": "Hi there" }
            ]
        });

        let imported = import_chat_payloads_from_json(&payload, "User", "Assistant")
            .expect("agnai payload should import");

        assert_eq!(imported.len(), 1);
        let chat = &imported[0];

        assert_eq!(chat.len(), 3);
        assert_eq!(chat[1].get("is_user").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(chat[1].get("name").and_then(|v| v.as_str()), Some("User"));
        assert_eq!(
            chat[2].get("name").and_then(|v| v.as_str()),
            Some("Assistant")
        );
    }
}
