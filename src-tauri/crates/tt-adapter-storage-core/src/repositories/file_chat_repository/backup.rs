use std::path::Path;

use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Local};
use tokio::fs;
use tt_domain::errors::DomainError;
use tt_domain::models::settings::ChatBackupSettings;
use tt_ports::settings::ChatBackupRuntime;

use super::FileChatRepository;
use super::backup_inventory::{
    BackupCandidate, BackupEntry, BackupHistoryState, BackupInventory, BackupInventoryState,
    parsed_backup_prefix, plan_evictions,
};
use super::summary::ChatFileDescriptor;

enum BackupPublishOutcome {
    Created {
        byte_len: u64,
        digest_available: bool,
    },
    DuplicateSkipped {
        byte_len: u64,
    },
}

impl FileChatRepository {
    /// The caller must hold the source payload's snapshot lock.
    pub(super) async fn backup_chat_file_automatic(
        &self,
        chat_path: &Path,
        backup_name: &str,
    ) -> Result<(), DomainError> {
        let policy = match self.backup_policy.try_read() {
            Ok(policy) => *policy,
            Err(_) => {
                return Err(DomainError::transient(
                    "Chat backup policy is being updated",
                ));
            }
        };
        if !policy.automatic_enabled || policy.history_disabled() {
            return Ok(());
        }

        let Ok(mut state) = self.backup_history.try_lock() else {
            return Err(DomainError::transient("Chat backup history is busy"));
        };
        let inventory = match &mut state.inventory {
            BackupInventoryState::Ready(inventory) => inventory,
            BackupInventoryState::Uninitialized => {
                return Err(DomainError::transient(
                    "Chat backup inventory is initializing",
                ));
            }
            BackupInventoryState::Failed(message) => {
                return Err(DomainError::InternalError(format!(
                    "Chat backup inventory is unavailable: {}",
                    message
                )));
            }
        };

        match self
            .publish_chat_backup(chat_path, backup_name, policy, inventory, true)
            .await
        {
            Ok(BackupPublishOutcome::Created {
                byte_len,
                digest_available,
            }) => {
                tracing::info!(
                    source = %chat_path.display(),
                    outcome = "created",
                    snapshot_bytes = byte_len,
                    digest_available,
                    "Created automatic chat backup"
                );
                Ok(())
            }
            Ok(BackupPublishOutcome::DuplicateSkipped { byte_len }) => {
                tracing::info!(
                    source = %chat_path.display(),
                    outcome = "duplicate_skipped",
                    snapshot_bytes = byte_len,
                    avoided_bytes = byte_len,
                    "Skipped duplicate automatic chat backup"
                );
                Ok(())
            }
            Err(DomainError::Conflict(message)) => {
                tracing::warn!(reason = %message, "Skipping automatic chat backup");
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// The caller must hold the source payload's snapshot lock.
    pub(super) async fn backup_chat_file_explicit(
        &self,
        chat_path: &Path,
        backup_name: &str,
    ) -> Result<(), DomainError> {
        let mut state = self.backup_history.lock().await;
        self.ensure_backup_inventory_ready(&mut state).await?;
        let policy = *self.backup_policy.read().await;
        let inventory = ready_inventory_mut(&mut state.inventory)?;
        self.publish_chat_backup(chat_path, backup_name, policy, inventory, false)
            .await
            .map(|_| ())
    }

    async fn publish_chat_backup(
        &self,
        chat_path: &Path,
        backup_name: &str,
        policy: ChatBackupSettings,
        inventory: &mut BackupInventory,
        suppress_duplicates: bool,
    ) -> Result<BackupPublishOutcome, DomainError> {
        policy
            .validate()
            .map_err(|error| DomainError::InvalidData(error.message()))?;
        let source_metadata = fs::metadata(chat_path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read chat file metadata before backup {:?}: {}",
                chat_path, error
            ))
        })?;
        if !source_metadata.is_file() {
            return Err(DomainError::InvalidData(format!(
                "Chat backup source is not a file: {}",
                chat_path.display()
            )));
        }

        let prefix = Self::backup_file_prefix(backup_name);
        let content_signature = self
            .current_content_signature_for_size(chat_path, source_metadata.len())
            .await;
        if suppress_duplicates
            && let Some(content_signature) = content_signature
            && inventory
                .latest_for_prefix(&prefix)
                .is_some_and(|entry| entry.content_signature == Some(content_signature))
        {
            return Ok(BackupPublishOutcome::DuplicateSkipped {
                byte_len: source_metadata.len(),
            });
        }

        let file_name = self.next_available_backup_file_name(backup_name).await?;
        let evictions = plan_evictions(
            inventory,
            policy,
            Some(BackupCandidate {
                prefix: &prefix,
                byte_len: source_metadata.len(),
            }),
        )?;
        self.delete_inventory_entries(inventory, &evictions).await?;

        let final_path = self.backups_dir.join(&file_name);
        let temp_path = self.backup_temp_path();
        let copied_bytes = match fs::copy(chat_path, &temp_path).await {
            Ok(copied_bytes) => copied_bytes,
            Err(error) => {
                let _ = fs::remove_file(&temp_path).await;
                return Err(DomainError::InternalError(format!(
                    "Failed to copy chat backup to staging: {}",
                    error
                )));
            }
        };
        if copied_bytes != source_metadata.len() {
            let _ = fs::remove_file(&temp_path).await;
            return Err(DomainError::InternalError(format!(
                "Chat backup copy length mismatch: expected {}, copied {}",
                source_metadata.len(),
                copied_bytes
            )));
        }

        let staged_metadata = match fs::metadata(&temp_path).await {
            Ok(metadata) => metadata,
            Err(error) => {
                let _ = fs::remove_file(&temp_path).await;
                return Err(DomainError::InternalError(format!(
                    "Failed to verify staged chat backup {:?}: {}",
                    temp_path, error
                )));
            }
        };
        if staged_metadata.len() != source_metadata.len() {
            let _ = fs::remove_file(&temp_path).await;
            return Err(DomainError::InternalError(format!(
                "Staged chat backup length mismatch: expected {}, found {}",
                source_metadata.len(),
                staged_metadata.len()
            )));
        }
        let modified = match staged_metadata.modified() {
            Ok(modified) => modified,
            Err(error) => {
                let _ = fs::remove_file(&temp_path).await;
                return Err(DomainError::InternalError(format!(
                    "Failed to read staged chat backup modification time {:?}: {}",
                    temp_path, error
                )));
            }
        };

        if let Err(error) = fs::rename(&temp_path, &final_path).await {
            let _ = fs::remove_file(&temp_path).await;
            return Err(DomainError::InternalError(format!(
                "Failed to publish chat backup {:?}: {}",
                final_path, error
            )));
        }

        inventory.insert(BackupEntry {
            parsed_prefix: parsed_backup_prefix(&file_name),
            file_name,
            modified,
            byte_len: copied_bytes,
            content_signature,
        })?;
        Ok(BackupPublishOutcome::Created {
            byte_len: copied_bytes,
            digest_available: content_signature.is_some(),
        })
    }

    pub(super) async fn invalidate_content_provenance(&self) {
        let mut history = self.backup_history.lock().await;
        let mut signatures = self.current_content_signatures.lock().await;

        signatures.invalidate_all();
        if let BackupInventoryState::Ready(inventory) = &mut history.inventory {
            for entry in &mut inventory.entries {
                entry.content_signature = None;
            }
        }
    }

    async fn next_available_backup_file_name(
        &self,
        backup_name: &str,
    ) -> Result<String, DomainError> {
        let mut at = Local::now();
        loop {
            let file_name = Self::backup_file_name_at(backup_name, at);
            let path = self.backups_dir.join(&file_name);
            if !fs::try_exists(&path).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to check chat backup path {:?}: {}",
                    path, error
                ))
            })? {
                return Ok(file_name);
            }
            at += ChronoDuration::seconds(1);
        }
    }

    async fn delete_inventory_entries(
        &self,
        inventory: &mut BackupInventory,
        file_names: &[String],
    ) -> Result<(), DomainError> {
        for file_name in file_names {
            let path = self.backups_dir.join(file_name);
            match fs::remove_file(&path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(DomainError::InternalError(format!(
                        "Failed to remove old chat backup {:?}: {}",
                        path, error
                    )));
                }
            }
            inventory.remove(file_name);
            self.remove_summary_cache_for_path(&path).await;
        }
        Ok(())
    }

    async fn ensure_backup_inventory_ready(
        &self,
        state: &mut BackupHistoryState,
    ) -> Result<(), DomainError> {
        if matches!(state.inventory, BackupInventoryState::Ready(_)) {
            return Ok(());
        }

        self.rebuild_backup_inventory(state).await
    }

    async fn prune_backup_inventory(
        &self,
        inventory: &mut BackupInventory,
    ) -> Result<(usize, u64), DomainError> {
        let mut evicted_files = 0;
        let mut evicted_bytes = 0;

        loop {
            let policy = *self.backup_policy.read().await;
            let evictions = plan_evictions(inventory, policy, None)?;
            if evictions.is_empty() {
                return Ok((evicted_files, evicted_bytes));
            }

            for file_name in evictions {
                // A waiting writer gets priority when this guard is dropped, so a settings
                // update waits for at most one filesystem deletion rather than the whole sweep.
                let current_policy = self.backup_policy.read().await;
                if *current_policy != policy {
                    break;
                }
                let byte_len = inventory
                    .entries
                    .iter()
                    .find(|entry| entry.file_name == file_name)
                    .map_or(0, |entry| entry.byte_len);
                self.delete_inventory_entries(inventory, std::slice::from_ref(&file_name))
                    .await?;
                evicted_files += 1;
                evicted_bytes += byte_len;
            }
        }
    }

    async fn rebuild_backup_inventory(
        &self,
        state: &mut BackupHistoryState,
    ) -> Result<(), DomainError> {
        let result = async {
            let mut inventory = self.scan_backup_inventory().await?;
            let before_files = inventory.entries.len();
            let before_bytes = inventory.total_bytes;
            let (evicted_files, evicted_bytes) =
                self.prune_backup_inventory(&mut inventory).await?;
            if evicted_files > 0 {
                tracing::info!(
                    before_files,
                    before_bytes,
                    after_files = inventory.entries.len(),
                    after_bytes = inventory.total_bytes,
                    evicted_files,
                    evicted_bytes,
                    "Reconciled chat backup history limits"
                );
            }
            Ok::<_, DomainError>(inventory)
        }
        .await;

        match result {
            Ok(inventory) => {
                state.inventory = BackupInventoryState::Ready(inventory);
                Ok(())
            }
            Err(error) => {
                state.inventory = BackupInventoryState::Failed(error.to_string());
                Err(error)
            }
        }
    }

    pub(super) async fn list_chat_backup_files(
        &self,
    ) -> Result<Vec<ChatFileDescriptor>, DomainError> {
        let mut state = self.backup_history.lock().await;
        self.ensure_backup_inventory_ready(&mut state).await?;
        let inventory = ready_inventory(&state.inventory)?;
        Ok(inventory
            .entries
            .iter()
            .map(|entry| ChatFileDescriptor {
                character_name: String::new(),
                file_name: entry.file_name.clone(),
                path: self.backups_dir.join(&entry.file_name),
            })
            .collect())
    }

    pub(super) async fn delete_chat_backup_from_inventory(
        &self,
        backup_file_name: &str,
    ) -> Result<(), DomainError> {
        let file_name = Self::normalize_backup_file_name(backup_file_name)?;
        let path = self.backups_dir.join(&file_name);

        let mut state = self.backup_history.lock().await;
        self.ensure_backup_inventory_ready(&mut state).await?;
        let inventory = ready_inventory_mut(&mut state.inventory)?;
        if !inventory.contains(&file_name) {
            return Err(DomainError::NotFound(format!(
                "Chat backup not found: {}",
                backup_file_name
            )));
        }

        match fs::remove_file(&path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to delete chat backup file: {}",
                    error
                )));
            }
        }
        inventory.remove(&file_name);
        drop(state);

        self.remove_summary_cache_for_path(&path).await;
        self.flush_summary_index_if_needed().await
    }
}

fn ready_inventory(state: &BackupInventoryState) -> Result<&BackupInventory, DomainError> {
    match state {
        BackupInventoryState::Ready(inventory) => Ok(inventory),
        BackupInventoryState::Uninitialized => Err(DomainError::InternalError(
            "Chat backup inventory is not initialized".to_string(),
        )),
        BackupInventoryState::Failed(message) => Err(DomainError::InternalError(format!(
            "Chat backup inventory is unavailable: {}",
            message
        ))),
    }
}

fn ready_inventory_mut(
    state: &mut BackupInventoryState,
) -> Result<&mut BackupInventory, DomainError> {
    match state {
        BackupInventoryState::Ready(inventory) => Ok(inventory),
        BackupInventoryState::Uninitialized => Err(DomainError::InternalError(
            "Chat backup inventory is not initialized".to_string(),
        )),
        BackupInventoryState::Failed(message) => Err(DomainError::InternalError(format!(
            "Chat backup inventory is unavailable: {}",
            message
        ))),
    }
}

#[async_trait]
impl ChatBackupRuntime for FileChatRepository {
    async fn apply_chat_backup_settings(
        &self,
        settings: ChatBackupSettings,
    ) -> Result<(), DomainError> {
        settings
            .validate()
            .map_err(|error| DomainError::InvalidData(error.message()))?;
        *self.backup_policy.write().await = settings;
        Ok(())
    }

    async fn reconcile_chat_backups(&self) -> Result<(), DomainError> {
        let mut state = self.backup_history.lock().await;
        state.inventory = BackupInventoryState::Uninitialized;
        self.rebuild_backup_inventory(&mut state).await
    }
}
