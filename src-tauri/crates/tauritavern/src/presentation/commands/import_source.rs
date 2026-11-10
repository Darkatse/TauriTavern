//! Every command that downloads content to import checks its URL here first.

use url::Url;

use crate::app::AppState;
use crate::presentation::commands::helpers::ensure_ios_policy_allows;
use crate::presentation::errors::CommandError;
use tt_application::services::external_import_service::{
    import_source, is_pre_approved_import_source, parse_external_import_url,
};

pub(super) fn allowed_import_url(app_state: &AppState, url: &str) -> Result<Url, CommandError> {
    ensure_ios_policy_allows(
        &app_state.ios_policy,
        app_state.ios_policy.capabilities.content.external_import,
        "content.external_import",
    )?;
    let url = parse_external_import_url(url)?;
    if !is_pre_approved_import_source(&url) {
        return Err(CommandError::NotFound(format!(
            "Import source is not allowed: {}",
            import_source(&url)
        )));
    }
    Ok(url)
}
