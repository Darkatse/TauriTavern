use std::fs;
use std::path::Path;

use serde::Deserialize;

use super::{MAX_SKILL_MD_BYTES, SIDECAR_VERSION};
use crate::domain::errors::DomainError;

#[derive(Debug, Default)]
pub(super) struct SkillFrontmatter {
    pub(super) name: String,
    pub(super) description: String,
    pub(super) license: Option<String>,
    pub(super) author: Option<String>,
    pub(super) version: Option<String>,
    pub(super) tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct TauriTavernSidecar {
    pub(super) version: u32,
    #[serde(default)]
    pub(super) display_name: Option<String>,
    #[serde(default)]
    pub(super) source_kind: Option<String>,
    #[serde(default)]
    pub(super) allow_implicit_invocation: Option<bool>,
    #[serde(default)]
    pub(super) recommended_tools: Vec<String>,
    #[serde(default)]
    pub(super) recommended_context: Option<TauriTavernRecommendedContext>,
    #[serde(default)]
    pub(super) tags: Vec<String>,
    #[serde(default)]
    pub(super) notes: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct TauriTavernRecommendedContext {
    #[serde(default)]
    pub(super) default_budget_tokens: Option<u32>,
    #[serde(default)]
    pub(super) preferred_mode: Option<String>,
}

pub(super) fn read_skill_frontmatter(root: &Path) -> Result<SkillFrontmatter, DomainError> {
    let skill_md_path = root.join("SKILL.md");
    let skill_md_metadata = fs::symlink_metadata(&skill_md_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            DomainError::InvalidData("Skill package must contain SKILL.md".to_string())
        } else {
            DomainError::InternalError(format!(
                "Failed to read SKILL.md metadata '{}': {}",
                skill_md_path.display(),
                error
            ))
        }
    })?;
    if skill_md_metadata.file_type().is_symlink() {
        return Err(DomainError::InvalidData(
            "SKILL.md cannot be a symlink".to_string(),
        ));
    }
    if !skill_md_metadata.is_file() {
        return Err(DomainError::InvalidData(
            "SKILL.md must be a file".to_string(),
        ));
    }
    if skill_md_metadata.len() > MAX_SKILL_MD_BYTES {
        return Err(DomainError::InvalidData(format!(
            "SKILL.md must be <= {MAX_SKILL_MD_BYTES} bytes"
        )));
    }

    let skill_md = fs::read_to_string(&skill_md_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read SKILL.md '{}': {}",
            skill_md_path.display(),
            error
        ))
    })?;
    parse_skill_frontmatter(&skill_md)
}

pub(super) fn parse_skill_frontmatter(text: &str) -> Result<SkillFrontmatter, DomainError> {
    let normalized = text.replace("\r\n", "\n");
    let Some(rest) = normalized.strip_prefix("---\n") else {
        return Err(DomainError::InvalidData(
            "SKILL.md must start with YAML frontmatter".to_string(),
        ));
    };
    let Some(end) = rest.find("\n---") else {
        return Err(DomainError::InvalidData(
            "SKILL.md frontmatter is not closed".to_string(),
        ));
    };
    let yaml = &rest[..end];
    let value: serde_yaml::Value = serde_yaml::from_str(yaml).map_err(|error| {
        DomainError::InvalidData(format!("Invalid SKILL.md frontmatter: {error}"))
    })?;
    let name = yaml_string(&value, "name").ok_or_else(|| {
        DomainError::InvalidData("SKILL.md frontmatter must include name".to_string())
    })?;
    let description = yaml_string(&value, "description").ok_or_else(|| {
        DomainError::InvalidData("SKILL.md frontmatter must include description".to_string())
    })?;
    if description.trim().is_empty() {
        return Err(DomainError::InvalidData(
            "SKILL.md description cannot be empty".to_string(),
        ));
    }

    let metadata = yaml_mapping_child(&value, "metadata");
    Ok(SkillFrontmatter {
        name,
        description,
        license: yaml_string(&value, "license"),
        author: metadata.and_then(|value| yaml_string(value, "author")),
        version: metadata.and_then(|value| yaml_string(value, "version")),
        tags: metadata
            .map(|value| yaml_string_array(value, "tags"))
            .unwrap_or_default(),
    })
}

pub(super) fn read_sidecar(root: &Path) -> Result<Option<TauriTavernSidecar>, DomainError> {
    let path = root.join("agents").join("tauritavern.json");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(DomainError::InternalError(format!(
                "Failed to read Skill sidecar metadata '{}': {}",
                path.display(),
                error
            )));
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(DomainError::InvalidData(
            "agents/tauritavern.json cannot be a symlink".to_string(),
        ));
    }
    let text = fs::read_to_string(&path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read Skill sidecar '{}': {}",
            path.display(),
            error
        ))
    })?;
    let sidecar: TauriTavernSidecar = serde_json::from_str(&text).map_err(|error| {
        DomainError::InvalidData(format!("Invalid agents/tauritavern.json: {error}"))
    })?;
    if sidecar.version != SIDECAR_VERSION {
        return Err(DomainError::InvalidData(format!(
            "Unsupported agents/tauritavern.json version {}",
            sidecar.version
        )));
    }
    let _ = sidecar.allow_implicit_invocation;
    let _ = &sidecar.recommended_tools;
    if let Some(context) = &sidecar.recommended_context {
        let _ = context.default_budget_tokens;
        let _ = &context.preferred_mode;
    }
    let _ = &sidecar.notes;
    Ok(Some(sidecar))
}

fn yaml_string(value: &serde_yaml::Value, key: &str) -> Option<String> {
    let map = value.as_mapping()?;
    let raw = map.get(serde_yaml::Value::String(key.to_string()))?;
    raw.as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn yaml_string_array(value: &serde_yaml::Value, key: &str) -> Vec<String> {
    let Some(map) = value.as_mapping() else {
        return Vec::new();
    };
    let Some(raw) = map.get(serde_yaml::Value::String(key.to_string())) else {
        return Vec::new();
    };
    let Some(items) = raw.as_sequence() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(serde_yaml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn yaml_mapping_child<'a>(
    value: &'a serde_yaml::Value,
    key: &str,
) -> Option<&'a serde_yaml::Value> {
    value
        .as_mapping()?
        .get(serde_yaml::Value::String(key.to_string()))
}
