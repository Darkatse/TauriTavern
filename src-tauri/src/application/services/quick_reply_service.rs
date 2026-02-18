use serde_json::Value;
use std::sync::Arc;

use crate::application::errors::ApplicationError;
use crate::domain::models::quick_reply::QuickReplySet;
use crate::domain::repositories::quick_reply_repository::QuickReplyRepository;

pub struct QuickReplyService {
    quick_reply_repository: Arc<dyn QuickReplyRepository>,
}

impl QuickReplyService {
    pub fn new(quick_reply_repository: Arc<dyn QuickReplyRepository>) -> Self {
        Self {
            quick_reply_repository,
        }
    }

    pub async fn save_quick_reply_set(&self, payload: Value) -> Result<(), ApplicationError> {
        let set = Self::parse_set(payload)?;
        set.validate().map_err(ApplicationError::ValidationError)?;

        self.quick_reply_repository
            .save_quick_reply_set(&set)
            .await?;
        Ok(())
    }

    pub async fn delete_quick_reply_set(&self, payload: Value) -> Result<(), ApplicationError> {
        let name = Self::extract_name(&payload)?;
        self.quick_reply_repository
            .delete_quick_reply_set(&name)
            .await?;
        Ok(())
    }

    fn parse_set(payload: Value) -> Result<QuickReplySet, ApplicationError> {
        if !payload.is_object() {
            return Err(ApplicationError::ValidationError(
                "Quick Reply payload must be a JSON object".to_string(),
            ));
        }

        let name = Self::extract_name(&payload)?;
        Ok(QuickReplySet::new(name, payload))
    }

    fn extract_name(payload: &Value) -> Result<String, ApplicationError> {
        let name = payload
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ApplicationError::ValidationError("Quick Reply set name is required".to_string())
            })?;

        Ok(name.to_string())
    }
}
