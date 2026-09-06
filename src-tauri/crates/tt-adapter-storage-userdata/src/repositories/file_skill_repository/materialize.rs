use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde_json::Value;
use uuid::Uuid;

use super::archive::extract_archive;
use super::fs_ops::{cleanup_dir, copy_dir_contents};
use super::package::sha256_hex;
use super::paths::normalize_skill_path;
use super::{FileSkillRepository, MAX_FILES, MAX_SINGLE_FILE_BYTES, MAX_TOTAL_BYTES};
use tt_domain::errors::DomainError;
use tt_domain::models::skill::{SkillImportInput, SkillInlineFile};

pub(super) struct PreparedImport {
    pub(super) cleanup_root: PathBuf,
    pub(super) package_root: PathBuf,
    pub(super) source: Value,
}

impl FileSkillRepository {
    pub(super) async fn discover_inputs(
        &self,
        inputs: Vec<SkillImportInput>,
    ) -> Result<Vec<SkillImportInput>, DomainError> {
        self.ensure_layout().await?;
        let mut discovered = Vec::new();

        for input in inputs {
            match input {
                SkillImportInput::Directory { path, source } => {
                    for skill_root in discover_skill_roots(Path::new(&path))? {
                        discovered.push(SkillImportInput::Directory {
                            path: skill_root.to_string_lossy().into_owned(),
                            source: source.clone(),
                        });
                    }
                }
                SkillImportInput::ArchiveFile {
                    path,
                    skill_root: Some(skill_root),
                    source,
                } => discovered.push(SkillImportInput::ArchiveFile {
                    path,
                    skill_root: Some(skill_root),
                    source,
                }),
                SkillImportInput::ArchiveFile {
                    path,
                    skill_root: None,
                    source,
                } => {
                    let staging_dir = self
                        .staging_root()
                        .join(format!("discover-{}", Uuid::new_v4().simple()));
                    fs::create_dir_all(&staging_dir).map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to create Skill discovery directory '{}': {}",
                            staging_dir.display(),
                            error
                        ))
                    })?;
                    let result = (|| {
                        extract_archive(Path::new(&path), &staging_dir)?;
                        discover_skill_roots(&staging_dir)?
                            .into_iter()
                            .map(|root| {
                                let relative = root.strip_prefix(&staging_dir).map_err(|error| {
                                    DomainError::InternalError(format!(
                                        "Failed to compute discovered Skill archive root: {error}"
                                    ))
                                })?;
                                Ok(SkillImportInput::ArchiveFile {
                                    path: path.clone(),
                                    skill_root: if relative.as_os_str().is_empty() {
                                        None
                                    } else {
                                        Some(normalize_skill_path(&relative.to_string_lossy())?)
                                    },
                                    source: source.clone(),
                                })
                            })
                            .collect::<Result<Vec<_>, DomainError>>()
                    })();
                    let archive_inputs = match result {
                        Ok(inputs) => inputs,
                        Err(error) => {
                            cleanup_dir(&staging_dir);
                            return Err(error);
                        }
                    };
                    discovered.extend(archive_inputs);
                    cleanup_dir(&staging_dir);
                }
                _ => {
                    return Err(DomainError::InvalidData(
                        "Skill discovery only supports directory and archiveFile inputs"
                            .to_string(),
                    ));
                }
            }
        }

        Ok(discovered)
    }

    pub(super) async fn materialize_input(
        &self,
        input: &SkillImportInput,
    ) -> Result<PreparedImport, DomainError> {
        self.ensure_layout().await?;
        let staging_dir = self
            .staging_root()
            .join(format!("import-{}", Uuid::new_v4().simple()));
        tokio::fs::create_dir_all(&staging_dir)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create Skill staging directory '{}': {}",
                    staging_dir.display(),
                    error
                ))
            })?;

        let result = match input {
            SkillImportInput::InlineFiles { files, source } => {
                let package_root = staging_dir.join("package");
                fs::create_dir_all(&package_root).map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create Skill inline package root '{}': {}",
                        package_root.display(),
                        error
                    ))
                })?;
                write_inline_files(files, &package_root)?;
                Ok(PreparedImport {
                    cleanup_root: staging_dir.clone(),
                    package_root,
                    source: source.clone(),
                })
            }
            SkillImportInput::Directory { path, source } => {
                let source_root = PathBuf::from(path);
                let selected_root = select_skill_root(&source_root, None)?;
                let package_root = staging_dir.join("package");
                copy_dir_contents(&selected_root, &package_root)?;
                Ok(PreparedImport {
                    cleanup_root: staging_dir.clone(),
                    package_root,
                    source: source.clone(),
                })
            }
            SkillImportInput::ArchiveFile {
                path,
                skill_root,
                source,
            } => {
                let archive_root = staging_dir.join("archive");
                fs::create_dir_all(&archive_root).map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create Skill archive extraction root '{}': {}",
                        archive_root.display(),
                        error
                    ))
                })?;
                extract_archive(Path::new(path), &archive_root)?;
                let selected_root = select_skill_root(&archive_root, skill_root.as_deref())?;
                let package_root = staging_dir.join("package");
                copy_dir_contents(&selected_root, &package_root)?;
                Ok(PreparedImport {
                    cleanup_root: staging_dir.clone(),
                    package_root,
                    source: source.clone(),
                })
            }
            SkillImportInput::ArchiveBase64 {
                file_name,
                content_base64,
                sha256,
                source,
            } => (|| {
                let archive_path = staging_dir.join(require_archive_file_name(file_name)?);
                let bytes = BASE64_STANDARD
                    .decode(content_base64.as_bytes())
                    .map_err(|error| {
                        DomainError::InvalidData(format!(
                            "Invalid base64 Skill archive '{}': {error}",
                            file_name
                        ))
                    })?;
                if bytes.len() as u64 > MAX_TOTAL_BYTES {
                    return Err(DomainError::InvalidData(format!(
                        "Embedded Skill archive '{}' exceeds {} bytes",
                        file_name, MAX_TOTAL_BYTES
                    )));
                }
                if let Some(expected_hash) = sha256.as_deref()
                    && expected_hash.trim().to_ascii_lowercase() != sha256_hex(&bytes)
                {
                    return Err(DomainError::InvalidData(format!(
                        "Embedded Skill archive '{}' sha256 mismatch",
                        file_name
                    )));
                }
                fs::write(&archive_path, bytes).map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to write embedded Skill archive '{}': {}",
                        archive_path.display(),
                        error
                    ))
                })?;

                let archive_root = staging_dir.join("archive");
                fs::create_dir_all(&archive_root).map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create Skill archive extraction root '{}': {}",
                        archive_root.display(),
                        error
                    ))
                })?;
                extract_archive(&archive_path, &archive_root)?;
                let selected_root = select_skill_root(&archive_root, None)?;
                let package_root = staging_dir.join("package");
                copy_dir_contents(&selected_root, &package_root)?;
                Ok(PreparedImport {
                    cleanup_root: staging_dir.clone(),
                    package_root,
                    source: source.clone(),
                })
            })(),
        };

        if result.is_err() {
            cleanup_dir(&staging_dir);
        }
        result
    }
}

fn require_archive_file_name(value: &str) -> Result<&str, DomainError> {
    let file_name = value.trim();
    if file_name.is_empty()
        || file_name.contains('/')
        || file_name.contains('\\')
        || file_name == "."
        || file_name == ".."
    {
        return Err(DomainError::InvalidData(format!(
            "Invalid embedded Skill archive file name: {value}"
        )));
    }
    Ok(file_name)
}

fn write_inline_files(files: &[SkillInlineFile], root: &Path) -> Result<(), DomainError> {
    if files.is_empty() {
        return Err(DomainError::InvalidData(
            "Skill inline import must include at least one file".to_string(),
        ));
    }
    if files.len() > MAX_FILES {
        return Err(DomainError::InvalidData(format!(
            "Skill inline import must include <= {MAX_FILES} files"
        )));
    }
    let mut total_bytes = 0u64;
    for file in files {
        let path = normalize_skill_path(&file.path)?;
        let bytes = match file.encoding.trim().to_ascii_lowercase().as_str() {
            "utf8" | "utf-8" => file.content.as_bytes().to_vec(),
            "base64" => BASE64_STANDARD
                .decode(file.content.as_bytes())
                .map_err(|error| {
                    DomainError::InvalidData(format!(
                        "Invalid base64 content for '{}': {error}",
                        path
                    ))
                })?,
            encoding => {
                return Err(DomainError::InvalidData(format!(
                    "Unsupported Skill inline encoding '{encoding}' for '{path}'"
                )));
            }
        };
        if let Some(expected_size) = file.size_bytes
            && expected_size != bytes.len() as u64
        {
            return Err(DomainError::InvalidData(format!(
                "Inline Skill file '{}' size mismatch: expected {}, got {}",
                path,
                expected_size,
                bytes.len()
            )));
        }
        if let Some(expected_hash) = file.sha256.as_deref()
            && expected_hash.trim().to_ascii_lowercase() != sha256_hex(&bytes)
        {
            return Err(DomainError::InvalidData(format!(
                "Inline Skill file '{}' sha256 mismatch",
                path
            )));
        }
        if bytes.len() as u64 > MAX_SINGLE_FILE_BYTES {
            return Err(DomainError::InvalidData(format!(
                "Skill file '{}' exceeds {} bytes",
                path, MAX_SINGLE_FILE_BYTES
            )));
        }
        total_bytes = total_bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| DomainError::InvalidData("Skill package is too large".to_string()))?;
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(DomainError::InvalidData(format!(
                "Skill package exceeds {} bytes",
                MAX_TOTAL_BYTES
            )));
        }

        let target = root.join(&path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create Skill inline parent '{}': {}",
                    parent.display(),
                    error
                ))
            })?;
        }
        fs::write(&target, bytes).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write Skill inline file '{}': {}",
                target.display(),
                error
            ))
        })?;
    }
    Ok(())
}

fn select_skill_root(root: &Path, skill_root: Option<&str>) -> Result<PathBuf, DomainError> {
    let metadata = fs::symlink_metadata(root).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read Skill import root '{}': {}",
            root.display(),
            error
        ))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(DomainError::InvalidData(
            "Skill import root cannot be a symlink".to_string(),
        ));
    }
    if !metadata.is_dir() {
        return Err(DomainError::InvalidData(format!(
            "Skill import root is not a directory: {}",
            root.display()
        )));
    }

    if let Some(skill_root) = skill_root {
        let selected = root.join(normalize_skill_path(skill_root)?);
        let metadata = fs::symlink_metadata(&selected).map_err(|error| {
            DomainError::InvalidData(format!(
                "Invalid discovered Skill root '{}': {}",
                skill_root, error
            ))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(DomainError::InvalidData(format!(
                "Discovered Skill root is not a regular directory: {skill_root}"
            )));
        }
        return Ok(selected);
    }

    if root.join("SKILL.md").is_file() {
        return Ok(root.to_path_buf());
    }

    let mut candidates = Vec::new();
    for entry in fs::read_dir(root).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read Skill import root '{}': {}",
            root.display(),
            error
        ))
    })? {
        let entry = entry.map_err(|error| {
            DomainError::InternalError(format!("Failed to read Skill import entry: {error}"))
        })?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read Skill import entry metadata '{}': {}",
                entry.path().display(),
                error
            ))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(DomainError::InvalidData(
                "Skill import package cannot contain symlink roots".to_string(),
            ));
        }
        if metadata.is_dir() && entry.path().join("SKILL.md").is_file() {
            candidates.push(entry.path());
        }
    }

    match candidates.len() {
        1 => Ok(candidates.remove(0)),
        0 => Err(DomainError::InvalidData(
            "Skill package must contain exactly one SKILL.md at root or one top-level folder"
                .to_string(),
        )),
        _ => Err(DomainError::InvalidData(
            "Skill package contains multiple candidate SKILL.md roots".to_string(),
        )),
    }
}

fn discover_skill_roots(root: &Path) -> Result<Vec<PathBuf>, DomainError> {
    let metadata = fs::symlink_metadata(root).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read Skill discovery root '{}': {}",
            root.display(),
            error
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(DomainError::InvalidData(format!(
            "Skill discovery root must be a regular directory: {}",
            root.display()
        )));
    }

    let mut roots = Vec::new();
    discover_skill_roots_inner(root, &mut roots)?;
    roots.sort();
    if roots.is_empty() {
        return Err(DomainError::InvalidData(format!(
            "No SKILL.md found under Skill import root: {}",
            root.display()
        )));
    }
    Ok(roots)
}

fn discover_skill_roots_inner(current: &Path, roots: &mut Vec<PathBuf>) -> Result<(), DomainError> {
    let skill_md = current.join("SKILL.md");
    match fs::symlink_metadata(&skill_md) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(DomainError::InvalidData(format!(
                "SKILL.md cannot be a symlink: {}",
                skill_md.display()
            )));
        }
        Ok(metadata) if metadata.is_file() => {
            roots.push(current.to_path_buf());
            return Ok(());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(DomainError::InternalError(format!(
                "Failed to inspect Skill candidate '{}': {}",
                skill_md.display(),
                error
            )));
        }
    }

    for entry in fs::read_dir(current).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read Skill discovery directory '{}': {}",
            current.display(),
            error
        ))
    })? {
        let entry = entry.map_err(|error| {
            DomainError::InternalError(format!("Failed to read Skill discovery entry: {error}"))
        })?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to inspect Skill discovery entry '{}': {}",
                entry.path().display(),
                error
            ))
        })?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            discover_skill_roots_inner(&entry.path(), roots)?;
        }
    }
    Ok(())
}
