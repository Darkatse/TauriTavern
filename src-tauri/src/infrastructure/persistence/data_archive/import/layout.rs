use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::domain::errors::DomainError;

use super::archive::{self, ArchiveFormat};
use crate::infrastructure::persistence::data_archive::shared::{
    is_user_handle_marker, is_user_root_marker, path_components,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKind {
    DataRoot,
    UserHandleRoot,
    UserRoot,
}

#[derive(Debug, Clone)]
pub struct LayoutMeta {
    pub format: ArchiveFormat,
    pub source_prefix: PathBuf,
    pub kind: LayoutKind,
    pub scanned_entries: usize,
    source_users: BTreeSet<String>,
}

impl LayoutMeta {
    pub fn source_users(&self) -> &BTreeSet<String> {
        &self.source_users
    }

    pub fn source_users_for_result(&self) -> Vec<String> {
        if self.source_users.is_empty() {
            return match self.kind {
                LayoutKind::UserRoot => vec![
                    crate::infrastructure::persistence::data_archive::shared::DEFAULT_USER_HANDLE
                        .to_string(),
                ],
                _ => Vec::new(),
            };
        }
        self.source_users.iter().cloned().collect()
    }
}

#[derive(Debug, Clone)]
struct PrefixEval {
    prefix_components: Vec<String>,
    prefix_path: PathBuf,
    kind: LayoutKind,
    source_users: BTreeSet<String>,
    entries_under_prefix: usize,
}

#[derive(Debug, Default, Clone)]
struct PrefixStats {
    entries_under_prefix: usize,
    has_any: bool,
    has_user_root_marker_at_root: bool,
    has_root_tauritavern: bool,
    has_global_extensions: bool,
    source_users: BTreeSet<String>,
}

pub fn scan_archive_layout(
    archive_path: &Path,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<LayoutMeta, DomainError> {
    let mut candidate_stats = BTreeMap::new();

    let scanned_archive = archive::scan_archive(archive_path, is_cancelled, &mut |path| {
        if matches!(
            path.components().next(),
            Some(std::path::Component::Normal(component))
                if component == OsStr::new("__MACOSX")
        ) {
            return Ok(());
        }

        let components = path_components(path);
        if components.is_empty() {
            return Ok(());
        }

        record_entry_layout(&mut candidate_stats, &components);
        Ok(())
    })?;
    let scanned_entries = scanned_archive.scanned_entries;

    if scanned_entries == 0 {
        return Err(DomainError::InvalidData("Archive is empty".to_string()));
    }

    let candidates = candidate_stats
        .iter()
        .flat_map(|(prefix, stats)| eval_prefix(prefix, stats))
        .collect::<Vec<_>>();

    let chosen = choose_candidate(&candidates)?;

    Ok(LayoutMeta {
        format: scanned_archive.format,
        source_prefix: chosen.prefix_path,
        kind: chosen.kind,
        scanned_entries,
        source_users: chosen.source_users,
    })
}

fn record_entry_layout(candidate_stats: &mut BTreeMap<Vec<String>, PrefixStats>, entry: &[String]) {
    record_prefix_candidate(candidate_stats, &[], entry);

    let first = &entry[0];

    if first == "data" {
        record_prefix_candidate(candidate_stats, &entry[..1], &entry[1..]);
        return;
    }

    record_prefix_candidate(candidate_stats, &entry[..1], &entry[1..]);

    if entry.len() >= 2 && entry[1] == "data" {
        record_prefix_candidate(candidate_stats, &entry[..2], &entry[2..]);
    }
}

fn record_prefix_candidate(
    candidate_stats: &mut BTreeMap<Vec<String>, PrefixStats>,
    prefix: &[String],
    remainder: &[String],
) {
    let stats = candidate_stats.entry(prefix.to_vec()).or_default();
    stats.entries_under_prefix += 1;

    if remainder.is_empty() {
        return;
    }

    stats.has_any = true;

    let first = remainder[0].as_str();
    if is_user_root_marker(first) {
        stats.has_user_root_marker_at_root = true;
    }

    if first == "_tauritavern" {
        stats.has_root_tauritavern = true;
    }

    if remainder.len() >= 2 && first == "extensions" && remainder[1] == "third-party" {
        stats.has_global_extensions = true;
    }

    collect_user_handles_from_components(remainder, &mut stats.source_users);
}

fn collect_user_handles_from_components(
    components: &[String],
    user_handles: &mut BTreeSet<String>,
) {
    if components.len() < 2 {
        return;
    }

    let handle = &components[0];
    if is_user_root_marker(handle) {
        return;
    }

    if is_user_handle_marker(&components[1]) {
        user_handles.insert(handle.clone());
    }
}

fn eval_prefix(prefix_components: &[String], stats: &PrefixStats) -> Vec<PrefixEval> {
    if !stats.has_any {
        return Vec::new();
    }

    let has_data_root_feature = stats.has_root_tauritavern || stats.has_global_extensions;
    let prefix_last_is_data = prefix_components
        .last()
        .is_some_and(|value| value == "data");

    let mut candidates = Vec::new();
    if has_data_root_feature || (prefix_last_is_data && !stats.source_users.is_empty()) {
        candidates.push(build_prefix_eval(
            prefix_components,
            LayoutKind::DataRoot,
            stats,
        ));
        if stats.has_user_root_marker_at_root {
            candidates.push(build_prefix_eval(
                prefix_components,
                LayoutKind::UserRoot,
                stats,
            ));
        }
        return candidates;
    }

    if !stats.source_users.is_empty() {
        candidates.push(build_prefix_eval(
            prefix_components,
            LayoutKind::UserHandleRoot,
            stats,
        ));
    }

    if stats.has_user_root_marker_at_root {
        candidates.push(build_prefix_eval(
            prefix_components,
            LayoutKind::UserRoot,
            stats,
        ));
    }

    candidates
}

fn build_prefix_eval(
    prefix_components: &[String],
    kind: LayoutKind,
    stats: &PrefixStats,
) -> PrefixEval {
    let mut prefix_path = PathBuf::new();
    for component in prefix_components {
        prefix_path.push(component);
    }

    PrefixEval {
        prefix_components: prefix_components.to_vec(),
        prefix_path,
        kind,
        source_users: stats.source_users.clone(),
        entries_under_prefix: stats.entries_under_prefix,
    }
}

fn choose_candidate(candidates: &[PrefixEval]) -> Result<PrefixEval, DomainError> {
    assert_no_same_prefix_kind_conflicts(candidates)?;

    for kind in [
        LayoutKind::DataRoot,
        LayoutKind::UserHandleRoot,
        LayoutKind::UserRoot,
    ] {
        let kind_candidates = candidates
            .iter()
            .filter(|candidate| candidate.kind == kind)
            .collect::<Vec<_>>();
        if kind_candidates.is_empty() {
            continue;
        }

        let chosen = choose_covering_candidate(&kind_candidates)?;
        assert_no_recognized_entries_outside_prefix(&chosen, candidates)?;
        return Ok(chosen);
    }

    Err(DomainError::InvalidData(
        "Archive does not contain a recognizable data directory".to_string(),
    ))
}

fn assert_no_same_prefix_kind_conflicts(candidates: &[PrefixEval]) -> Result<(), DomainError> {
    for (index, candidate) in candidates.iter().enumerate() {
        if candidates.iter().skip(index + 1).any(|other| {
            other.prefix_components == candidate.prefix_components && other.kind != candidate.kind
        }) {
            return Err(DomainError::InvalidData(
                "Archive layout is ambiguous".to_string(),
            ));
        }
    }

    Ok(())
}

fn choose_covering_candidate(candidates: &[&PrefixEval]) -> Result<PrefixEval, DomainError> {
    if candidates.len() == 1 {
        return Ok((*candidates[0]).clone());
    }

    let covering_candidates = candidates
        .iter()
        .filter(|candidate| {
            candidates.iter().all(|other| {
                other.prefix_components == candidate.prefix_components
                    || other
                        .prefix_components
                        .starts_with(&candidate.prefix_components)
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

fn assert_no_recognized_entries_outside_prefix(
    chosen: &PrefixEval,
    candidates: &[PrefixEval],
) -> Result<(), DomainError> {
    for candidate in candidates {
        if candidate.prefix_components == chosen.prefix_components {
            continue;
        }

        if candidate
            .prefix_components
            .starts_with(&chosen.prefix_components)
        {
            continue;
        }

        if chosen
            .prefix_components
            .starts_with(&candidate.prefix_components)
            && candidate.entries_under_prefix == chosen.entries_under_prefix
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

    use std::fs;
    use std::fs::File;
    use std::io::Write;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions as FileOptions;

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).expect("create zip");
        let mut writer = ZipWriter::new(file);
        for (name, bytes) in entries {
            writer
                .start_file(*name, FileOptions::default())
                .expect("start file");
            writer.write_all(bytes).expect("write bytes");
        }
        writer.finish().expect("finish zip");
    }

    #[test]
    fn detects_data_default_user_layout() {
        let root =
            std::env::temp_dir().join(format!("tauritavern-layout-{}", rand::random::<u64>()));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(&zip_path, &[("data/default-user/characters/a.json", b"{}")]);

        let layout = scan_archive_layout(&zip_path, &|| false).expect("scan layout");
        assert_eq!(layout.kind, LayoutKind::DataRoot);
        assert_eq!(layout.source_prefix, PathBuf::from("data"));

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn detects_default_user_layout() {
        let root =
            std::env::temp_dir().join(format!("tauritavern-layout-{}", rand::random::<u64>()));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(&zip_path, &[("default-user/characters/a.json", b"{}")]);

        let layout = scan_archive_layout(&zip_path, &|| false).expect("scan layout");
        assert_eq!(layout.kind, LayoutKind::UserHandleRoot);
        assert!(layout.source_prefix.as_os_str().is_empty());

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn detects_default_user_layout_with_extra_root_file() {
        let root =
            std::env::temp_dir().join(format!("tauritavern-layout-{}", rand::random::<u64>()));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(
            &zip_path,
            &[
                ("README.txt", b"hello"),
                ("default-user/characters/a.json", b"{}"),
            ],
        );

        let layout = scan_archive_layout(&zip_path, &|| false).expect("scan layout");
        assert_eq!(layout.kind, LayoutKind::UserHandleRoot);
        assert!(layout.source_prefix.as_os_str().is_empty());

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn detects_default_user_layout_with_macosx_junk() {
        let root =
            std::env::temp_dir().join(format!("tauritavern-layout-{}", rand::random::<u64>()));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(
            &zip_path,
            &[
                ("__MACOSX/._junk", b"junk"),
                ("default-user/characters/a.json", b"{}"),
            ],
        );

        let layout = scan_archive_layout(&zip_path, &|| false).expect("scan layout");
        assert_eq!(layout.kind, LayoutKind::UserHandleRoot);
        assert!(layout.source_prefix.as_os_str().is_empty());

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn ignores_macosx_resource_forks_for_data_root_layout() {
        let root =
            std::env::temp_dir().join(format!("tauritavern-layout-{}", rand::random::<u64>()));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(
            &zip_path,
            &[
                ("data/default-user/characters/a.json", b"{}"),
                ("__MACOSX/data/default-user/characters/._a.json", b"junk"),
            ],
        );

        let layout = scan_archive_layout(&zip_path, &|| false).expect("scan layout");
        assert_eq!(layout.kind, LayoutKind::DataRoot);
        assert_eq!(layout.source_prefix, PathBuf::from("data"));

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn ignores_macosx_resource_forks_for_user_handle_layout() {
        let root =
            std::env::temp_dir().join(format!("tauritavern-layout-{}", rand::random::<u64>()));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(
            &zip_path,
            &[
                ("default-user/characters/a.json", b"{}"),
                ("__MACOSX/default-user/characters/._a.json", b"junk"),
            ],
        );

        let layout = scan_archive_layout(&zip_path, &|| false).expect("scan layout");
        assert_eq!(layout.kind, LayoutKind::UserHandleRoot);
        assert!(layout.source_prefix.as_os_str().is_empty());

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn detects_user_root_layout() {
        let root =
            std::env::temp_dir().join(format!("tauritavern-layout-{}", rand::random::<u64>()));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(&zip_path, &[("characters/a.json", b"{}")]);

        let layout = scan_archive_layout(&zip_path, &|| false).expect("scan layout");
        assert_eq!(layout.kind, LayoutKind::UserRoot);
        assert!(layout.source_prefix.as_os_str().is_empty());

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn detects_user_root_layout_with_marker_named_content_paths() {
        let root =
            std::env::temp_dir().join(format!("tauritavern-layout-{}", rand::random::<u64>()));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(
            &zip_path,
            &[
                ("characters/a.json", b"{}"),
                ("chats/characters/session.jsonl", b"{}"),
                ("assets/worlds/cover.png", b"image"),
            ],
        );

        let layout = scan_archive_layout(&zip_path, &|| false).expect("scan layout");
        assert_eq!(layout.kind, LayoutKind::UserRoot);
        assert!(layout.source_prefix.as_os_str().is_empty());

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn detects_single_file_settings_layout() {
        let root =
            std::env::temp_dir().join(format!("tauritavern-layout-{}", rand::random::<u64>()));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(&zip_path, &[("settings.json", b"{}")]);

        let layout = scan_archive_layout(&zip_path, &|| false).expect("scan layout");
        assert_eq!(layout.kind, LayoutKind::UserRoot);

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn detects_wrapped_data_layout() {
        let root =
            std::env::temp_dir().join(format!("tauritavern-layout-{}", rand::random::<u64>()));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(
            &zip_path,
            &[("BackupRoot/data/default-user/chats/hello.jsonl", b"{}")],
        );

        let layout = scan_archive_layout(&zip_path, &|| false).expect("scan layout");
        assert_eq!(layout.kind, LayoutKind::DataRoot);
        assert_eq!(
            layout.source_prefix,
            PathBuf::from("BackupRoot").join("data")
        );

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn rejects_mixed_user_root_and_user_handle_root_at_same_prefix() {
        let root =
            std::env::temp_dir().join(format!("tauritavern-layout-{}", rand::random::<u64>()));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(
            &zip_path,
            &[
                ("characters/a.json", b"{}"),
                ("default-user/characters/b.json", b"{}"),
            ],
        );

        let error = scan_archive_layout(&zip_path, &|| false).unwrap_err();
        assert!(matches!(error, DomainError::InvalidData(_)));

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn rejects_mixed_data_root_and_user_root_at_same_prefix() {
        let root =
            std::env::temp_dir().join(format!("tauritavern-layout-{}", rand::random::<u64>()));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(
            &zip_path,
            &[
                ("_tauritavern/state.json", b"{}"),
                ("characters/a.json", b"{}"),
            ],
        );

        let error = scan_archive_layout(&zip_path, &|| false).unwrap_err();
        assert!(matches!(error, DomainError::InvalidData(_)));

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn rejects_ambiguous_mixed_roots() {
        let root =
            std::env::temp_dir().join(format!("tauritavern-layout-{}", rand::random::<u64>()));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(
            &zip_path,
            &[
                ("data/default-user/characters/a.json", b"{}"),
                ("default-user/characters/b.json", b"{}"),
            ],
        );

        let error = scan_archive_layout(&zip_path, &|| false).unwrap_err();
        assert!(matches!(error, DomainError::InvalidData(_)));

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }
}
