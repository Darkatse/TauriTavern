use super::FileSkillRepository;
use super::paths::{normalize_skill_path, validate_skill_name};
use tt_domain::errors::DomainError;
use tt_domain::models::agent::WorkspacePath;
use tt_domain::models::skill::{SkillReadRequest, SkillReadResult};
use tt_domain::text_lines::TextLineSelection;
use tt_domain::text_metrics::TextMetrics;
use tt_ports::repositories::skill_repository::SkillRepository;
use tt_ports::workspace_fs::sha256_hex;

const MAX_SKILL_READ_LINES: usize = 1_200;

pub(super) async fn read_skill_file(
    repository: &FileSkillRepository,
    request: SkillReadRequest,
) -> Result<SkillReadResult, DomainError> {
    if request.max_output_chars == 0 {
        return Err(DomainError::InvalidData(
            "max_output_chars must be greater than 0".to_string(),
        ));
    }
    let name = validate_skill_name(&request.name)?;
    let path = WorkspacePath::parse(normalize_skill_path(&request.path)?)?;
    let bytes = repository
        .read_skill_bytes(&request.scope, &name, &path, usize::MAX)
        .await?;
    let size = bytes.len() as u64;
    let sha256 = sha256_hex(&bytes);
    let resource_ref = format!("skills/{name}/{}", path.as_str());
    let content = String::from_utf8(bytes)
        .map_err(|_| DomainError::workspace_file_not_text(&resource_ref))?;
    let selection = TextLineSelection::select(
        &content,
        request.start_line.unwrap_or(1),
        request.line_count,
        MAX_SKILL_READ_LINES,
        request.max_output_chars,
    )
    .map_err(|error| DomainError::InvalidData(error.to_string()))?;
    let selected_metrics = TextMetrics::from_text(&selection.content);
    let total_metrics = TextMetrics::from_text(&content);
    let next_start_line = selection.next_start_line();
    let truncated = selection.truncated();

    Ok(SkillReadResult {
        scope: request.scope,
        name,
        path: path.as_str().to_string(),
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
        bytes: size,
        sha256,
        truncated,
        resource_ref,
    })
}
