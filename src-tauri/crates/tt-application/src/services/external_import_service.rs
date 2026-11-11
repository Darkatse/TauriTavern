pub use tt_ports::external_import::*;

use std::sync::Arc;

use async_trait::async_trait;
use tt_domain::errors::DomainError;
use tt_ports::repositories::grant_repository::GrantRepository;
use url::Url;

use crate::services::user_grants::{GrantList, UserGrants};

/// Origins imports may download from without asking.
const PRE_APPROVED_IMPORT_SOURCES: &[&str] = &[
    "https://cdn.discordapp.com",
    "https://files.catbox.moe",
    "https://raw.githubusercontent.com",
    "https://botbooru.com",
    "https://api.chub.ai",
    "https://avatars.charhub.io",
    "https://realm.risuai.net",
    "https://server.pygmalion.chat",
    "https://assets.pygmalion.chat",
];

/// An import source is an origin: scheme, host and port.
pub fn import_source(url: &Url) -> String {
    url.origin().ascii_serialization()
}

pub(crate) struct AnySource;

#[async_trait]
impl DownloadSources for AnySource {
    async fn allow(&self, _url: &Url) -> Result<(), DomainError> {
        Ok(())
    }
}

/// Import downloads may request pre-approved sources and sources the user confirmed.
pub struct ImportSourceAccessService {
    grants: GrantList,
}

impl ImportSourceAccessService {
    pub async fn initialize(repository: Arc<dyn GrantRepository>) -> Self {
        Self {
            grants: GrantList::load("import source", repository).await,
        }
    }
}

/// Subjects are [`import_source`]s.
#[async_trait]
impl UserGrants for ImportSourceAccessService {
    async fn is_allowed(&self, source: &str) -> bool {
        PRE_APPROVED_IMPORT_SOURCES.contains(&source) || self.grants.contains(source).await
    }

    async fn grant(&self, source: &str) {
        self.grants.grant(source).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::user_grants::tests::MemoryGrantRepository;

    fn url(raw: &str) -> Url {
        parse_external_import_url(raw).expect("valid import URL")
    }

    #[tokio::test]
    async fn import_sources_match_by_origin() {
        let access =
            ImportSourceAccessService::initialize(Arc::new(MemoryGrantRepository::default())).await;
        let allowed = async |raw: &str| access.is_allowed(&import_source(&url(raw))).await;

        assert!(allowed("https://files.catbox.moe/abc.png").await);
        assert!(allowed("https://cdn.discordapp.com/attachments/1/2/card.png").await);
        assert!(allowed("https://cdn.discordapp.com/ephemeral/card.png").await);
        assert!(allowed("https://RAW.githubusercontent.com/owner/repo/main/card.json").await);
        assert!(allowed("https://botbooru.com:443/post/123").await);

        assert!(!allowed("https://botbooru.com.evil.example/post/123").await);
        assert!(!allowed("http://raw.githubusercontent.com/owner/repo/main/card.png").await);
        assert!(!allowed("https://files.catbox.moe:8443/abc.png").await);
        assert!(!allowed("http://localhost:8000/card.png").await);
        assert!(!allowed("http://127.0.0.1/card.png").await);
        assert!(!allowed("http://[::1]/card.png").await);

        access
            .grant(&import_source(&url("http://localhost:8000/card.png")))
            .await;

        assert!(allowed("http://localhost:8000/other.json").await);
        assert!(!allowed("http://localhost:8001/card.png").await);
        assert!(!allowed("https://localhost:8000/card.png").await);
    }
}
