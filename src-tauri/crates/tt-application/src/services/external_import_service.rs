pub use tt_ports::external_import::*;

use url::Url;

use crate::errors::ApplicationError;

/// Origins imports may download from without asking.
const PRE_APPROVED_IMPORT_SOURCES: &[&str] = &[
    "https://cdn.discordapp.com",
    "https://files.catbox.moe",
    "https://raw.githubusercontent.com",
    "https://botbooru.com",
];

pub fn parse_external_import_url(raw: &str) -> Result<Url, ApplicationError> {
    let url = Url::parse(raw.trim())
        .map_err(|_| ApplicationError::ValidationError("Invalid import URL".to_string()))?;

    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err(ApplicationError::ValidationError(
            "Unsupported URL protocol".to_string(),
        )),
    }
}

/// An import source is an origin: scheme, host and port.
pub fn import_source(url: &Url) -> String {
    url.origin().ascii_serialization()
}

pub fn is_pre_approved_import_source(url: &Url) -> bool {
    PRE_APPROVED_IMPORT_SOURCES.contains(&import_source(url).as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pre_approved(raw: &str) -> bool {
        is_pre_approved_import_source(&parse_external_import_url(raw).expect("valid import URL"))
    }

    #[test]
    fn pre_approved_sources_match_by_origin() {
        assert!(pre_approved("https://files.catbox.moe/abc.png"));
        assert!(pre_approved(
            "https://cdn.discordapp.com/attachments/1/2/card.png"
        ));
        assert!(pre_approved(
            "https://cdn.discordapp.com/ephemeral/card.png"
        ));
        assert!(pre_approved(
            "https://RAW.githubusercontent.com/owner/repo/main/card.json"
        ));
        assert!(pre_approved("https://botbooru.com:443/post/123"));

        assert!(!pre_approved("https://botbooru.com.evil.example/post/123"));
        assert!(!pre_approved(
            "http://raw.githubusercontent.com/owner/repo/main/card.png"
        ));
        assert!(!pre_approved("https://files.catbox.moe:8443/abc.png"));
        assert!(!pre_approved("http://localhost:8000/card.png"));
        assert!(!pre_approved("http://127.0.0.1/card.png"));
        assert!(!pre_approved("http://[::1]/card.png"));
    }
}
