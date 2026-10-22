use async_trait::async_trait;

use tt_domain::errors::DomainError;

/// An opened source. Reads advance this source without reopening its path.
#[async_trait]
pub trait ByteReader: Send {
    async fn read(&mut self, buffer: &mut [u8]) -> Result<usize, DomainError>;
}
