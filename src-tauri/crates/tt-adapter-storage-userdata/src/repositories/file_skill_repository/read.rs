use std::fs;
use std::path::PathBuf;

use super::FileSkillRepository;
use super::package::{collect_skill_files, sha256_hex};
use super::paths::{normalize_skill_path, validate_skill_name};
use tt_domain::errors::DomainError;
use tt_domain::frozen_macros::MAX_EXPANDED_TEXT_BYTES;
use tt_domain::models::skill::{
    SkillFileKind, SkillFileRef, SkillReadRequest, SkillReadResult, SkillScope, SkillSearchHit,
    SkillSearchRequest, SkillSearchResult,
};
use tt_domain::text_lines::TextLineSelection;
use tt_domain::text_metrics::TextMetrics;
use tt_domain::text_search::PreparedTextSearch;

const MAX_SKILL_READ_LINES: usize = 1_200;

pub(super) async fn read_skill_script(
    repository: &FileSkillRepository,
    scope: &SkillScope,
    name: &str,
    relative_path: &str,
) -> Result<String, DomainError> {
    let name = validate_skill_name(name)?;
    let path = normalize_skill_path(relative_path)?;
    if !path.starts_with("scripts/") {
        return Err(DomainError::InvalidData(format!(
            "Skill script path must stay under scripts/: skills/{name}/{path}"
        )));
    }
    Ok(read_skill_text_file(repository, scope, &name, &path)
        .await?
        .content)
}

struct SkillTextFile {
    scope: SkillScope,
    name: String,
    path: String,
    content: String,
    bytes: u64,
    sha256: String,
    resource_ref: String,
}

pub(super) async fn read_skill_file(
    repository: &FileSkillRepository,
    request: SkillReadRequest,
) -> Result<SkillReadResult, DomainError> {
    if request.max_output_chars == 0 {
        return Err(DomainError::InvalidData(
            "max_output_chars must be greater than 0".to_string(),
        ));
    }

    let mut file =
        read_skill_text_file(repository, &request.scope, &request.name, &request.path).await?;
    if let Some(macros) = &request.frozen_macros
        && !file.path.starts_with("scripts/")
        && let std::borrow::Cow::Owned(text) =
            macros.render(&file.content, MAX_EXPANDED_TEXT_BYTES)?
    {
        file.content = text;
    }
    let selection = TextLineSelection::select(
        &file.content,
        request.start_line.unwrap_or(1),
        request.line_count,
        MAX_SKILL_READ_LINES,
        request.max_output_chars,
    )
    .map_err(|error| DomainError::InvalidData(error.to_string()))?;
    let selected_metrics = TextMetrics::from_text(&selection.content);
    let total_metrics = TextMetrics::from_text(&file.content);
    let next_start_line = selection.next_start_line();
    let truncated = selection.truncated();

    Ok(SkillReadResult {
        scope: file.scope,
        name: file.name,
        path: file.path,
        content: selection.content,
        chars: selected_metrics.chars,
        words: selected_metrics.words,
        total_chars: total_metrics.chars,
        total_words: total_metrics.words,
        total_lines: selection.total_lines,
        start_line: selection.start_line,
        end_line: selection.end_line,
        next_start_line,
        line_truncated: selection.line_truncated,
        bytes: file.bytes,
        sha256: file.sha256,
        truncated,
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

    let skill_root = repository
        .installed_skill_root(&request.scope, &name)
        .await?;
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
        let mut file = read_text_file_at(&skill_root, &request.scope, &name, &file_ref)?;
        if let Some(macros) = &request.frozen_macros
            && !file.path.starts_with("scripts/")
            && let std::borrow::Cow::Owned(text) =
                macros.render(&file.content, MAX_EXPANDED_TEXT_BYTES)?
        {
            file.content = text;
        }
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
        scope: request.scope,
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
    scope: &SkillScope,
    name: &str,
    path: &str,
) -> Result<SkillTextFile, DomainError> {
    let name = validate_skill_name(name)?;
    let path = normalize_skill_path(path)?;
    let skill_root = repository.installed_skill_root(scope, &name).await?;
    let file_ref = SkillFileRef {
        path,
        kind: SkillFileKind::Text,
        media_type: "text/plain".to_string(),
        size_bytes: 0,
        sha256: String::new(),
    };
    read_text_file_at(&skill_root, scope, &name, &file_ref)
}

fn read_text_file_at(
    skill_root: &PathBuf,
    scope: &SkillScope,
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
        scope: scope.clone(),
        name: name.to_string(),
        path: path.clone(),
        content,
        bytes: bytes.len() as u64,
        sha256,
        resource_ref: format!("skills/{name}/{path}"),
    })
}
