use crate::application::errors::ApplicationError;
use crate::domain::models::character::sanitize_filename;
use crate::domain::models::chat::normalize_chat_file_stem;

pub(super) fn validate_character_path_component(value: &str) -> Result<(), ApplicationError> {
    if value.trim().is_empty() || sanitize_filename(value.trim()).is_empty() {
        return Err(ApplicationError::ValidationError(
            "Character name cannot be empty or invalid".to_string(),
        ));
    }

    Ok(())
}

pub(super) fn validate_chat_file_name(value: &str, label: &str) -> Result<(), ApplicationError> {
    if value.trim().is_empty() || normalize_chat_file_stem(value).is_none() {
        return Err(ApplicationError::ValidationError(format!(
            "{label} cannot be empty or invalid"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_character_path_component, validate_chat_file_name};

    #[test]
    fn rejects_path_components_that_sanitize_to_empty() {
        assert!(validate_character_path_component("CON").is_err());
        assert!(validate_chat_file_name("*.jsonl", "Chat file name").is_err());
        assert!(validate_chat_file_name(".jsonl", "Chat file name").is_err());
    }

    #[test]
    fn accepts_safe_chat_file_names_with_or_without_jsonl_extension() {
        assert!(validate_character_path_component("Alice").is_ok());
        assert!(validate_chat_file_name("session", "Chat file name").is_ok());
        assert!(validate_chat_file_name("session.JSONL", "Chat file name").is_ok());
        assert!(validate_chat_file_name("中文会话.jsonl", "Chat file name").is_ok());
    }
}
