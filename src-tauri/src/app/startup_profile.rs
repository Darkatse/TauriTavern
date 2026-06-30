use std::path::Path;

use crate::domain::errors::DomainError;
use crate::domain::ios_policy::{
    IosPolicyActivationReport, IosPolicyScope, resolve_ios_policy_activation_report,
};
use crate::domain::models::settings::TauriTavernSettings;
use crate::infrastructure::ios_policy_cache::resolve_effective_raw_policy_sync;
use crate::infrastructure::repositories::file_settings_repository::FileSettingsRepository;

#[derive(Debug, Clone)]
pub(crate) struct StartupProfile {
    pub tauritavern_settings: TauriTavernSettings,
    pub ios_policy: IosPolicyActivationReport,
}

impl StartupProfile {
    pub(crate) fn load(data_root: &Path) -> Result<Self, DomainError> {
        let settings_repository = FileSettingsRepository::new(data_root.join("default-user"));
        let tauritavern_settings = settings_repository.load_tauritavern_settings_sync()?;
        let scope = IosPolicyScope::for_current_platform();
        let raw_policy = if scope == IosPolicyScope::Ios {
            resolve_effective_raw_policy_sync(data_root, tauritavern_settings.ios_policy.as_ref())?
        } else {
            None
        };
        let ios_policy = resolve_ios_policy_activation_report(scope, raw_policy.as_ref())?;

        Ok(Self {
            tauritavern_settings,
            ios_policy,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "tauritavern-startup-profile-test-{}-{}",
                std::process::id(),
                suffix
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
            profile.ios_policy.scope,
            IosPolicyScope::for_current_platform()
        );
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
