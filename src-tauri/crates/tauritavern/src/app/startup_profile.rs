use std::path::Path;

use serde::{Deserialize, de::value::StrDeserializer};

use crate::infrastructure::ios_policy_cache::resolve_effective_raw_policy_sync;
use crate::platform::identity::HOST_IDENTITY;
use tt_adapter_storage_core::load_tauritavern_settings_blocking;
use tt_contracts::host::HostPlatform;
use tt_domain::errors::DomainError;
use tt_domain::ios_policy::{
    IosPolicyActivationReport, IosPolicyProfile, IosPolicyScope, ios_policy_seed,
    resolve_ios_policy_activation_report,
};
use tt_domain::models::settings::TauriTavernSettings;

#[derive(Debug, Clone)]
pub(crate) struct StartupProfile {
    pub tauritavern_settings: TauriTavernSettings,
    pub ios_policy: IosPolicyActivationReport,
}

impl StartupProfile {
    pub(crate) fn load(data_root: &Path) -> Result<Self, DomainError> {
        let tauritavern_settings = load_tauritavern_settings_blocking(
            &data_root.join("default-user"),
            &initial_tauritavern_settings(),
        )?;
        tauritavern_settings
            .chat_backups
            .validate()
            .map_err(|error| DomainError::InvalidData(error.message()))?;
        let scope = if HOST_IDENTITY.platform == HostPlatform::Ios {
            IosPolicyScope::Ios
        } else {
            IosPolicyScope::Ignored
        };
        let raw_policy = if scope == IosPolicyScope::Ios {
            resolve_effective_raw_policy_sync(data_root, tauritavern_settings.ios_policy.as_ref())?
        } else {
            None
        };
        let ios_policy = resolve_ios_policy_activation_report(
            scope,
            raw_policy.as_ref(),
            factory_ios_profile(),
        )?;

        Ok(Self {
            tauritavern_settings,
            ios_policy,
        })
    }
}

/// Native defaults shared by startup loading and repository composition.
pub(super) fn initial_tauritavern_settings() -> TauriTavernSettings {
    TauriTavernSettings {
        ios_policy: factory_ios_profile().map(ios_policy_seed),
        ..TauriTavernSettings::default()
    }
}

fn factory_ios_profile() -> Option<IosPolicyProfile> {
    match env!("TAURITAVERN_IOS_POLICY_PROFILE") {
        "" => None,
        value => Some(
            IosPolicyProfile::deserialize(StrDeserializer::<serde::de::value::Error>::new(value))
                .expect("iOS factory profile was validated by build.rs"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "tauritavern-startup-profile-test-{}-{}",
                std::process::id(),
                NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("failed to create temp dir");

            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn load_creates_default_tauritavern_settings_file() {
        let dir = TestDir::new();

        let profile = StartupProfile::load(dir.path()).expect("load startup profile");

        assert!(
            dir.path()
                .join("default-user/tauritavern-settings.json")
                .is_file()
        );
        assert_eq!(
            profile.tauritavern_settings.ios_policy,
            initial_tauritavern_settings().ios_policy
        );
    }

    #[test]
    fn load_rejects_invalid_chat_backup_limits() {
        let dir = TestDir::new();
        let default_user = dir.path().join("default-user");
        fs::create_dir_all(&default_user).expect("create default user dir");
        fs::write(
            default_user.join("tauritavern-settings.json"),
            r#"{
                "updates":{"startup_popup":{"dismissed_release_token":null}},
                "chat_backups":{
                    "automatic_enabled":true,
                    "max_files_per_prefix":20,
                    "max_total_files":500,
                    "max_total_bytes":-2
                }
            }"#,
        )
        .expect("write settings");

        assert!(matches!(
            StartupProfile::load(dir.path()),
            Err(DomainError::InvalidData(message)) if message.contains("max_total_bytes")
        ));
    }

    #[cfg(not(target_os = "ios"))]
    #[test]
    fn load_ignores_invalid_ios_policy_off_ios() {
        let dir = TestDir::new();
        let default_user = dir.path().join("default-user");
        fs::create_dir_all(&default_user).expect("create default user dir");
        fs::write(
            default_user.join("tauritavern-settings.json"),
            r#"{"updates":{"startup_popup":{"dismissed_release_token":null}},"ios_policy":{"version":"bad"}}"#,
        )
        .expect("write settings");

        let profile = StartupProfile::load(dir.path()).expect("load startup profile");

        assert_eq!(profile.ios_policy.scope, IosPolicyScope::Ignored);
    }
}
