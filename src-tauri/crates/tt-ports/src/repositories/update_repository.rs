use async_trait::async_trait;

use tt_domain::errors::DomainError;
use tt_domain::models::update::{ReleaseInfo, UpdateChannel};

#[async_trait]
pub trait UpdateRepository: Send + Sync {
    /// 获取指定更新渠道当前发布的 Release。
    async fn get_release(&self, channel: UpdateChannel) -> Result<ReleaseInfo, DomainError>;
}
