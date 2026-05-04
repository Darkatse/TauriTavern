use std::fs;
use std::path::PathBuf;

use super::package::{collect_skill_files, sha256_hex};
use super::paths::{normalize_skill_path, validate_skill_name};
use super::{DEFAULT_READ_MAX_CHARS, FileSkillRepository, MAX_READ_CHARS};
use crate::domain::errors::DomainError;
use crate::domain::models::skill::{
    SkillFileKind, SkillFileRef, SkillReadRequest, SkillReadResult, SkillSearchHit,
    SkillSearchRequest, SkillSearchResult,
};
use crate::domain::text_search::PreparedTextSearch;

struct SkillTextFile {
    name: String,
    path: String,
    content: String,
    bytes: u64,
    sha256: String,
    resource_ref: String,
}

struct SelectedText {
    content: String,
    chars: usize,
    total_chars: usize,
    start_char: usize,
    end_char: usize,
    total_lines: usize,
    start_line: usize,
    end_line: usize,
    truncated: bool,
}

pub(super) async fn read_skill_file(
    repository: &FileSkillRepository,
    request: SkillReadRequest,
) -> Result<SkillReadResult, DomainError> {
    let requested_chars = request.max_chars.unwrap_or(DEFAULT_READ_MAX_CHARS);
    if requested_chars == 0 {
        return Err(DomainError::InvalidData(
            "max_chars must be greater than 0".to_string(),
        ));
    }
    if requested_chars > MAX_READ_CHARS {
        return Err(DomainError::InvalidData(format!(
            "max_chars must be <= {MAX_READ_CHARS}"
        )));
    }

    let file = read_skill_text_file(repository, &request.name, &request.path).await?;
    let selected = select_text(&file.content, &request, requested_chars)?;

    Ok(SkillReadResult {
        name: file.name,
        path: file.path,
        content: selected.content,
        chars: selected.chars,
        total_chars: selected.total_chars,
        start_char: selected.start_char,
        end_char: selected.end_char,
        total_lines: selected.total_lines,
        start_line: selected.start_line,
        end_line: selected.end_line,
        bytes: file.bytes,
        sha256: file.sha256,
        truncated: selected.truncated,
        resource_ref: file.resource_ref,
    })
}

pub(super) async fn search_skill_files(
    repository: &FileSkillRepository,
    request: SkillSearchRequest,
) -> Result<SkillSearchResult, DomainError> {
    let name = validate_skill_name(&request.name)?;
    let query = request.query.trim();
    if query.is_empty() {
        return Err(DomainError::InvalidData(
            "query must not be empty".to_string(),
        ));
    }
    if request.limit == 0 {
        return Err(DomainError::InvalidData(
            "limit must be greater than 0".to_string(),
        ));
    }

    let skill_root = installed_skill_root(repository, &name).await?;
    let files = collect_skill_files(&skill_root)?;
    let path_filter = request
        .path
        .as_deref()
        .map(normalize_skill_path)
        .transpose()?;
    let filtered = files
        .into_iter()
        .filter(|file| match path_filter.as_deref() {
            Some(path) => file.path == path || file.path.starts_with(&format!("{path}/")),
            None => true,
        })
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        return Err(DomainError::NotFound(format!(
            "Skill path not found: skills/{name}/{}",
            path_filter.as_deref().unwrap_or("")
        )));
    }

    let search = PreparedTextSearch::new(query, request.limit, request.context_lines);
    let mut searched_files = 0_usize;
    let mut skipped_files = 0_usize;
    let mut hits = Vec::new();

    for file_ref in filtered {
        if file_ref.kind != SkillFileKind::Text {
            skipped_files += 1;
            continue;
        }
        let file = read_text_file_at(&skill_root, &name, &file_ref)?;
        searched_files += 1;
        hits.extend(
            search
                .search(&file.content)
                .into_iter()
                .map(|hit| SkillSearchHit {
                    path: file.path.clone(),
                    score: hit.score,
                    start_line: hit.start_line,
                    end_line: hit.end_line,
                    snippet: hit.snippet,
                    bytes: file.bytes,
                    sha256: file.sha256.clone(),
                    resource_ref: format!(
                        "skills/{}/{}#L{}-L{}",
                        file.name, file.path, hit.start_line, hit.end_line
                    ),
                }),
        );
    }

    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.start_line.cmp(&right.start_line))
    });
    let truncated = hits.len() > request.limit;
    hits.truncate(request.limit);
    let returned_chars = hits
        .iter()
        .map(|hit| hit.snippet.chars().count())
        .sum::<usize>();

    Ok(SkillSearchResult {
        name,
        query: query.to_string(),
        hits,
        searched_files,
        skipped_files,
        truncated,
        returned_chars,
    })
}

async fn read_skill_text_file(
    repository: &FileSkillRepository,
    name: &str,
    path: &str,
) -> Result<SkillTextFile, DomainError> {
    let name = validate_skill_name(name)?;
    let path = normalize_skill_path(path)?;
    let skill_root = installed_skill_root(repository, &name).await?;
    let file_ref = SkillFileRef {
        path,
        kind: SkillFileKind::Text,
        media_type: "text/plain".to_string(),
        size_bytes: 0,
        sha256: String::new(),
    };
    read_text_file_at(&skill_root, &name, &file_ref)
}

async fn installed_skill_root(
    repository: &FileSkillRepository,
    name: &str,
) -> Result<PathBuf, DomainError> {
    let index = repository.load_index().await?;
    if !index.skills.iter().any(|skill| skill.name == name) {
        return Err(DomainError::NotFound(format!("Skill not found: {name}")));
    }

    let skill_root = repository.installed_root().join(name);
    let root_metadata = fs::symlink_metadata(&skill_root).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            DomainError::NotFound(format!("Skill directory not found: {name}"))
        } else {
            DomainError::InternalError(format!(
                "Failed to read Skill directory metadata '{}': {}",
                skill_root.display(),
                error
            ))
        }
    })?;
    if root_metadata.file_type().is_symlink() {
        return Err(DomainError::InvalidData(format!(
            "Skill directory cannot be a symlink: {name}"
        )));
    }
    if !root_metadata.is_dir() {
        return Err(DomainError::InvalidData(format!(
            "Skill installed path is not a directory: {name}"
        )));
    }
    Ok(skill_root)
}

fn read_text_file_at(
    skill_root: &PathBuf,
    name: &str,
    file_ref: &SkillFileRef,
) -> Result<SkillTextFile, DomainError> {
    let path = normalize_skill_path(&file_ref.path)?;
    let full_path = skill_root.join(&path);
    let metadata = fs::symlink_metadata(&full_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            DomainError::NotFound(format!("Skill file not found: skills/{name}/{path}"))
        } else {
            DomainError::InternalError(format!(
                "Failed to read Skill file metadata '{}': {}",
                full_path.display(),
                error
            ))
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(DomainError::InvalidData(format!(
            "Skill file cannot be a symlink: skills/{name}/{path}"
        )));
    }
    if !metadata.is_file() {
        return Err(DomainError::InvalidData(format!(
            "Skill path is not a file: skills/{name}/{path}"
        )));
    }

    let canonical_root = fs::canonicalize(skill_root).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to resolve Skill directory '{}': {}",
            skill_root.display(),
            error
        ))
    })?;
    let canonical_file = fs::canonicalize(&full_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to resolve Skill file '{}': {}",
            full_path.display(),
            error
        ))
    })?;
    if !canonical_file.starts_with(&canonical_root) {
        return Err(DomainError::InvalidData(format!(
            "Skill file escapes installed directory: skills/{name}/{path}"
        )));
    }

    let bytes = fs::read(&full_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read Skill file '{}': {}",
            full_path.display(),
            error
        ))
    })?;
    let content = String::from_utf8(bytes.clone()).map_err(|_| {
        DomainError::InvalidData(format!(
            "Skill file is not UTF-8 text: skills/{name}/{path}"
        ))
    })?;
    let sha256 = sha256_hex(&bytes);

    Ok(SkillTextFile {
        name: name.to_string(),
        path: path.clone(),
        content,
        bytes: bytes.len() as u64,
        sha256,
        resource_ref: format!("skills/{name}/{path}"),
    })
}

fn select_text(
    text: &str,
    request: &SkillReadRequest,
    max_chars: usize,
) -> Result<SelectedText, DomainError> {
    let uses_char_range = request.start_char.is_some();
    let uses_line_range = request.start_line.is_some() || request.line_count.is_some();
    if uses_char_range && uses_line_range {
        return Err(DomainError::InvalidData(
            "Use either start_char/max_chars or start_line/line_count, not both".to_string(),
        ));
    }
    if request.line_count == Some(0) {
        return Err(DomainError::InvalidData(
            "line_count must be greater than 0".to_string(),
        ));
    }
    if request.start_line == Some(0) {
        return Err(DomainError::InvalidData(
            "start_line must be greater than 0".to_string(),
        ));
    }

    let total_chars = text.chars().count();
    let lines = if text.is_empty() {
        Vec::new()
    } else {
        text.split('\n').collect::<Vec<_>>()
    };
    let total_lines = lines.len();

    if uses_char_range {
        return select_char_range(text, total_chars, total_lines, request, max_chars);
    }
    select_line_range(total_chars, &lines, request, max_chars)
}

fn select_char_range(
    text: &str,
    total_chars: usize,
    total_lines: usize,
    request: &SkillReadRequest,
    max_chars: usize,
) -> Result<SelectedText, DomainError> {
    let start_char = request.start_char.unwrap_or(0);
    if total_chars > 0 && start_char >= total_chars {
        return Err(DomainError::InvalidData(format!(
            "start_char {start_char} is outside file with {total_chars} characters"
        )));
    }
    if total_chars == 0 && start_char > 0 {
        return Err(DomainError::InvalidData(
            "start_char must be 0 for an empty file".to_string(),
        ));
    }

    let end_char = start_char.saturating_add(max_chars).min(total_chars);
    let content = slice_chars(text, start_char, end_char);
    let chars = content.chars().count();
    Ok(SelectedText {
        content,
        chars,
        total_chars,
        start_char,
        end_char,
        total_lines,
        start_line: 0,
        end_line: 0,
        truncated: start_char > 0 || end_char < total_chars,
    })
}

fn select_line_range(
    total_chars: usize,
    lines: &[&str],
    request: &SkillReadRequest,
    max_chars: usize,
) -> Result<SelectedText, DomainError> {
    let total_lines = lines.len();
    let start_line = request.start_line.unwrap_or(1);
    if start_line > total_lines.max(1) {
        return Err(DomainError::InvalidData(format!(
            "start_line {start_line} is beyond total lines {total_lines}"
        )));
    }

    let end_line = match request.line_count {
        Some(count) => (start_line + count - 1).min(total_lines),
        None => total_lines,
    };
    let selected = if total_lines == 0 {
        String::new()
    } else {
        lines[start_line - 1..end_line].join("\n")
    };

    let selected_total_chars = selected.chars().count();
    let returned_chars = selected_total_chars.min(max_chars);
    let content = selected.chars().take(returned_chars).collect::<String>();
    let start_char = if start_line <= 1 {
        0
    } else {
        lines[..start_line - 1]
            .iter()
            .map(|line| line.chars().count() + 1)
            .sum()
    };
    let end_char = start_char + content.chars().count();

    Ok(SelectedText {
        content,
        chars: returned_chars,
        total_chars,
        start_char,
        end_char,
        total_lines,
        start_line: if total_lines == 0 { 0 } else { start_line },
        end_line: if total_lines == 0 { 0 } else { end_line },
        truncated: start_line > 1
            || end_line < total_lines
            || returned_chars < selected_total_chars,
    })
}

fn slice_chars(text: &str, start: usize, end: usize) -> String {
    text.chars().skip(start).take(end - start).collect()
}
