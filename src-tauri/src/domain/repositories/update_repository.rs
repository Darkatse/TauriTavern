use async_trait::async_trait;

use crate::domain::errors::DomainError;
use crate::domain::models::update::ReleaseInfo;

#[async_trait]
pub trait UpdateRepository: Send + Sync {
    /// 获取指定 GitHub 仓库的最新 Release。
    async fn get_latest_release(&self) -> Result<ReleaseInfo, DomainError>;
}

