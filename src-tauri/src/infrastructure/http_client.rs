use reqwest::{Client, ClientBuilder, Error};

/// Keep a stable product token so upstream API gateways can whitelist requests.
pub const APP_USER_AGENT: &str = concat!("TauriTavern/", env!("CARGO_PKG_VERSION"));

pub fn apply_default_user_agent(builder: ClientBuilder) -> ClientBuilder {
    builder.user_agent(APP_USER_AGENT)
}

pub fn build_http_client(builder: ClientBuilder) -> Result<Client, Error> {
    apply_default_user_agent(builder).build()
}

#[cfg(test)]
mod tests {
    use super::APP_USER_AGENT;

    #[test]
    fn app_user_agent_matches_package_version() {
        assert_eq!(
            APP_USER_AGENT,
            concat!("TauriTavern/", env!("CARGO_PKG_VERSION"))
        );
    }
}
