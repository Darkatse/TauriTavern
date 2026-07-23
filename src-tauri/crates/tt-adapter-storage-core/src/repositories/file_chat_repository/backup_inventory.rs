use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

use chrono::NaiveDateTime;
use tokio::fs;
use tt_domain::errors::DomainError;
use tt_domain::models::settings::ChatBackupSettings;

use super::backup_codec::{BackupFormat, set_backup_modified};
use super::{ContentSignature, FileChatRepository};

pub(super) const BACKUP_TEMP_PREFIX: &str = ".tmp-chat-backup-";

#[derive(Clone, Debug)]
pub(super) struct BackupEntry {
    pub logical_file_name: String,
    pub file_name: String,
    pub format: BackupFormat,
    pub parsed_prefix: Option<String>,
    pub modified: SystemTime,
    pub byte_len: u64,
    pub content_signature: Option<ContentSignature>,
}

#[derive(Debug, Default)]
pub(super) struct BackupInventory {
    pub entries: Vec<BackupEntry>,
    pub total_bytes: u64,
}

impl BackupInventory {
    pub fn insert(&mut self, entry: BackupEntry) -> Result<(), DomainError> {
        self.total_bytes = self
            .total_bytes
            .checked_add(entry.byte_len)
            .ok_or_else(|| {
                DomainError::InternalError("Chat backup inventory byte count overflowed".into())
            })?;
        self.entries.push(entry);
        Ok(())
    }

    pub fn remove(&mut self, file_name: &str) -> Option<BackupEntry> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.file_name == file_name)?;
        let entry = self.entries.swap_remove(index);
        self.total_bytes -= entry.byte_len;
        Some(entry)
    }

    pub fn find_by_logical_name(&self, logical_file_name: &str) -> Option<&BackupEntry> {
        self.entries
            .iter()
            .find(|entry| entry.logical_file_name == logical_file_name)
    }

    pub fn latest_for_prefix(&self, prefix: &str) -> Option<&BackupEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.parsed_prefix.as_deref() == Some(prefix))
            .max_by(|left, right| {
                left.modified
                    .cmp(&right.modified)
                    .then_with(|| left.file_name.cmp(&right.file_name))
            })
    }
}

#[derive(Debug)]
pub(super) enum BackupInventoryState {
    Uninitialized,
    Ready(BackupInventory),
    Failed(String),
}

pub(super) struct BackupHistoryState {
    pub inventory: BackupInventoryState,
}

impl BackupHistoryState {
    pub fn new() -> Self {
        Self {
            inventory: BackupInventoryState::Uninitialized,
        }
    }
}

pub(super) struct BackupCandidate<'a> {
    pub prefix: &'a str,
    pub byte_len: u64,
}

pub(super) fn plan_evictions(
    inventory: &BackupInventory,
    policy: ChatBackupSettings,
    candidate: Option<BackupCandidate<'_>>,
) -> Result<Vec<String>, DomainError> {
    policy
        .validate()
        .map_err(|error| DomainError::InvalidData(error.message()))?;

    if policy.history_disabled() {
        if candidate.is_some() {
            return Err(DomainError::Conflict(
                "Chat backup history is disabled by its quota settings".to_string(),
            ));
        }

        return Ok(oldest_entries(inventory)
            .into_iter()
            .map(|entry| entry.file_name.clone())
            .collect());
    }

    if let Some(candidate) = candidate.as_ref()
        && policy.max_total_bytes > 0
        && candidate.byte_len > policy.max_total_bytes as u64
    {
        return Err(DomainError::Conflict(format!(
            "Chat backup is {} bytes, exceeding the {} byte history limit",
            candidate.byte_len, policy.max_total_bytes
        )));
    }

    let oldest = oldest_entries(inventory);
    let mut selected = HashSet::new();

    if policy.max_files_per_prefix > 0 {
        let limit = policy.max_files_per_prefix as u64;
        let mut counts: HashMap<&str, u64> = HashMap::new();
        for entry in &inventory.entries {
            if let Some(prefix) = entry.parsed_prefix.as_deref() {
                *counts.entry(prefix).or_default() += 1;
            }
        }
        if let Some(candidate) = candidate.as_ref() {
            *counts.entry(candidate.prefix).or_default() += 1;
        }

        for (prefix, count) in counts {
            let mut excess = count.saturating_sub(limit);
            if excess == 0 {
                continue;
            }

            for entry in &oldest {
                if excess == 0 {
                    break;
                }
                if entry.parsed_prefix.as_deref() == Some(prefix)
                    && selected.insert(entry.file_name.as_str())
                {
                    excess -= 1;
                }
            }
        }
    }

    if policy.max_total_files > 0 {
        let candidate_files = u64::from(candidate.is_some());
        let mut retained_files = (inventory.entries.len() - selected.len()) as u64;
        retained_files = retained_files.checked_add(candidate_files).ok_or_else(|| {
            DomainError::InternalError("Chat backup file count overflowed".into())
        })?;
        let limit = policy.max_total_files as u64;

        for entry in &oldest {
            if retained_files <= limit {
                break;
            }
            if selected.insert(entry.file_name.as_str()) {
                retained_files -= 1;
            }
        }
    }

    if policy.max_total_bytes > 0 {
        let candidate_bytes = candidate.as_ref().map_or(0, |value| value.byte_len);
        let selected_bytes: u64 = inventory
            .entries
            .iter()
            .filter(|entry| selected.contains(entry.file_name.as_str()))
            .map(|entry| entry.byte_len)
            .sum();
        let mut retained_bytes = inventory
            .total_bytes
            .checked_sub(selected_bytes)
            .and_then(|value| value.checked_add(candidate_bytes))
            .ok_or_else(|| {
                DomainError::InternalError("Chat backup byte count overflowed".into())
            })?;
        let limit = policy.max_total_bytes as u64;

        for entry in &oldest {
            if retained_bytes <= limit {
                break;
            }
            if selected.insert(entry.file_name.as_str()) {
                retained_bytes -= entry.byte_len;
            }
        }
    }

    Ok(oldest
        .into_iter()
        .filter(|entry| selected.contains(entry.file_name.as_str()))
        .map(|entry| entry.file_name.clone())
        .collect())
}

fn oldest_entries(inventory: &BackupInventory) -> Vec<&BackupEntry> {
    let mut entries: Vec<_> = inventory.entries.iter().collect();
    entries.sort_by(|left, right| {
        left.modified
            .cmp(&right.modified)
            .then_with(|| left.file_name.cmp(&right.file_name))
    });
    entries
}

pub(super) fn is_finalized_backup_name(file_name: &str) -> bool {
    file_name.starts_with(FileChatRepository::CHAT_BACKUP_PREFIX)
        && BackupFormat::parse_physical_file_name(file_name).is_some()
}

pub(super) fn parsed_backup_prefix(file_name: &str) -> Option<String> {
    let (_, logical_file_name) = BackupFormat::parse_physical_file_name(file_name)?;
    let stem = logical_file_name
        .strip_prefix(FileChatRepository::CHAT_BACKUP_PREFIX)?
        .strip_suffix(".jsonl")?;
    let (name, timestamp) = stem.rsplit_once('_')?;
    NaiveDateTime::parse_from_str(timestamp, "%Y%m%d-%H%M%S").ok()?;
    Some(format!(
        "{}{}_",
        FileChatRepository::CHAT_BACKUP_PREFIX,
        name
    ))
}

pub(super) fn is_backup_temp_name(file_name: &str) -> bool {
    let Some(identifier) = file_name.strip_prefix(BACKUP_TEMP_PREFIX) else {
        return false;
    };
    identifier.len() == 32 && uuid::Uuid::parse_str(identifier).is_ok()
}

impl FileChatRepository {
    pub(super) async fn scan_backup_inventory(
        &self,
        target_format: BackupFormat,
    ) -> Result<BackupInventory, DomainError> {
        self.ensure_directory_exists().await?;

        let mut inventory = BackupInventory::default();
        let mut entries = fs::read_dir(&self.backups_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read chat backup directory {:?}: {}",
                self.backups_dir, error
            ))
        })?;

        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to enumerate chat backup directory {:?}: {}",
                self.backups_dir, error
            ))
        })? {
            let Some(file_name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
                continue;
            };
            let file_type = match entry.file_type().await {
                Ok(file_type) => file_type,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(DomainError::InternalError(format!(
                        "Failed to read chat backup entry type {:?}: {}",
                        entry.path(),
                        error
                    )));
                }
            };
            if !file_type.is_file() {
                continue;
            }

            if is_backup_temp_name(&file_name) {
                match fs::remove_file(entry.path()).await {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(DomainError::InternalError(format!(
                            "Failed to remove stale chat backup staging file {:?}: {}",
                            entry.path(),
                            error
                        )));
                    }
                }
                continue;
            }

            if !is_finalized_backup_name(&file_name) {
                continue;
            }

            let (format, logical_file_name) = BackupFormat::parse_physical_file_name(&file_name)
                .ok_or_else(|| {
                    DomainError::InvalidData(format!(
                        "Invalid finalized chat backup name: {file_name}"
                    ))
                })?;

            let metadata = match entry.metadata().await {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(DomainError::InternalError(format!(
                        "Failed to read chat backup metadata {:?}: {}",
                        entry.path(),
                        error
                    )));
                }
            };
            let modified = metadata.modified().map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read chat backup modification time {:?}: {}",
                    entry.path(),
                    error
                ))
            })?;
            let candidate = BackupEntry {
                logical_file_name,
                parsed_prefix: parsed_backup_prefix(&file_name),
                file_name,
                format,
                modified,
                byte_len: metadata.len(),
                content_signature: None,
            };
            self.insert_scanned_backup_entry(&mut inventory, candidate, target_format)
                .await?;
        }

        Ok(inventory)
    }

    async fn insert_scanned_backup_entry(
        &self,
        inventory: &mut BackupInventory,
        candidate: BackupEntry,
        target_format: BackupFormat,
    ) -> Result<(), DomainError> {
        let Some(existing) = inventory
            .find_by_logical_name(&candidate.logical_file_name)
            .cloned()
        else {
            return inventory.insert(candidate);
        };

        let (mut target_entry, source_entry) = if existing.format == target_format {
            (existing.clone(), candidate)
        } else if candidate.format == target_format {
            (candidate, existing.clone())
        } else {
            return Err(DomainError::Conflict(format!(
                "Multiple chat backup files share the logical name {}",
                existing.logical_file_name
            )));
        };
        let target_path = self.backups_dir.join(&target_entry.file_name);
        let source_path = self.backups_dir.join(&source_entry.file_name);

        set_backup_modified(&target_path, source_entry.modified).await?;
        match fs::remove_file(&source_path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to finish interrupted chat backup conversion {}: {error}",
                    source_path.display()
                )));
            }
        }

        target_entry.modified = source_entry.modified;
        inventory.remove(&existing.file_name);
        inventory.insert(target_entry)?;
        self.remove_summary_cache_for_path(&source_path).await;
        self.remove_summary_cache_for_path(&target_path).await;
        tracing::warn!(
            logical_name = %existing.logical_file_name,
            kept = ?target_format,
            "Recovered interrupted chat backup format conversion"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    fn entry(name: &str, prefix: Option<&str>, age: u64, byte_len: u64) -> BackupEntry {
        BackupEntry {
            logical_file_name: name.to_string(),
            file_name: name.to_string(),
            format: BackupFormat::RawJsonl,
            parsed_prefix: prefix.map(ToOwned::to_owned),
            modified: UNIX_EPOCH + Duration::from_secs(age),
            byte_len,
            content_signature: None,
        }
    }

    fn policy(prefix: i64, files: i64, bytes: i64) -> ChatBackupSettings {
        ChatBackupSettings {
            automatic_enabled: true,
            zstd_compression_enabled: false,
            max_files_per_prefix: prefix,
            max_total_files: files,
            max_total_bytes: bytes,
        }
    }

    #[test]
    fn parser_uses_the_fixed_timestamp_tail() {
        assert_eq!(
            parsed_backup_prefix("chat_角色_a_b_20260714-010203.jsonl").as_deref(),
            Some("chat_角色_a_b_")
        );
        assert_eq!(parsed_backup_prefix("chat_角色_a_b_bad.jsonl"), None);
    }

    #[test]
    fn inventory_byte_overflow_is_an_error_not_a_panic() {
        let mut inventory = BackupInventory::default();
        inventory
            .insert(entry("largest", None, 1, u64::MAX))
            .unwrap();

        assert!(inventory.insert(entry("overflow", None, 2, 1)).is_err());
        assert_eq!(inventory.entries.len(), 1);
        assert_eq!(inventory.total_bytes, u64::MAX);
    }

    #[test]
    fn planner_combines_prefix_file_and_byte_limits_oldest_first() {
        let mut inventory = BackupInventory::default();
        inventory.insert(entry("a1", Some("a"), 1, 3)).unwrap();
        inventory.insert(entry("a2", Some("a"), 2, 3)).unwrap();
        inventory.insert(entry("b1", Some("b"), 3, 3)).unwrap();

        let deleted = plan_evictions(
            &inventory,
            policy(2, 3, 7),
            Some(BackupCandidate {
                prefix: "a",
                byte_len: 3,
            }),
        )
        .expect("plan candidate");

        assert_eq!(deleted, ["a1", "a2"]);
    }

    #[test]
    fn planner_replaces_the_old_entry_when_prefix_limit_is_one() {
        let mut inventory = BackupInventory::default();
        inventory.insert(entry("old", Some("a"), 1, 10)).unwrap();

        let deleted = plan_evictions(
            &inventory,
            policy(1, -1, -1),
            Some(BackupCandidate {
                prefix: "a",
                byte_len: 10,
            }),
        )
        .expect("plan candidate");

        assert_eq!(deleted, ["old"]);
    }

    #[test]
    fn zero_limit_purges_reconcile_and_rejects_candidates() {
        let mut inventory = BackupInventory::default();
        inventory.insert(entry("old", None, 1, 10)).unwrap();
        assert_eq!(
            plan_evictions(&inventory, policy(0, -1, -1), None).expect("plan reconcile"),
            ["old"]
        );
        assert!(matches!(
            plan_evictions(
                &inventory,
                policy(0, -1, -1),
                Some(BackupCandidate {
                    prefix: "a",
                    byte_len: 1,
                })
            ),
            Err(DomainError::Conflict(_))
        ));
    }

    #[test]
    fn candidate_equal_to_byte_limit_is_admitted_but_larger_is_rejected() {
        let inventory = BackupInventory::default();
        assert!(
            plan_evictions(
                &inventory,
                policy(-1, -1, 10),
                Some(BackupCandidate {
                    prefix: "a",
                    byte_len: 10,
                })
            )
            .is_ok()
        );
        assert!(matches!(
            plan_evictions(
                &inventory,
                policy(-1, -1, 10),
                Some(BackupCandidate {
                    prefix: "a",
                    byte_len: 11,
                })
            ),
            Err(DomainError::Conflict(_))
        ));
    }

    #[test]
    fn unparseable_backups_still_count_toward_global_limits() {
        let mut inventory = BackupInventory::default();
        inventory
            .insert(entry("malformed-old", None, 1, 4))
            .unwrap();
        inventory
            .insert(entry("malformed-new", None, 2, 4))
            .unwrap();

        assert_eq!(
            plan_evictions(&inventory, policy(-1, 1, -1), None).expect("plan global retention"),
            ["malformed-old"]
        );
        assert_eq!(
            plan_evictions(&inventory, policy(-1, -1, 4), None)
                .expect("plan global byte retention"),
            ["malformed-old"]
        );
    }

    #[test]
    fn prefix_limit_uses_exact_parsed_prefixes() {
        let mut inventory = BackupInventory::default();
        inventory
            .insert(entry("al", Some("chat_al_"), 1, 1))
            .unwrap();
        inventory
            .insert(entry("alice", Some("chat_al_ice_"), 2, 1))
            .unwrap();

        assert_eq!(
            plan_evictions(
                &inventory,
                policy(1, -1, -1),
                Some(BackupCandidate {
                    prefix: "chat_al_",
                    byte_len: 1,
                })
            )
            .expect("plan exact prefix retention"),
            ["al"]
        );
    }
}
