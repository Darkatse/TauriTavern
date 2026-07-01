use tt_domain::errors::DomainError;
use tt_domain::models::settings::RequestProxySettings;

pub trait RequestProxyRuntime: Send + Sync {
    fn validate_request_proxy_settings(
        &self,
        settings: &RequestProxySettings,
    ) -> Result<(), DomainError>;

    fn apply_request_proxy_settings(
        &self,
        settings: &RequestProxySettings,
    ) -> Result<(), DomainError>;
}
