use std::fs;
use std::path::Path;

use tt_domain::errors::DomainError;
use tt_domain::models::settings::revision::UserSettingsRevision;
use tt_ports::repositories::settings_repository::SettingsCommitResult;

use super::FileSettingsRepository;
use super::lazy_object::LazyObject;
use super::sections::{self, StoredSections};

impl FileSettingsRepository {
    pub async fn cleanup_orphaned_commit_staging(&self) {
        self.commit_sessions.cleanup_orphans().await;
    }
}

/// The service serializes settings saves and restores. The stored section texts are read once:
/// for the conflict check against the revision the page loaded, and to write only what changed.
pub(super) fn publish(
    root: &Path,
    received: &Path,
    expected: Option<&UserSettingsRevision>,
) -> Result<SettingsCommitResult, DomainError> {
    let stored = StoredSections::read(root)?;
    if let Some(expected) = expected
        && &stored.revision() != expected
    {
        return Err(DomainError::Conflict(
            "Settings revision mismatch. Reload settings before saving again.".into(),
        ));
    }
    let text = fs::read(received).map_err(|error| {
        DomainError::InternalError(format!("Failed to read settings commit: {error}"))
    })?;
    let mut settings = LazyObject::parse(&text).map_err(|error| {
        DomainError::InvalidData(format!("Invalid settings replacement: {error}"))
    })?;
    drop(text);
    let personas = sections::take_personas(&mut settings)?.unwrap_or_default();
    let revision = sections::write_user(root, settings, stored)?;
    Ok(SettingsCommitResult { revision, personas })
}
