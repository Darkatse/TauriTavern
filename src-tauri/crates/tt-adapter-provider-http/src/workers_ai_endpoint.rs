use url::Url;

use crate::endpoint_url::{append_endpoint_path, append_endpoint_segments};
use tt_domain::errors::DomainError;

const WORKERS_AI_API_BASE: &str = "https://api.cloudflare.com/client/v4/accounts";

pub(crate) fn workers_ai_models_search_url(account_id: &str) -> Result<Url, DomainError> {
    let account_url = append_endpoint_segments(WORKERS_AI_API_BASE, &[account_id])?;
    append_endpoint_path(account_url.as_str(), "ai/models/search")
}

pub(crate) fn workers_ai_run_url(account_id: &str, model: &str) -> Result<Url, DomainError> {
    let account_url = append_endpoint_segments(WORKERS_AI_API_BASE, &[account_id])?;
    append_endpoint_path(
        account_url.as_str(),
        &format!("ai/run/{}", model.trim().trim_start_matches('/')),
    )
}

#[cfg(test)]
mod tests {
    use super::{workers_ai_models_search_url, workers_ai_run_url};

    #[test]
    fn models_search_url_encodes_account_id() {
        let url = workers_ai_models_search_url("account/id").expect("build Workers AI models URL");

        assert_eq!(
            url.as_str(),
            "https://api.cloudflare.com/client/v4/accounts/account%2Fid/ai/models/search"
        );
    }

    #[test]
    fn run_url_encodes_account_id_but_preserves_model_path() {
        let url = workers_ai_run_url("account/id", "@cf/black-forest-labs/flux-1-schnell")
            .expect("build Workers AI run URL");

        assert_eq!(
            url.as_str(),
            "https://api.cloudflare.com/client/v4/accounts/account%2Fid/ai/run/@cf/black-forest-labs/flux-1-schnell"
        );
    }
}
