use std::sync::Arc;

use crate::errors::ApplicationError;
use tt_domain::models::quick_reply::QuickReplySet;
use tt_ports::repositories::quick_reply_repository::QuickReplyRepository;

pub struct QuickReplyService {
    quick_reply_repository: Arc<dyn QuickReplyRepository>,
}

impl QuickReplyService {
    pub fn new(quick_reply_repository: Arc<dyn QuickReplyRepository>) -> Self {
        Self {
            quick_reply_repository,
        }
    }

    pub async fn save_quick_reply_set(&self, set_json: &str) -> Result<(), ApplicationError> {
        let set = QuickReplySet::parse(set_json)?;

        self.quick_reply_repository
            .save_quick_reply_set(&set)
            .await?;
        Ok(())
    }

    pub async fn delete_quick_reply_set(&self, name: &str) -> Result<(), ApplicationError> {
        self.quick_reply_repository
            .delete_quick_reply_set(name)
            .await?;
        Ok(())
    }
}
