use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tt_domain::errors::DomainError;

use super::archive::ScannedArchive;
use crate::data_archive::shared::{
    IMPORT_TARGET_USER_HANDLE, is_macos_resource_fork_path, is_sillytavern_user_root_entry,
    is_user_handle_root_child_entry, path_components,
};

/// Controls how entries under `archive_root_prefix` map into the normalized data root.
/// Wrapped `BackupRoot/data/...` archives are `DataRoot` with a non-empty prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveLayoutPolicy {
    DataRoot,
    UserHandleRoot,
    SillyTavernUserRoot,
}

#[derive(Debug, Clone)]
pub struct DetectedArchiveLayout {
    pub archive_root_prefix: PathBuf,
    pub policy: ArchiveLayoutPolicy,
    pub scanned_entries: usize,
    detected_user_handles: BTreeSet<String>,
}

impl DetectedArchiveLayout {
    pub fn detected_user_handles(&self) -> &BTreeSet<String> {
        &self.detected_user_handles
    }

    pub fn source_user_handles_for_import_result(&self) -> Vec<String> {
        if self.detected_user_handles.is_empty() {
            return match self.policy {
                ArchiveLayoutPolicy::SillyTavernUserRoot => {
                    vec![IMPORT_TARGET_USER_HANDLE.to_string()]
                }
                _ => Vec::new(),
            };
        }
        self.detected_user_handles.iter().cloned().collect()
    }
}

#[derive(Debug, Clone)]
struct ArchiveLayoutCandidate {
    archive_root_components: Vec<String>,
    archive_root_prefix: PathBuf,
    policy: ArchiveLayoutPolicy,
    detected_user_handles: BTreeSet<String>,
    entries_under_root: usize,
}

#[derive(Debug, Default, Clone)]
struct ArchiveRootStats {
    entries_under_root: usize,
    has_entries: bool,
    has_sillytavern_user_root_marker: bool,
    has_tauritavern_state_root: bool,
    has_global_extensions_root: bool,
    detected_user_handles: BTreeSet<String>,
}

#[derive(Debug, Default)]
pub struct ArchiveLayoutScan {
    candidate_stats: BTreeMap<Vec<String>, ArchiveRootStats>,
}

impl ArchiveLayoutScan {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn visit_path(&mut self, path: &Path) -> Result<(), DomainError> {
        if is_macos_resource_fork_path(path) {
            return Ok(());
        }

        let components = path_components(path);
        if components.is_empty() {
            return Ok(());
        }

        record_archive_layout_candidates(&mut self.candidate_stats, &components);
        Ok(())
    }

    pub fn finish(self, scanned_archive: ScannedArchive) -> Result<DetectedArchiveLayout, DomainError> {
        let scanned_entries = scanned_archive.scanned_entries;

        if scanned_entries == 0 {
            return Err(DomainError::InvalidData("Archive is empty".to_string()));
        }

        let candidates = self
            .candidate_stats
            .iter()
            .flat_map(|(prefix, stats)| layout_policy_candidates_for_root(prefix, stats))
            .collect::<Vec<_>>();

        let chosen = choose_archive_layout(&candidates)?;

        Ok(DetectedArchiveLayout {
            archive_root_prefix: chosen.archive_root_prefix,
            policy: chosen.policy,
            scanned_entries,
            detected_user_handles: chosen.detected_user_handles,
        })
    }
}

fn record_archive_layout_candidates(
    candidate_stats: &mut BTreeMap<Vec<String>, ArchiveRootStats>,
    entry: &[String],
) {
    record_archive_root_candidate(candidate_stats, &[], entry);

    let first = &entry[0];

    if first == "data" {
        record_archive_root_candidate(candidate_stats, &entry[..1], &entry[1..]);
        return;
    }

    record_archive_root_candidate(candidate_stats, &entry[..1], &entry[1..]);

    if entry.len() >= 2 && entry[1] == "data" {
        record_archive_root_candidate(candidate_stats, &entry[..2], &entry[2..]);
    }
}

fn record_archive_root_candidate(
    candidate_stats: &mut BTreeMap<Vec<String>, ArchiveRootStats>,
    prefix: &[String],
    remainder: &[String],
) {
    let stats = candidate_stats.entry(prefix.to_vec()).or_default();
    stats.entries_under_root += 1;

    if remainder.is_empty() {
        return;
    }

    stats.has_entries = true;

    let first = remainder[0].as_str();
    if is_sillytavern_user_root_entry(first) {
        stats.has_sillytavern_user_root_marker = true;
    }

    if first == "_tauritavern" {
        stats.has_tauritavern_state_root = true;
    }

    if remainder.len() >= 2 && first == "extensions" && remainder[1] == "third-party" {
        stats.has_global_extensions_root = true;
    }

    collect_user_handles_from_archive_entry(remainder, &mut stats.detected_user_handles);
}

fn collect_user_handles_from_archive_entry(
    components: &[String],
    user_handles: &mut BTreeSet<String>,
) {
    if components.len() < 2 {
        return;
    }

    let handle = &components[0];
    if is_sillytavern_user_root_entry(handle) {
        return;
    }

    if is_user_handle_root_child_entry(&components[1]) {
        user_handles.insert(handle.clone());
    }
}

fn layout_policy_candidates_for_root(
    archive_root_components: &[String],
    stats: &ArchiveRootStats,
) -> Vec<ArchiveLayoutCandidate> {
    if !stats.has_entries {
        return Vec::new();
    }

    let has_data_root_feature =
        stats.has_tauritavern_state_root || stats.has_global_extensions_root;
    let root_last_is_data = archive_root_components
        .last()
        .is_some_and(|value| value == "data");

    let mut candidates = Vec::new();
    if has_data_root_feature || (root_last_is_data && !stats.detected_user_handles.is_empty()) {
        candidates.push(build_archive_layout_candidate(
            archive_root_components,
            ArchiveLayoutPolicy::DataRoot,
            stats,
        ));
        if stats.has_sillytavern_user_root_marker {
            candidates.push(build_archive_layout_candidate(
                archive_root_components,
                ArchiveLayoutPolicy::SillyTavernUserRoot,
                stats,
            ));
        }
        return candidates;
    }

    if !stats.detected_user_handles.is_empty() {
        candidates.push(build_archive_layout_candidate(
            archive_root_components,
            ArchiveLayoutPolicy::UserHandleRoot,
            stats,
        ));
    }

    if stats.has_sillytavern_user_root_marker {
        candidates.push(build_archive_layout_candidate(
            archive_root_components,
            ArchiveLayoutPolicy::SillyTavernUserRoot,
            stats,
        ));
    }

    candidates
}

fn build_archive_layout_candidate(
    archive_root_components: &[String],
    policy: ArchiveLayoutPolicy,
    stats: &ArchiveRootStats,
) -> ArchiveLayoutCandidate {
    let mut archive_root_prefix = PathBuf::new();
    for component in archive_root_components {
        archive_root_prefix.push(component);
    }

    ArchiveLayoutCandidate {
        archive_root_components: archive_root_components.to_vec(),
        archive_root_prefix,
        policy,
        detected_user_handles: stats.detected_user_handles.clone(),
        entries_under_root: stats.entries_under_root,
    }
}

fn choose_archive_layout(
    candidates: &[ArchiveLayoutCandidate],
) -> Result<ArchiveLayoutCandidate, DomainError> {
    assert_no_ambiguous_layouts_for_same_root(candidates)?;

    for policy in [
        ArchiveLayoutPolicy::DataRoot,
        ArchiveLayoutPolicy::UserHandleRoot,
        ArchiveLayoutPolicy::SillyTavernUserRoot,
    ] {
        let layout_candidates = candidates
            .iter()
            .filter(|candidate| candidate.policy == policy)
            .collect::<Vec<_>>();
        if layout_candidates.is_empty() {
            continue;
        }

        let chosen = choose_covering_archive_root(&layout_candidates)?;
        assert_no_recognized_entries_outside_archive_root(&chosen, candidates)?;
        return Ok(chosen);
    }

    Err(DomainError::InvalidData(
        "Archive does not contain a recognizable data directory".to_string(),
    ))
}

fn assert_no_ambiguous_layouts_for_same_root(
    candidates: &[ArchiveLayoutCandidate],
) -> Result<(), DomainError> {
    for (index, candidate) in candidates.iter().enumerate() {
        if candidates.iter().skip(index + 1).any(|other| {
            other.archive_root_components == candidate.archive_root_components
                && other.policy != candidate.policy
        }) {
            return Err(DomainError::InvalidData(
                "Archive layout is ambiguous".to_string(),
            ));
        }
    }

    Ok(())
}

fn choose_covering_archive_root(
    candidates: &[&ArchiveLayoutCandidate],
) -> Result<ArchiveLayoutCandidate, DomainError> {
    if candidates.len() == 1 {
        return Ok((*candidates[0]).clone());
    }

    let covering_candidates = candidates
        .iter()
        .filter(|candidate| {
            candidates.iter().all(|other| {
                other.archive_root_components == candidate.archive_root_components
                    || other
                        .archive_root_components
                        .starts_with(&candidate.archive_root_components)
            })
        })
        .collect::<Vec<_>>();

    if covering_candidates.len() == 1 {
        return Ok((**covering_candidates[0]).clone());
    }

    Err(DomainError::InvalidData(
        "Archive layout is ambiguous".to_string(),
    ))
}

fn assert_no_recognized_entries_outside_archive_root(
    chosen: &ArchiveLayoutCandidate,
    candidates: &[ArchiveLayoutCandidate],
) -> Result<(), DomainError> {
    for candidate in candidates {
        if candidate.archive_root_components == chosen.archive_root_components {
            continue;
        }

        if candidate
            .archive_root_components
            .starts_with(&chosen.archive_root_components)
        {
            continue;
        }

        if chosen
            .archive_root_components
            .starts_with(&candidate.archive_root_components)
            && candidate.entries_under_root == chosen.entries_under_root
        {
            continue;
        }

        return Err(DomainError::InvalidData(
            "Archive layout is ambiguous".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect_layout(entries: &[&str]) -> Result<DetectedArchiveLayout, DomainError> {
        let mut scan = ArchiveLayoutScan::new();
        for entry in entries {
            scan.visit_path(Path::new(entry))?;
        }
        scan.finish(ScannedArchive {
            scanned_entries: entries.len(),
        })
    }

    #[test]
    fn detects_tauritavern_data_root_layout() {
        let layout = detect_layout(&["data/default-user/characters/a.json"]).expect("scan layout");
        assert_eq!(layout.policy, ArchiveLayoutPolicy::DataRoot);
        assert_eq!(layout.archive_root_prefix, PathBuf::from("data"));
    }

    #[test]
    fn detects_user_handle_root_layout() {
        let layout = detect_layout(&["default-user/characters/a.json"]).expect("scan layout");
        assert_eq!(layout.policy, ArchiveLayoutPolicy::UserHandleRoot);
        assert!(layout.archive_root_prefix.as_os_str().is_empty());
    }

    #[test]
    fn detects_user_handle_root_layout_with_extra_root_file() {
        let layout =
            detect_layout(&["README.txt", "default-user/characters/a.json"]).expect("scan layout");
        assert_eq!(layout.policy, ArchiveLayoutPolicy::UserHandleRoot);
        assert!(layout.archive_root_prefix.as_os_str().is_empty());
    }

    #[test]
    fn detects_user_handle_root_layout_with_macos_resource_forks() {
        let layout =
            detect_layout(&["__MACOSX/._junk", "default-user/characters/a.json"])
                .expect("scan layout");
        assert_eq!(layout.policy, ArchiveLayoutPolicy::UserHandleRoot);
        assert!(layout.archive_root_prefix.as_os_str().is_empty());
    }

    #[test]
    fn ignores_macos_resource_forks_for_data_root_layout() {
        let layout = detect_layout(&[
            "data/default-user/characters/a.json",
            "__MACOSX/data/default-user/characters/._a.json",
        ])
        .expect("scan layout");
        assert_eq!(layout.policy, ArchiveLayoutPolicy::DataRoot);
        assert_eq!(layout.archive_root_prefix, PathBuf::from("data"));
    }

    #[test]
    fn ignores_macos_resource_forks_for_user_handle_layout() {
        let layout = detect_layout(&[
            "default-user/characters/a.json",
            "__MACOSX/default-user/characters/._a.json",
        ])
        .expect("scan layout");
        assert_eq!(layout.policy, ArchiveLayoutPolicy::UserHandleRoot);
        assert!(layout.archive_root_prefix.as_os_str().is_empty());
    }

    #[test]
    fn detects_sillytavern_user_root_layout() {
        let layout = detect_layout(&["characters/a.json"]).expect("scan layout");
        assert_eq!(layout.policy, ArchiveLayoutPolicy::SillyTavernUserRoot);
        assert!(layout.archive_root_prefix.as_os_str().is_empty());
    }

    #[test]
    fn detects_sillytavern_user_root_layout_with_marker_named_content_paths() {
        let layout = detect_layout(&[
            "characters/a.json",
            "chats/characters/session.jsonl",
            "assets/worlds/cover.png",
        ])
        .expect("scan layout");
        assert_eq!(layout.policy, ArchiveLayoutPolicy::SillyTavernUserRoot);
        assert!(layout.archive_root_prefix.as_os_str().is_empty());
    }

    #[test]
    fn detects_single_file_settings_layout() {
        let layout = detect_layout(&["settings.json"]).expect("scan layout");
        assert_eq!(layout.policy, ArchiveLayoutPolicy::SillyTavernUserRoot);
    }

    #[test]
    fn detects_wrapped_data_layout() {
        let layout =
            detect_layout(&["BackupRoot/data/default-user/chats/hello.jsonl"])
                .expect("scan layout");
        assert_eq!(layout.policy, ArchiveLayoutPolicy::DataRoot);
        assert_eq!(
            layout.archive_root_prefix,
            PathBuf::from("BackupRoot").join("data")
        );
    }

    #[test]
    fn rejects_mixed_sillytavern_user_root_and_user_handle_root_at_same_prefix() {
        let error =
            detect_layout(&["characters/a.json", "default-user/characters/b.json"]).unwrap_err();
        assert!(matches!(error, DomainError::InvalidData(_)));
    }

    #[test]
    fn rejects_mixed_data_root_and_sillytavern_user_root_at_same_prefix() {
        let error = detect_layout(&["_tauritavern/state.json", "characters/a.json"]).unwrap_err();
        assert!(matches!(error, DomainError::InvalidData(_)));
    }

    #[test]
    fn rejects_ambiguous_mixed_roots() {
        let error = detect_layout(&[
            "data/default-user/characters/a.json",
            "default-user/characters/b.json",
        ])
        .unwrap_err();
        assert!(matches!(error, DomainError::InvalidData(_)));
    }
}
