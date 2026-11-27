use url::Url;

use crate::errors::DomainError;
use crate::models::endpoint_url::{append_endpoint_path, append_endpoint_segments};

const ACCOUNTS_API_BASE: &str = "https://api.cloudflare.com/client/v4/accounts";

/// A Cloudflare account ID; URLs carry it as one encoded path segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkersAiAccount(String);

impl WorkersAiAccount {
    pub fn parse(account_id: &str) -> Result<Self, DomainError> {
        let account_id = account_id.trim();
        if account_id.is_empty() {
            return Err(DomainError::InvalidData(
                "workers_ai_account_id is required".to_string(),
            ));
        }
        Ok(Self(account_id.to_string()))
    }

    /// `path` below the account's Workers AI REST root (`.../accounts/{id}/ai`).
    pub fn ai_url(&self, path: &str) -> Result<Url, DomainError> {
        let ai_root = append_endpoint_segments(ACCOUNTS_API_BASE, &[&self.0, "ai"])?;
        append_endpoint_path(ai_root.as_str(), path)
    }

    pub fn openai_base_url(&self) -> Result<Url, DomainError> {
        self.ai_url("v1")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_id_stays_one_path_segment() {
        let account = WorkersAiAccount::parse("account/id").unwrap();

        assert_eq!(
            account.ai_url("models/search").unwrap().as_str(),
            "https://api.cloudflare.com/client/v4/accounts/account%2Fid/ai/models/search"
        );
    }

    #[test]
    fn model_paths_keep_their_segments() {
        let account = WorkersAiAccount::parse("account").unwrap();

        assert_eq!(
            account
                .ai_url("run/@cf/black-forest-labs/flux-1-schnell")
                .unwrap()
                .as_str(),
            "https://api.cloudflare.com/client/v4/accounts/account/ai/run/@cf/black-forest-labs/flux-1-schnell"
        );
    }
}
