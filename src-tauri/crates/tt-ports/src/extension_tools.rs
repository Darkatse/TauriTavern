use async_trait::async_trait;
use tokio::sync::watch;
use tt_contracts::extension_tools::{ExtensionTool, ExtensionToolCall, ExtensionToolReply};
use tt_domain::errors::DomainError;

#[async_trait]
pub trait ExtensionTools: Send + Sync {
    fn list(&self) -> Result<Vec<ExtensionTool>, DomainError>;

    async fn call(
        &self,
        call: ExtensionToolCall,
        cancel: watch::Receiver<bool>,
    ) -> Result<ExtensionToolReply, DomainError>;
}
