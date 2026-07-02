use std::sync::Arc;

use async_trait::async_trait;

use tt_application::services::character_service::CharacterService;
use tt_application::services::chat_service::ChatService;
use tt_application::services::group_chat_service::GroupChatService;
use tt_application::services::group_service::GroupService;
use tt_application::services::secret_service::SecretService;
use tt_domain::errors::DomainError;
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
