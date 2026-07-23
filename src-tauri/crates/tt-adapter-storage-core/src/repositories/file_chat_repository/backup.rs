use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Local};
use tokio::fs;
use tt_domain::errors::DomainError;
use tt_domain::models::settings::ChatBackupSettings;
use tt_ports::settings::{ChatBackupRuntime, ChatBackupStorageStats};

use super::FileChatRepository;
use super::backup_codec::{
    BackupFormat, compress_backup, copy_decoded_backup_reader, decompress_backup,
    is_materialization_path, materialization_file_name, open_decoded_backup,
    read_zstd_frame_content_size, set_backup_modified,
};
use super::backup_inventory::{
    BackupCandidate, BackupEntry, BackupHistoryState, BackupInventory, BackupInventoryState,
    parsed_backup_prefix, plan_evictions,
};
use super::summary::ChatFileDescriptor;

enum BackupPublishOutcome {
    Created,
    DuplicateSkipped { byte_len: u64 },
}

#[derive(Default)]
struct BackupConvergenceOutcome {
    evicted_files: usize,
    evicted_bytes: u64,
    first_error: Option<DomainError>,
}

impl FileChatRepository {
    pub(super) async fn materialize_chat_backup_file(
        &self,
        backup_file_name: &str,
    ) -> Result<PathBuf, DomainError> {
        fs::create_dir_all(&self.chat_commit_staging_dir)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create chat backup materialization directory {}: {error}",
                    self.chat_commit_staging_dir.display()
                ))
            })?;
        let target_path = self
            .chat_commit_staging_dir
            .join(materialization_file_name());
        self.copy_chat_backup_to_path(backup_file_name, &target_path)
            .await?;
        Ok(target_path)
    }

    pub(super) async fn discard_chat_backup_materialization_file(
        &self,
        path: &Path,
    ) -> Result<(), DomainError> {
        if !is_materialization_path(path, &self.chat_commit_staging_dir) {
            return Err(DomainError::InvalidData(
                "Invalid chat backup materialization path".to_string(),
            ));
        }

        match fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(DomainError::InternalError(format!(
                "Failed to discard chat backup materialization {}: {error}",
                path.display()
            ))),
        }
    }

    pub(super) async fn copy_chat_backup_to_path(
        &self,
        backup_file_name: &str,
        target_path: &Path,
    ) -> Result<u64, DomainError> {
        let logical_file_name = Self::normalize_backup_file_name(backup_file_name)?;
        let mut state = self.backup_history.lock().await;
        self.ensure_backup_inventory_ready(&mut state).await?;
        let inventory = ready_inventory(&state.inventory)?;
        let entry = inventory
            .find_by_logical_name(&logical_file_name)
            .ok_or_else(|| {
                DomainError::NotFound(format!("Chat backup not found: {backup_file_name}"))
            })?;
        let source_path = self.backups_dir.join(&entry.file_name);
        let source = open_decoded_backup(&source_path, entry.format).await?;
        drop(state);
        copy_decoded_backup_reader(source, &source_path, target_path).await
    }

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
            Ok(BackupPublishOutcome::Created) => Ok(()),
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
        if policy.history_disabled() {
            return Err(DomainError::Conflict(
                "Chat backup history is disabled by its quota settings".to_string(),
            ));
        }
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

        let format = BackupFormat::from_compression_enabled(policy.zstd_compression_enabled);
        let logical_file_name = self
            .next_available_backup_file_name(backup_name, inventory)
            .await?;
        let raw_evictions = if format == BackupFormat::RawJsonl {
            Some(plan_evictions(
                inventory,
                policy,
                Some(BackupCandidate {
                    prefix: &prefix,
                    byte_len: source_metadata.len(),
                }),
            )?)
        } else {
            None
        };

        let file_name = format.physical_file_name(&logical_file_name);
        let final_path = self.backups_dir.join(&file_name);
        let temp_path = self.backup_temp_path();
        let stored_bytes = match format {
            BackupFormat::RawJsonl => fs::copy(chat_path, &temp_path).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to copy chat backup to staging: {error}"
                ))
            }),
            BackupFormat::Zstd => {
                compress_backup(chat_path, &temp_path, source_metadata.len()).await
            }
        };
        let stored_bytes = match stored_bytes {
            Ok(stored_bytes) => stored_bytes,
            Err(error) => {
                let _ = fs::remove_file(&temp_path).await;
                return Err(error);
            }
        };
        if format == BackupFormat::RawJsonl && stored_bytes != source_metadata.len() {
            let _ = fs::remove_file(&temp_path).await;
            return Err(DomainError::InternalError(format!(
                "Chat backup copy length mismatch: expected {}, copied {}",
                source_metadata.len(),
                stored_bytes
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
        if staged_metadata.len() != stored_bytes {
            let _ = fs::remove_file(&temp_path).await;
            return Err(DomainError::InternalError(format!(
                "Staged chat backup length mismatch: expected {}, found {}",
                stored_bytes,
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

        let evictions = match raw_evictions {
            Some(evictions) => evictions,
            None => match plan_evictions(
                inventory,
                policy,
                Some(BackupCandidate {
                    prefix: &prefix,
                    byte_len: stored_bytes,
                }),
            ) {
                Ok(evictions) => evictions,
                Err(error) => {
                    let _ = fs::remove_file(&temp_path).await;
                    return Err(error);
                }
            },
        };
        if let Err(error) = fs::rename(&temp_path, &final_path).await {
            let _ = fs::remove_file(&temp_path).await;
            return Err(DomainError::InternalError(format!(
                "Failed to publish chat backup {:?}: {}",
                final_path, error
            )));
        }

        inventory.insert(BackupEntry {
            logical_file_name,
            parsed_prefix: parsed_backup_prefix(&file_name),
            file_name,
            format,
            modified,
            byte_len: stored_bytes,
            content_signature,
        })?;
        self.delete_inventory_entries(inventory, &evictions).await?;
        let stored_ratio = if source_metadata.len() == 0 {
            1.0
        } else {
            stored_bytes as f64 / source_metadata.len() as f64
        };
        tracing::info!(
            source = %chat_path.display(),
            outcome = "created",
            ?format,
            source_bytes = source_metadata.len(),
            stored_bytes,
            stored_ratio,
            digest_available = content_signature.is_some(),
            "Created chat backup"
        );
        Ok(BackupPublishOutcome::Created)
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

    async fn converge_backup_inventory_format(
        &self,
        inventory: &mut BackupInventory,
    ) -> Result<BackupConvergenceOutcome, DomainError> {
        let mut outcome = BackupConvergenceOutcome::default();
        let mut failed_conversions = HashMap::new();

        loop {
            let policy = *self.backup_policy.read().await;
            if policy.history_disabled() {
                return Ok(outcome);
            }
            let target_format =
                BackupFormat::from_compression_enabled(policy.zstd_compression_enabled);
            let Some(source_entry) = inventory
                .entries
                .iter()
                .filter(|entry| entry.format != target_format)
                .filter(|entry| failed_conversions.get(&entry.file_name) != Some(&target_format))
                .max_by(|left, right| {
                    left.modified
                        .cmp(&right.modified)
                        .then_with(|| left.file_name.cmp(&right.file_name))
                })
                .cloned()
            else {
                return Ok(outcome);
            };

            let target_entry = match self
                .convert_backup_entry(&source_entry, target_format)
                .await
            {
                Ok(Some(entry)) => entry,
                Ok(None) => continue,
                Err(error) => {
                    tracing::warn!(
                        logical_name = %source_entry.logical_file_name,
                        from = ?source_entry.format,
                        to = ?target_format,
                        error = %error,
                        "Failed to convert chat backup; continuing inventory maintenance"
                    );
                    failed_conversions.insert(source_entry.file_name, target_format);
                    if outcome.first_error.is_none() {
                        outcome.first_error = Some(error);
                    }
                    continue;
                }
            };
            inventory.remove(&source_entry.file_name);
            inventory.insert(target_entry)?;
            let (files, bytes) = self.prune_backup_inventory(inventory).await?;
            outcome.evicted_files += files;
            outcome.evicted_bytes += bytes;
        }
    }

    async fn convert_backup_entry(
        &self,
        source_entry: &BackupEntry,
        target_format: BackupFormat,
    ) -> Result<Option<BackupEntry>, DomainError> {
        let source_path = self.backups_dir.join(&source_entry.file_name);
        let target_file_name = target_format.physical_file_name(&source_entry.logical_file_name);
        let target_path = self.backups_dir.join(&target_file_name);
        let temp_path = self.backup_temp_path();

        let stored_bytes = match (source_entry.format, target_format) {
            (BackupFormat::RawJsonl, BackupFormat::Zstd) => {
                compress_backup(&source_path, &temp_path, source_entry.byte_len).await
            }
            (BackupFormat::Zstd, BackupFormat::RawJsonl) => {
                decompress_backup(&source_path, &temp_path).await
            }
            _ => return Ok(None),
        }?;

        let result = async {
            let policy = self.backup_policy.read().await;
            if policy.history_disabled()
                || BackupFormat::from_compression_enabled(policy.zstd_compression_enabled)
                    != target_format
            {
                return Ok(None);
            }
            if fs::try_exists(&target_path).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to check chat backup conversion target {}: {error}",
                    target_path.display()
                ))
            })? {
                return Err(DomainError::Conflict(format!(
                    "Chat backup conversion target already exists: {}",
                    target_path.display()
                )));
            }

            set_backup_modified(&temp_path, source_entry.modified).await?;
            fs::rename(&temp_path, &target_path)
                .await
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to publish converted chat backup {}: {error}",
                        target_path.display()
                    ))
                })?;
            match fs::remove_file(&source_path).await {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(DomainError::InternalError(format!(
                        "Failed to remove converted chat backup source {}: {error}",
                        source_path.display()
                    )));
                }
            }
            drop(policy);

            self.remove_summary_cache_for_path(&source_path).await;
            self.remove_summary_cache_for_path(&target_path).await;
            tracing::info!(
                logical_name = %source_entry.logical_file_name,
                from = ?source_entry.format,
                to = ?target_format,
                source_bytes = source_entry.byte_len,
                stored_bytes,
                "Converted chat backup storage format"
            );

            Ok(Some(BackupEntry {
                logical_file_name: source_entry.logical_file_name.clone(),
                file_name: target_file_name,
                format: target_format,
                parsed_prefix: source_entry.parsed_prefix.clone(),
                modified: source_entry.modified,
                byte_len: stored_bytes,
                content_signature: source_entry.content_signature,
            }))
        }
        .await;

        if !matches!(&result, Ok(Some(_))) {
            let _ = fs::remove_file(&temp_path).await;
        }
        result
    }

    async fn next_available_backup_file_name(
        &self,
        backup_name: &str,
        inventory: &BackupInventory,
    ) -> Result<String, DomainError> {
        let mut at = Local::now();
        loop {
            let file_name = Self::backup_file_name_at(backup_name, at);
            let raw_path = self.backups_dir.join(&file_name);
            let zstd_path = self
                .backups_dir
                .join(BackupFormat::Zstd.physical_file_name(&file_name));
            let raw_exists = fs::try_exists(&raw_path).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to check chat backup path {:?}: {}",
                    raw_path, error
                ))
            })?;
            let zstd_exists = fs::try_exists(&zstd_path).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to check chat backup path {:?}: {}",
                    zstd_path, error
                ))
            })?;
            if !raw_exists && !zstd_exists && inventory.find_by_logical_name(&file_name).is_none() {
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

        match self.rebuild_backup_inventory(state).await {
            Err(error) if matches!(state.inventory, BackupInventoryState::Ready(_)) => {
                tracing::warn!(
                    error = %error,
                    "Chat backup inventory is usable but format maintenance is incomplete"
                );
                Ok(())
            }
            result => result,
        }
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
        let known_content_signatures: HashMap<_, _> = match &state.inventory {
            BackupInventoryState::Ready(inventory) => inventory
                .entries
                .iter()
                .filter_map(|entry| {
                    entry
                        .content_signature
                        .map(|signature| (entry.logical_file_name.clone(), signature))
                })
                .collect(),
            _ => HashMap::new(),
        };
        let result = async {
            let policy = *self.backup_policy.read().await;
            let target_format =
                BackupFormat::from_compression_enabled(policy.zstd_compression_enabled);
            let mut inventory = self.scan_backup_inventory(target_format).await?;
            for entry in &mut inventory.entries {
                entry.content_signature = known_content_signatures
                    .get(&entry.logical_file_name)
                    .copied();
            }
            let before_files = inventory.entries.len();
            let before_bytes = inventory.total_bytes;
            let convergence = self
                .converge_backup_inventory_format(&mut inventory)
                .await?;
            let mut evicted_files = convergence.evicted_files;
            let mut evicted_bytes = convergence.evicted_bytes;
            let (final_evicted_files, final_evicted_bytes) =
                self.prune_backup_inventory(&mut inventory).await?;
            evicted_files += final_evicted_files;
            evicted_bytes += final_evicted_bytes;
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
            Ok::<_, DomainError>((inventory, convergence.first_error))
        }
        .await;

        match result {
            Ok((inventory, maintenance_error)) => {
                state.inventory = BackupInventoryState::Ready(inventory);
                maintenance_error.map_or(Ok(()), Err)
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
                file_name: entry.logical_file_name.clone(),
                path: self.backups_dir.join(&entry.file_name),
            })
            .collect())
    }

    pub(super) async fn delete_chat_backup_from_inventory(
        &self,
        backup_file_name: &str,
    ) -> Result<(), DomainError> {
        let logical_file_name = Self::normalize_backup_file_name(backup_file_name)?;

        let mut state = self.backup_history.lock().await;
        self.ensure_backup_inventory_ready(&mut state).await?;
        let inventory = ready_inventory_mut(&mut state.inventory)?;
        let file_name = inventory
            .find_by_logical_name(&logical_file_name)
            .map(|entry| entry.file_name.clone())
            .ok_or_else(|| {
                DomainError::NotFound(format!("Chat backup not found: {backup_file_name}"))
            })?;
        let path = self.backups_dir.join(&file_name);

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
        self.rebuild_backup_inventory(&mut state).await
    }

    async fn get_chat_backup_storage_stats(
        &self,
    ) -> Result<Option<ChatBackupStorageStats>, DomainError> {
        let (file_names, stored_bytes) = {
            let Ok(state) = self.backup_history.try_lock() else {
                return Ok(None);
            };
            let Ok(policy) = self.backup_policy.try_read() else {
                return Ok(None);
            };
            if !policy.zstd_compression_enabled {
                return Ok(None);
            }
            let inventory = match &state.inventory {
                BackupInventoryState::Ready(inventory) => inventory,
                BackupInventoryState::Uninitialized => return Ok(None),
                BackupInventoryState::Failed(message) => {
                    return Err(DomainError::InternalError(format!(
                        "Chat backup inventory is unavailable: {message}"
                    )));
                }
            };
            if inventory.entries.is_empty()
                || inventory
                    .entries
                    .iter()
                    .any(|entry| entry.format != BackupFormat::Zstd)
            {
                return Ok(None);
            }
            (
                inventory
                    .entries
                    .iter()
                    .map(|entry| entry.file_name.clone())
                    .collect::<Vec<_>>(),
                inventory.total_bytes,
            )
        };

        let mut original_bytes = 0u64;
        for file_name in file_names {
            let Some(content_size) =
                read_zstd_frame_content_size(&self.backups_dir.join(file_name)).await?
            else {
                return Ok(None);
            };
            original_bytes = original_bytes.checked_add(content_size).ok_or_else(|| {
                DomainError::InvalidData("Chat backup original byte count overflowed".to_string())
            })?;
        }

        Ok(Some(ChatBackupStorageStats {
            original_bytes,
            stored_bytes,
        }))
    }
}
