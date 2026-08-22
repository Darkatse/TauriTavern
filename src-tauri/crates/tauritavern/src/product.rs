use tt_domain::models::update::UpdateChannel;

pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) const USER_AGENT: &str = concat!("TauriTavern/", env!("CARGO_PKG_VERSION"));
pub(crate) const GIT_REVISION: &str = env!("TAURITAVERN_GIT_REVISION");
pub(crate) const GIT_BRANCH: &str = env!("TAURITAVERN_GIT_BRANCH");

pub(crate) fn optional_build_value(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

pub(crate) fn default_update_channel() -> UpdateChannel {
    infer_update_channel(optional_build_value(GIT_BRANCH))
}

fn infer_update_channel(branch: Option<&str>) -> UpdateChannel {
    match branch {
        Some("main") | None => UpdateChannel::Stable,
        Some(_) => UpdateChannel::Canary,
    }
}
