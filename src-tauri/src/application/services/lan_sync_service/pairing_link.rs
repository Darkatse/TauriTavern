use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ttsync_contract::peer::{DeviceId, Permissions};
use url::Url;

use crate::domain::errors::DomainError;

pub(super) fn build_pair_uri(
    base_url: &str,
    token: &str,
    expires_at_ms: u64,
    spki_sha256: &str,
) -> Result<String, DomainError> {
    validate_https_base_url(base_url)?;

    let mut uri = Url::parse("tauritavern://lan-sync/pair")
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    uri.query_pairs_mut()
        .append_pair("v", "2")
        .append_pair("url", base_url)
        .append_pair("token", token)
        .append_pair("exp", &expires_at_ms.to_string())
        .append_pair("spki", spki_sha256);

    Ok(uri.to_string())
}

pub(super) struct ParsedPairUri {
    pub base_url: String,
    pub token: String,
    pub expires_at_ms: u64,
    pub spki_sha256: String,
}

pub(super) fn parse_pair_uri(pair_uri: &str) -> Result<ParsedPairUri, DomainError> {
    let uri = Url::parse(pair_uri).map_err(|error| DomainError::InvalidData(error.to_string()))?;
    if uri.scheme() != "tauritavern" || uri.host_str() != Some("lan-sync") || uri.path() != "/pair"
    {
        return Err(DomainError::InvalidData(
            "Pair URI is not a LAN Sync pairing link".to_string(),
        ));
    }

    let version = uri
        .query_pairs()
        .find_map(|(key, value)| (key == "v").then(|| value.to_string()));
    if version.as_deref() != Some("2") {
        return Err(DomainError::InvalidData(
            "LAN Sync Pair URI must be v=2".to_string(),
        ));
    }

    parse_lan_pair_uri_payload(&uri)
}

fn parse_lan_pair_uri_payload(uri: &Url) -> Result<ParsedPairUri, DomainError> {
    let mut base_url = None;
    let mut token = None;
    let mut expires_at_ms = None;
    let mut spki_sha256 = None;
    for (key, value) in uri.query_pairs() {
        match key.as_ref() {
            "url" => base_url = Some(value.to_string()),
            "token" => token = Some(value.to_string()),
            "exp" => {
                expires_at_ms = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| DomainError::InvalidData("Invalid exp".to_string()))?,
                )
            }
            "spki" => spki_sha256 = Some(value.to_string()),
            _ => {}
        }
    }

    let base_url = base_url.ok_or_else(|| DomainError::InvalidData("Missing url".to_string()))?;
    validate_https_base_url(&base_url)?;

    Ok(ParsedPairUri {
        base_url,
        token: token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| DomainError::InvalidData("Missing token".to_string()))?,
        expires_at_ms: expires_at_ms
            .ok_or_else(|| DomainError::InvalidData("Missing exp".to_string()))?,
        spki_sha256: spki_sha256
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| DomainError::InvalidData("Missing spki".to_string()))?,
    })
}

pub(super) fn validate_https_base_url(value: &str) -> Result<(), DomainError> {
    let parsed = Url::parse(value).map_err(|error| DomainError::InvalidData(error.to_string()))?;
    if parsed.scheme() != "https" {
        return Err(DomainError::InvalidData(
            "LAN Sync base URL must use https".to_string(),
        ));
    }
    if parsed.host_str().is_none() {
        return Err(DomainError::InvalidData(
            "LAN Sync base URL is missing host".to_string(),
        ));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(DomainError::InvalidData(
            "LAN Sync base URL must not include credentials".to_string(),
        ));
    }
    if !matches!(parsed.path(), "" | "/") || parsed.query().is_some() || parsed.fragment().is_some()
    {
        return Err(DomainError::InvalidData(
            "LAN Sync base URL must be an origin".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn host_for_pairing_prompt(base_url: &str) -> Result<String, DomainError> {
    let parsed =
        Url::parse(base_url).map_err(|error| DomainError::InvalidData(error.to_string()))?;
    parsed
        .host_str()
        .map(str::to_string)
        .ok_or_else(|| DomainError::InvalidData("LAN Sync base URL is missing host".to_string()))
}

pub(super) fn default_lan_permissions() -> Permissions {
    Permissions {
        read: true,
        write: false,
        mirror_delete: true,
    }
}

pub(super) fn decode_device_pubkey_b64url(value: &str) -> Result<Vec<u8>, DomainError> {
    let public_key = URL_SAFE_NO_PAD
        .decode(value.as_bytes())
        .map_err(|error| DomainError::InvalidData(error.to_string()))?;
    if public_key.len() != 32 {
        return Err(DomainError::InvalidData(
            "LAN Sync device public key must be 32 bytes".to_string(),
        ));
    }

    Ok(public_key)
}

pub(super) fn device_pubkey_b64url(seed: &str) -> Result<String, DomainError> {
    ttsync_core::crypto::device_pubkey_b64url(seed)
        .map_err(|error| DomainError::InvalidData(error.to_string()))
}

pub(super) fn parse_device_id(device_id: &str) -> Result<DeviceId, DomainError> {
    DeviceId::new(device_id.to_string())
        .map_err(|error| DomainError::InvalidData(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_uri_round_trips_required_fields() {
        let uri = build_pair_uri("https://127.0.0.1:50000", "token", 1234, "spki")
            .expect("build pair uri");

        let parsed = parse_pair_uri(&uri).expect("parse pair uri");

        assert_eq!(parsed.base_url, "https://127.0.0.1:50000");
        assert_eq!(parsed.token, "token");
        assert_eq!(parsed.expires_at_ms, 1234);
        assert_eq!(parsed.spki_sha256, "spki");
    }

    #[test]
    fn pair_uri_rejects_http_base_url() {
        assert!(matches!(
            build_pair_uri("http://127.0.0.1:50000", "token", 1234, "spki"),
            Err(DomainError::InvalidData(_))
        ));
    }

    #[test]
    fn base_url_rejects_non_origin_values() {
        for value in [
            "https://127.0.0.1:50000/path",
            "https://127.0.0.1:50000?x=1",
            "https://127.0.0.1:50000#fragment",
            "https://user@127.0.0.1:50000",
        ] {
            assert!(matches!(
                validate_https_base_url(value),
                Err(DomainError::InvalidData(_))
            ));
        }
    }

    #[test]
    fn pair_uri_rejects_legacy_version() {
        assert!(matches!(
            parse_pair_uri(
                "tauritavern://lan-sync/pair?v=1&addr=http%3A%2F%2F127.0.0.1%3A50000&pair_code=x"
            ),
            Err(DomainError::InvalidData(_))
        ));
    }

    #[test]
    fn device_pubkey_requires_32_bytes() {
        let encoded = URL_SAFE_NO_PAD.encode([7u8; 32]);
        assert_eq!(
            decode_device_pubkey_b64url(&encoded).unwrap(),
            vec![7u8; 32]
        );

        let short = URL_SAFE_NO_PAD.encode([7u8; 31]);
        assert!(matches!(
            decode_device_pubkey_b64url(&short),
            Err(DomainError::InvalidData(_))
        ));
    }

    #[test]
    fn default_permissions_allow_read_and_mirror_delete_only() {
        let permissions = default_lan_permissions();
        assert!(permissions.read);
        assert!(permissions.mirror_delete);
        assert!(!permissions.write);
    }
}
