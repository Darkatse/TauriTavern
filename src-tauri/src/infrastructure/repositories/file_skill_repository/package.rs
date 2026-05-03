use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use chrono::Utc;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::manifest::{read_sidecar, read_skill_frontmatter};
use super::paths::{normalize_optional_string, normalize_skill_path, validate_skill_name};
use super::source_refs::skill_source_ref_from_import_source;
use super::{MAX_FILES, MAX_SINGLE_FILE_BYTES, MAX_TOTAL_BYTES};
use crate::domain::errors::DomainError;
use crate::domain::models::skill::{
    SkillFileKind, SkillFileRef, SkillImportConflict, SkillImportConflictKind, SkillImportPreview,
    SkillIndexEntry,
};

pub(super) struct ValidatedSkill {
    pub(super) entry: SkillIndexEntry,
    pub(super) preview: SkillImportPreview,
}

pub(super) fn validate_skill_root(
    root: &Path,
    source: Value,
) -> Result<ValidatedSkill, DomainError> {
    let frontmatter = read_skill_frontmatter(root)?;
    let name = validate_skill_name(&frontmatter.name)?;

    let sidecar = read_sidecar(root)?;
    let files = collect_skill_files(root)?;
    let file_count = files.len();
    let total_bytes = files.iter().map(|file| file.size_bytes).sum();
    let has_scripts = files
        .iter()
        .any(|file| file.path == "scripts" || file.path.starts_with("scripts/"));
    let has_binary = files.iter().any(|file| file.kind == SkillFileKind::Binary);
    let installed_hash = directory_hash(root, &files)?;
    let mut tags = BTreeSet::new();
    tags.extend(frontmatter.tags.iter().cloned());
    if let Some(sidecar) = &sidecar {
        tags.extend(
            sidecar
                .tags
                .iter()
                .map(|tag| tag.trim().to_string())
                .filter(|tag| !tag.is_empty()),
        );
    }

    let mut warnings = Vec::new();
    if has_scripts {
        warnings.push(
            "Skill contains scripts/ files; TauriTavern stores them but does not execute them."
                .to_string(),
        );
    }
    if has_binary {
        warnings.push(
            "Skill contains binary files; Agent skill.read can only read UTF-8 text files."
                .to_string(),
        );
    }
    if frontmatter.license.is_none() {
        warnings.push("Skill has no license field.".to_string());
    }

    let entry = SkillIndexEntry {
        name,
        description: frontmatter.description,
        display_name: sidecar
            .as_ref()
            .and_then(|sidecar| normalize_optional_string(sidecar.display_name.as_deref())),
        source_kind: sidecar
            .as_ref()
            .and_then(|sidecar| normalize_optional_string(sidecar.source_kind.as_deref())),
        license: frontmatter.license,
        author: frontmatter.author,
        version: frontmatter.version,
        tags: tags.into_iter().collect(),
        installed_hash,
        file_count,
        total_bytes,
        has_scripts,
        has_binary,
        installed_at: Utc::now(),
        source_refs: Vec::new(),
    };
    let mut entry = entry;
    if let Some(source_ref) = skill_source_ref_from_import_source(&source, &entry.installed_hash)? {
        entry.source_refs.push(source_ref);
    }

    Ok(ValidatedSkill {
        preview: SkillImportPreview {
            skill: entry.clone(),
            files,
            conflict: SkillImportConflict {
                kind: SkillImportConflictKind::New,
                installed_hash: None,
            },
            warnings,
            source,
        },
        entry,
    })
}

pub(super) fn collect_skill_files(root: &Path) -> Result<Vec<SkillFileRef>, DomainError> {
    let mut files = Vec::new();
    collect_skill_files_inner(root, root, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    if files.is_empty() {
        return Err(DomainError::InvalidData(
            "Skill package cannot be empty".to_string(),
        ));
    }
    if files.len() > MAX_FILES {
        return Err(DomainError::InvalidData(format!(
            "Skill package must contain <= {MAX_FILES} files"
        )));
    }
    let total = files.iter().map(|file| file.size_bytes).sum::<u64>();
    if total > MAX_TOTAL_BYTES {
        return Err(DomainError::InvalidData(format!(
            "Skill package exceeds {} bytes",
            MAX_TOTAL_BYTES
        )));
    }
    Ok(files)
}

fn collect_skill_files_inner(
    root: &Path,
    current: &Path,
    files: &mut Vec<SkillFileRef>,
) -> Result<(), DomainError> {
    for entry in fs::read_dir(current).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read Skill directory '{}': {}",
            current.display(),
            error
        ))
    })? {
        let entry = entry.map_err(|error| {
            DomainError::InternalError(format!("Failed to read Skill directory entry: {error}"))
        })?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read Skill file metadata '{}': {}",
                path.display(),
                error
            ))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(DomainError::InvalidData(format!(
                "Skill package cannot contain symlink: {}",
                path.display()
            )));
        }
        if metadata.is_dir() {
            collect_skill_files_inner(root, &path, files)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        if metadata.len() > MAX_SINGLE_FILE_BYTES {
            return Err(DomainError::InvalidData(format!(
                "Skill file '{}' exceeds {} bytes",
                path.display(),
                MAX_SINGLE_FILE_BYTES
            )));
        }
        let relative = path.strip_prefix(root).map_err(|error| {
            DomainError::InternalError(format!("Failed to compute Skill file path: {error}"))
        })?;
        let normalized = normalize_skill_path(&relative.to_string_lossy())?;
        let bytes = fs::read(&path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read Skill file '{}': {}",
                path.display(),
                error
            ))
        })?;
        let kind = if std::str::from_utf8(&bytes).is_ok() {
            SkillFileKind::Text
        } else {
            SkillFileKind::Binary
        };
        let media_type = mime_guess::from_path(&normalized)
            .first_or_octet_stream()
            .essence_str()
            .to_string();
        files.push(SkillFileRef {
            path: normalized,
            kind,
            media_type,
            size_bytes: metadata.len(),
            sha256: sha256_hex(&bytes),
        });
    }
    Ok(())
}

fn directory_hash(root: &Path, files: &[SkillFileRef]) -> Result<String, DomainError> {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update(file.path.as_bytes());
        hasher.update([0]);
        let bytes = fs::read(root.join(&file.path)).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read Skill file '{}' for hashing: {}",
                file.path, error
            ))
        })?;
        hasher.update(bytes);
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
