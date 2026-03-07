use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use zip::ZipArchive;

use crate::domain::errors::DomainError;
use crate::infrastructure::zipkit;

use crate::infrastructure::persistence::data_archive::shared::{
    FILE_IO_BUFFER_BYTES, MAX_ARCHIVE_ENTRIES, collect_user_handles_from_components, internal_error,
    is_user_root_marker, path_components, validate_zip_entry_limits,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKind {
    DataRoot,
    UserHandleRoot,
    UserRoot,
}

#[derive(Debug, Clone)]
pub struct LayoutMeta {
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
                LayoutKind::UserRoot => vec![crate::infrastructure::persistence::data_archive::shared::DEFAULT_USER_HANDLE
                    .to_string()],
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
}

pub fn scan_archive_layout(archive_path: &Path) -> Result<LayoutMeta, DomainError> {
    let archive_file =
        File::open(archive_path).map_err(|error| internal_error("Failed to open archive file", error))?;
    let archive_reader = BufReader::with_capacity(FILE_IO_BUFFER_BYTES, archive_file);
    let mut archive = ZipArchive::new(archive_reader)
        .map_err(|error| internal_error("Failed to parse archive file", error))?;

    let mut scanned_entries = 0usize;
    let mut total_uncompressed_bytes = 0u64;

    let mut entry_components = Vec::with_capacity(archive.len());
    let mut top_level_dirs = BTreeSet::new();

    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| internal_error("Failed to read archive entry", error))?;
        let sanitized_path = zipkit::enclosed_zip_entry_path(&entry)?;
        if sanitized_path.as_os_str().is_empty() {
            continue;
        }

        validate_zip_entry_limits(
            entry.name(),
            entry.size(),
            entry.compressed_size(),
            &mut total_uncompressed_bytes,
        )?;

        scanned_entries = scanned_entries.saturating_add(1);
        if scanned_entries > MAX_ARCHIVE_ENTRIES {
            return Err(DomainError::InvalidData(format!(
                "Archive contains too many entries (>{})",
                MAX_ARCHIVE_ENTRIES
            )));
        }

        let components = path_components(&sanitized_path);
        if components.is_empty() {
            continue;
        }

        if let Some(first) = components.first() {
            top_level_dirs.insert(first.clone());
        }

        entry_components.push(components);
    }

    if scanned_entries == 0 {
        return Err(DomainError::InvalidData("Archive is empty".to_string()));
    }

    let mut candidate_prefixes = BTreeSet::new();
    candidate_prefixes.insert(Vec::<String>::new());
    candidate_prefixes.insert(vec!["data".to_string()]);
    for top in &top_level_dirs {
        candidate_prefixes.insert(vec![top.clone()]);
        if top != "data" {
            candidate_prefixes.insert(vec![top.clone(), "data".to_string()]);
        }
    }

    let mut candidates = Vec::new();
    for prefix in candidate_prefixes {
        if let Some(eval) = eval_prefix(&prefix, &entry_components) {
            candidates.push(eval);
        }
    }

    let chosen = choose_candidate(&candidates, &entry_components)?;

    Ok(LayoutMeta {
        source_prefix: chosen.prefix_path,
        kind: chosen.kind,
        scanned_entries,
        source_users: chosen.source_users,
    })
}

fn eval_prefix(prefix_components: &[String], entries: &[Vec<String>]) -> Option<PrefixEval> {
    let mut has_any = false;
    let mut has_user_root_marker_at_root = false;
    let mut has_root_tauritavern = false;
    let mut has_global_extensions = false;
    let mut source_users = BTreeSet::new();

    for entry in entries {
        if entry.len() < prefix_components.len() {
            continue;
        }
        if !entry.starts_with(prefix_components) {
            continue;
        }

        let remainder = &entry[prefix_components.len()..];
        if remainder.is_empty() {
            continue;
        }

        has_any = true;

        let first = remainder[0].as_str();
        if is_user_root_marker(first) {
            has_user_root_marker_at_root = true;
        }

        if first == "_tauritavern" {
            has_root_tauritavern = true;
        }

        if remainder.len() >= 2 && first == "extensions" && remainder[1] == "third-party" {
            has_global_extensions = true;
        }

        collect_user_handles_from_components(remainder, &mut source_users);
    }

    if !has_any {
        return None;
    }

    let has_data_root_feature = has_root_tauritavern || has_global_extensions;
    let prefix_last_is_data = prefix_components.last().is_some_and(|value| value == "data");

    if has_user_root_marker_at_root && (has_data_root_feature || !source_users.is_empty()) {
        return None;
    }

    let kind = if has_data_root_feature || (prefix_last_is_data && !source_users.is_empty()) {
        LayoutKind::DataRoot
    } else if !source_users.is_empty() {
        LayoutKind::UserHandleRoot
    } else if has_user_root_marker_at_root {
        LayoutKind::UserRoot
    } else {
        return None;
    };

    let mut prefix_path = PathBuf::new();
    for component in prefix_components {
        prefix_path.push(component);
    }

    Some(PrefixEval {
        prefix_components: prefix_components.to_vec(),
        prefix_path,
        kind,
        source_users,
    })
}

fn choose_candidate(candidates: &[PrefixEval], entries: &[Vec<String>]) -> Result<PrefixEval, DomainError> {
    let data_roots = candidates
        .iter()
        .filter(|candidate| candidate.kind == LayoutKind::DataRoot)
        .cloned()
        .collect::<Vec<_>>();
    if data_roots.len() > 1 {
        return Err(DomainError::InvalidData("Archive layout is ambiguous".to_string()));
    }
    if data_roots.len() == 1 {
        let chosen = data_roots[0].clone();
        assert_no_recognized_entries_outside_prefix(&chosen, candidates, entries)?;
        return Ok(chosen);
    }

    let user_handles = candidates
        .iter()
        .filter(|candidate| candidate.kind == LayoutKind::UserHandleRoot)
        .cloned()
        .collect::<Vec<_>>();
    if user_handles.len() > 1 {
        return Err(DomainError::InvalidData("Archive layout is ambiguous".to_string()));
    }
    if user_handles.len() == 1 {
        let chosen = user_handles[0].clone();
        assert_no_recognized_entries_outside_prefix(&chosen, candidates, entries)?;
        return Ok(chosen);
    }

    let user_roots = candidates
        .iter()
        .filter(|candidate| candidate.kind == LayoutKind::UserRoot)
        .cloned()
        .collect::<Vec<_>>();
    if user_roots.len() > 1 {
        return Err(DomainError::InvalidData("Archive layout is ambiguous".to_string()));
    }
    if user_roots.len() == 1 {
        let chosen = user_roots[0].clone();
        assert_no_recognized_entries_outside_prefix(&chosen, candidates, entries)?;
        return Ok(chosen);
    }

    Err(DomainError::InvalidData(
        "Archive does not contain a recognizable data directory".to_string(),
    ))
}

fn assert_no_recognized_entries_outside_prefix(
    chosen: &PrefixEval,
    candidates: &[PrefixEval],
    entries: &[Vec<String>],
) -> Result<(), DomainError> {
    for entry in entries {
        if entry.starts_with(&chosen.prefix_components) {
            continue;
        }

        let mut is_recognized_elsewhere = false;
        for candidate in candidates {
            if candidate.prefix_components == chosen.prefix_components {
                continue;
            }
            if entry.starts_with(&candidate.prefix_components) {
                is_recognized_elsewhere = true;
                break;
            }
        }

        if is_recognized_elsewhere {
            return Err(DomainError::InvalidData("Archive layout is ambiguous".to_string()));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;
    use std::io::Write;
    use zip::write::SimpleFileOptions as FileOptions;
    use zip::ZipWriter;

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
        let root = std::env::temp_dir().join(format!(
            "tauritavern-layout-{}",
            rand::random::<u64>()
        ));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(&zip_path, &[("data/default-user/characters/a.json", b"{}")]);

        let layout = scan_archive_layout(&zip_path).expect("scan layout");
        assert_eq!(layout.kind, LayoutKind::DataRoot);
        assert_eq!(layout.source_prefix, PathBuf::from("data"));

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn detects_default_user_layout() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-layout-{}",
            rand::random::<u64>()
        ));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(&zip_path, &[("default-user/characters/a.json", b"{}")]);

        let layout = scan_archive_layout(&zip_path).expect("scan layout");
        assert_eq!(layout.kind, LayoutKind::UserHandleRoot);
        assert!(layout.source_prefix.as_os_str().is_empty());

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn detects_default_user_layout_with_extra_root_file() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-layout-{}",
            rand::random::<u64>()
        ));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(
            &zip_path,
            &[
                ("README.txt", b"hello"),
                ("default-user/characters/a.json", b"{}"),
            ],
        );

        let layout = scan_archive_layout(&zip_path).expect("scan layout");
        assert_eq!(layout.kind, LayoutKind::UserHandleRoot);
        assert!(layout.source_prefix.as_os_str().is_empty());

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn detects_default_user_layout_with_macosx_junk() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-layout-{}",
            rand::random::<u64>()
        ));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(
            &zip_path,
            &[
                ("__MACOSX/._junk", b"junk"),
                ("default-user/characters/a.json", b"{}"),
            ],
        );

        let layout = scan_archive_layout(&zip_path).expect("scan layout");
        assert_eq!(layout.kind, LayoutKind::UserHandleRoot);
        assert!(layout.source_prefix.as_os_str().is_empty());

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn detects_user_root_layout() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-layout-{}",
            rand::random::<u64>()
        ));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(&zip_path, &[("characters/a.json", b"{}")]);

        let layout = scan_archive_layout(&zip_path).expect("scan layout");
        assert_eq!(layout.kind, LayoutKind::UserRoot);
        assert!(layout.source_prefix.as_os_str().is_empty());

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn detects_single_file_settings_layout() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-layout-{}",
            rand::random::<u64>()
        ));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(&zip_path, &[("settings.json", b"{}")]);

        let layout = scan_archive_layout(&zip_path).expect("scan layout");
        assert_eq!(layout.kind, LayoutKind::UserRoot);

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn detects_wrapped_data_layout() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-layout-{}",
            rand::random::<u64>()
        ));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(
            &zip_path,
            &[("BackupRoot/data/default-user/chats/hello.jsonl", b"{}")],
        );

        let layout = scan_archive_layout(&zip_path).expect("scan layout");
        assert_eq!(layout.kind, LayoutKind::DataRoot);
        assert_eq!(layout.source_prefix, PathBuf::from("BackupRoot").join("data"));

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }

    #[test]
    fn rejects_ambiguous_mixed_roots() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-layout-{}",
            rand::random::<u64>()
        ));
        let zip_path = root.join("fixture.zip");
        fs::create_dir_all(&root).expect("create root");

        write_zip(
            &zip_path,
            &[
                ("data/default-user/characters/a.json", b"{}"),
                ("default-user/characters/b.json", b"{}"),
            ],
        );

        let error = scan_archive_layout(&zip_path).unwrap_err();
        assert!(matches!(error, DomainError::InvalidData(_)));

        crate::infrastructure::persistence::data_archive::shared::cleanup_directory_sync(&root);
    }
}
