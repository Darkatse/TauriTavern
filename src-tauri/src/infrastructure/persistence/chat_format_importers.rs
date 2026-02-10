use crate::domain::errors::DomainError;
use crate::domain::models::chat::{
    humanized_date, message_date_format, Chat, ChatMessage, MessageExtra,
};
use crate::infrastructure::logging::logger;
use chrono::{Local, Utc};
use serde_json::Value;

/// Import a chat from SillyTavern format
///
/// # Arguments
///
/// * `data` - The chat data as a JSON value
///
/// # Returns
///
/// * `Ok(Chat)` - The imported chat
/// * `Err(DomainError)` - If the chat cannot be imported
pub fn import_from_sillytavern(data: &Value) -> Result<Chat, DomainError> {
    logger::debug("Importing chat from SillyTavern format");

    // Extract metadata
    let user_name = data["user_name"].as_str().unwrap_or("User").to_string();
    let character_name = data["character_name"]
        .as_str()
        .unwrap_or("Character")
        .to_string();
    let create_date = data["create_date"]
        .as_str()
        .unwrap_or(&humanized_date(Utc::now()))
        .to_string();

    // Create a new chat
    let mut chat = Chat {
        user_name,
        character_name,
        create_date,
        ..Default::default()
    };

    // Extract chat metadata
    if let Some(chat_metadata) = data.get("chat_metadata") {
        if let Ok(parsed) = serde_json::from_value(chat_metadata.clone()) {
            chat.chat_metadata = parsed;
        }
    }

    // Extract messages
    if let Some(messages) = data.get("messages").and_then(|m| m.as_array()) {
        for message in messages {
            if let Ok(parsed) = serde_json::from_value(message.clone()) {
                chat.add_message(parsed);
            }
        }
    }

    Ok(chat)
}

/// Import a chat from Ooba format
///
/// # Arguments
///
/// * `data` - The chat data as a JSON value
///
/// # Returns
///
/// * `Ok(Chat)` - The imported chat
/// * `Err(DomainError)` - If the chat cannot be imported
pub fn import_from_ooba(data: &Value) -> Result<Chat, DomainError> {
    logger::debug("Importing chat from Ooba format");

    // Extract metadata
    let user_name = data["user"].as_str().unwrap_or("User").to_string();
    let character_name = data["bot"].as_str().unwrap_or("Character").to_string();
    let create_date = humanized_date(Utc::now());

    // Create a new chat
    let mut chat = Chat {
        user_name,
        character_name,
        create_date,
        ..Default::default()
    };

    // Extract messages
    if let Some(messages) = data.get("messages").and_then(|m| m.as_array()) {
        for message in messages {
            let role = message["role"].as_str().unwrap_or("user");
            let content = message["content"].as_str().unwrap_or("").to_string();

            let chat_message = if role == "user" {
                ChatMessage::user(&chat.user_name, &content)
            } else {
                ChatMessage::character(&chat.character_name, &content)
            };

            chat.add_message(chat_message);
        }
    }

    Ok(chat)
}

/// Import a chat from Agnai format
///
/// # Arguments
///
/// * `data` - The chat data as a JSON value
///
/// # Returns
///
/// * `Ok(Chat)` - The imported chat
/// * `Err(DomainError)` - If the chat cannot be imported
pub fn import_from_agnai(data: &Value) -> Result<Chat, DomainError> {
    logger::debug("Importing chat from Agnai format");

    // Extract metadata
    let user_name = data["user_name"].as_str().unwrap_or("User").to_string();
    let character_name = data["character_name"]
        .as_str()
        .unwrap_or("Character")
        .to_string();
    let create_date = humanized_date(Utc::now());

    // Create a new chat
    let mut chat = Chat {
        user_name,
        character_name,
        create_date,
        ..Default::default()
    };

    // Extract messages
    if let Some(messages) = data.get("messages").and_then(|m| m.as_array()) {
        for message in messages {
            let role = message["role"].as_str().unwrap_or("user");
            let content = message["content"].as_str().unwrap_or("").to_string();

            let chat_message = if role == "user" {
                ChatMessage::user(&chat.user_name, &content)
            } else {
                ChatMessage::character(&chat.character_name, &content)
            };

            chat.add_message(chat_message);
        }
    }

    Ok(chat)
}

/// Import a chat from CAI Tools format
///
/// # Arguments
///
/// * `data` - The chat data as a JSON value
///
/// # Returns
///
/// * `Ok(Chat)` - The imported chat
/// * `Err(DomainError)` - If the chat cannot be imported
pub fn import_from_cai_tools(data: &Value) -> Result<Chat, DomainError> {
    logger::debug("Importing chat from CAI Tools format");

    // Extract metadata
    let user_name = data["user"].as_str().unwrap_or("User").to_string();
    let character_name = data["char"].as_str().unwrap_or("Character").to_string();
    let create_date = humanized_date(Utc::now());

    // Create a new chat
    let mut chat = Chat {
        user_name,
        character_name,
        create_date,
        ..Default::default()
    };

    // Extract messages
    if let Some(messages) = data.get("msgs").and_then(|m| m.as_array()) {
        for message in messages {
            let is_user = message["is_user"].as_bool().unwrap_or(false);
            let content = message["msg"].as_str().unwrap_or("").to_string();

            let chat_message = if is_user {
                ChatMessage::user(&chat.user_name, &content)
            } else {
                ChatMessage::character(&chat.character_name, &content)
            };

            chat.add_message(chat_message);
        }
    }

    Ok(chat)
}

/// Import a chat from Kobold Lite format
///
/// # Arguments
///
/// * `data` - The chat data as a JSON value
///
/// # Returns
///
/// * `Ok(Chat)` - The imported chat
/// * `Err(DomainError)` - If the chat cannot be imported
pub fn import_from_kobold_lite(data: &Value) -> Result<Chat, DomainError> {
    logger::debug("Importing chat from Kobold Lite format");

    // Extract metadata
    let user_name = "User".to_string();
    let character_name = "Character".to_string();
    let create_date = humanized_date(Utc::now());

    // Create a new chat
    let mut chat = Chat {
        user_name,
        character_name,
        create_date,
        ..Default::default()
    };

    // Extract messages
    if let Some(messages) = data.get("messages").and_then(|m| m.as_array()) {
        for message in messages {
            let role = message["role"].as_str().unwrap_or("user");
            let content = message["content"].as_str().unwrap_or("").to_string();

            let chat_message = if role == "user" {
                ChatMessage::user(&chat.user_name, &content)
            } else {
                ChatMessage::character(&chat.character_name, &content)
            };

            chat.add_message(chat_message);
        }
    }

    Ok(chat)
}

/// Import a chat from RisuAI format
///
/// # Arguments
///
/// * `data` - The chat data as a JSON value
///
/// # Returns
///
/// * `Ok(Chat)` - The imported chat
/// * `Err(DomainError)` - If the chat cannot be imported
pub fn import_from_risuai(data: &Value) -> Result<Chat, DomainError> {
    logger::debug("Importing chat from RisuAI format");

    // Extract metadata
    let user_name = data["user"].as_str().unwrap_or("User").to_string();
    let character_name = data["char"].as_str().unwrap_or("Character").to_string();
    let create_date = humanized_date(Utc::now());

    // Create a new chat
    let mut chat = Chat {
        user_name,
        character_name,
        create_date,
        ..Default::default()
    };

    // Extract messages
    if let Some(messages) = data.get("data").and_then(|m| m.as_array()) {
        for message in messages {
            let role = message["role"].as_str().unwrap_or("user");
            let content = message["content"].as_str().unwrap_or("").to_string();

            let chat_message = if role == "user" {
                ChatMessage::user(&chat.user_name, &content)
            } else {
                ChatMessage::character(&chat.character_name, &content)
            };

            chat.add_message(chat_message);
        }
    }

    Ok(chat)
}

/// Export a chat to plain text format
///
/// # Arguments
///
/// * `chat` - The chat to export
///
/// # Returns
///
/// * `String` - The exported chat as plain text
pub fn export_to_plain_text(chat: &Chat) -> String {
    logger::debug("Exporting chat to plain text format");

    let mut output = String::new();

    // Add header
    output.push_str(&format!("# Chat with {}\n", chat.character_name));
    output.push_str(&format!("# Date: {}\n\n", chat.create_date));

    // Add messages
    for message in &chat.messages {
        let name = if message.is_user {
            &chat.user_name
        } else {
            &message.name
        };

        output.push_str(&format!("## {}\n", name));
        output.push_str(&format!("{}\n\n", message.mes));
    }

    output
}
