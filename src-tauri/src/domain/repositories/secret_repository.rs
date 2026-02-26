use async_trait::async_trait;

use crate::domain::errors::DomainError;
use crate::domain::models::secret::Secrets;

#[async_trait]
pub trait SecretRepository: Send + Sync {
    /// 保存所有密钥
    async fn save(&self, secrets: &Secrets) -> Result<(), DomainError>;

    /// 加载所有密钥
    async fn load(&self) -> Result<Secrets, DomainError>;

    /// 写入单个密钥，返回新密钥 ID
    async fn write_secret(
        &self,
        key: &str,
        value: &str,
        label: &str,
    ) -> Result<String, DomainError>;

    /// 读取单个密钥（按 ID 或当前 active）
    async fn read_secret(&self, key: &str, id: Option<&str>)
    -> Result<Option<String>, DomainError>;

    /// 删除单个密钥（按 ID 或当前 active）
    async fn delete_secret(&self, key: &str, id: Option<&str>) -> Result<(), DomainError>;

    /// 旋转 active 密钥
    async fn rotate_secret(&self, key: &str, id: &str) -> Result<(), DomainError>;

    /// 重命名密钥
    async fn rename_secret(&self, key: &str, id: &str, label: &str) -> Result<(), DomainError>;
}
