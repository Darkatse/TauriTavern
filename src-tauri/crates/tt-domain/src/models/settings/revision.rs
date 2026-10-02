use std::io::{self, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::UserSettings;
use crate::errors::DomainError;

/// Hash the canonical serde_json Value serialization; Persona cards are excluded.
const USER_SETTINGS_HASH_ALGORITHM: &str = "tt-user-settings-stable-sha256-v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserSettingsRevision {
    pub hash_algorithm: String,
    pub settings_hash: String,
}

impl UserSettingsRevision {
    /// Call after normalization, before adding the independent Persona projection.
    pub fn from_settings(settings: &UserSettings) -> Result<Self, DomainError> {
        Ok(Self {
            hash_algorithm: USER_SETTINGS_HASH_ALGORITHM.into(),
            settings_hash: stable_settings_hash(&settings.data)?,
        })
    }
}

fn stable_settings_hash(value: &Value) -> Result<String, DomainError> {
    struct HashWriter(Sha256);
    impl Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = HashWriter(Sha256::new());
    serde_json::to_writer(&mut writer, value)
        .map_err(|error| DomainError::InvalidData(format!("Failed to hash settings: {error}")))?;
    Ok(writer
        .0
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{UserSettings, UserSettingsRevision};
    use serde_json::json;

    #[test]
    fn revision_preserves_v1_unicode_key_order() {
        let settings = UserSettings {
            data: json!({"b": 1, "a": 2, "\u{10000}": 3, "\u{e000}": 4}),
        };
        let revision = UserSettingsRevision::from_settings(&settings).unwrap();
        assert_eq!(
            revision.settings_hash,
            "b80f11d0d2b9f8a24fa66a8d485776a5def012c48b9d3da47d626f42c199569a"
        );
    }
}
