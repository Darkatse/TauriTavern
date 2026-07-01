use std::sync::Arc;

use async_trait::async_trait;

use crate::application::services::character_service::CharacterService;
use crate::application::services::chat_service::ChatService;
use crate::application::services::group_chat_service::GroupChatService;
use crate::application::services::group_service::GroupService;
use crate::application::services::secret_service::SecretService;
use crate::domain::errors::DomainError;
use tt_ports::sync::DataChangeReconciler;

pub(in crate::app::composition) fn data_change_reconciler(
    character_service: Arc<CharacterService>,
    chat_service: Arc<ChatService>,
    group_chat_service: Arc<GroupChatService>,
    group_service: Arc<GroupService>,
    secret_service: Arc<SecretService>,
) -> Arc<dyn DataChangeReconciler> {
    Arc::new(ServiceCacheReconciler {
        character_service,
        chat_service,
        group_chat_service,
        group_service,
        secret_service,
    })
}

struct ServiceCacheReconciler {
    character_service: Arc<CharacterService>,
    chat_service: Arc<ChatService>,
    group_chat_service: Arc<GroupChatService>,
    group_service: Arc<GroupService>,
    secret_service: Arc<SecretService>,
}

#[async_trait]
impl DataChangeReconciler for ServiceCacheReconciler {
    async fn reconcile(&self, reason: &str) -> Result<(), DomainError> {
        tracing::info!(
            reason = reason,
            "Refreshing runtime caches after external data change"
        );

        self.character_service.clear_cache().await?;
        self.chat_service.clear_cache().await?;
        self.group_chat_service.clear_cache().await?;
        self.group_service.clear_cache().await?;
        self.secret_service.clear_cache().await?;

        Ok(())
    }
}
