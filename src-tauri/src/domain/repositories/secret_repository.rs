use async_trait::async_trait;
use std::collections::HashMap;

use crate::domain::errors::DomainError;
use crate::domain::models::secret::Secrets;

#[async_trait]
pub trait SecretRepository: Send + Sync {
    /// 保存所有密钥
    async fn save(&self, secrets: &Secrets) -> Result<(), DomainError>;

    /// 加载所有密钥
    async fn load(&self) -> Result<Secrets, DomainError>;

    /// 写入单个密钥
    async fn write_secret(&self, key: &str, value: &str) -> Result<(), DomainError>;

    /// 读取单个密钥
    async fn read_secret(&self, key: &str) -> Result<Option<String>, DomainError>;

    /// 删除单个密钥
    async fn delete_secret(&self, key: &str) -> Result<(), DomainError>;

    /// 获取所有密钥的状态（是否存在有效值）
    async fn get_secret_state(&self) -> Result<HashMap<String, bool>, DomainError>;
}
