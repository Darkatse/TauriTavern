use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use tt_domain::errors::DomainError;
use tt_domain::models::persona::take_personas;
use tt_domain::models::settings::UserSettings;
use tt_domain::models::settings::revision::UserSettingsRevision;
use tt_ports::repositories::settings_repository::SettingsCommitResult;

use super::{FileSettingsRepository, sections};

impl FileSettingsRepository {
    pub async fn cleanup_orphaned_commit_staging(&self) {
        self.commit_sessions.cleanup_orphans().await;
    }
}

/// The service serializes settings saves and restores. CAS uses the same normalized
/// read as the settings API, and releases it before decoding the replacement.
pub(super) fn publish(
    root: &Path,
    defaults: &UserSettings,
    received: &Path,
    expected: Option<&UserSettingsRevision>,
) -> Result<SettingsCommitResult, DomainError> {
    if let Some(expected) = expected {
        let current = sections::load_user(root, defaults)?;
        let current_revision = UserSettingsRevision::from_settings(&current)?;
        if &current_revision != expected {
            return Err(DomainError::Conflict(
                "Settings revision mismatch. Reload settings before saving again.".into(),
            ));
        }
    }
    let file = File::open(received).map_err(|error| {
        DomainError::InternalError(format!("Failed to open settings commit: {error}"))
    })?;
    let mut settings: UserSettings =
        serde_json::from_reader(BufReader::new(file)).map_err(|error| {
            DomainError::InvalidData(format!("Invalid settings replacement: {error}"))
        })?;
    let personas = take_personas(&mut settings.data)?.unwrap_or_default();
    let revision = sections::save_user(root, settings)?;
    Ok(SettingsCommitResult { revision, personas })
}
