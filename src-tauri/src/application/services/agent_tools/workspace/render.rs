use super::MAX_LIST_ENTRIES;
use super::policy::WorkspaceAccessPolicy;
use crate::domain::repositories::workspace_repository::{WorkspaceEntryKind, WorkspaceFileList};

pub(super) fn filter_visible_entries(
    list: WorkspaceFileList,
    policy: &WorkspaceAccessPolicy,
) -> WorkspaceFileList {
    WorkspaceFileList {
        truncated: list.truncated,
        entries: list
            .entries
            .into_iter()
            .filter(|entry| policy.is_visible(&entry.path))
            .collect(),
    }
}

pub(super) fn split_lines_for_display(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split('\n').collect()
}

pub(super) fn format_lines_with_numbers(lines: &[&str], start_line: usize) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let last_line = start_line + lines.len() - 1;
    let width = last_line.to_string().len();
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| format!("{:>width$} | {}", start_line + index, line, width = width))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn render_file_list(list: &WorkspaceFileList) -> String {
    if list.entries.is_empty() {
        return "No visible workspace files found.".to_string();
    }

    let mut lines = list
        .entries
        .iter()
        .map(|entry| match entry.kind {
            WorkspaceEntryKind::Directory => format!("{}/", entry.path.as_str()),
            WorkspaceEntryKind::File => {
                format!(
                    "{} ({} bytes)",
                    entry.path.as_str(),
                    entry.bytes.unwrap_or(0)
                )
            }
        })
        .collect::<Vec<_>>();
    if list.truncated {
        lines.push(format!("... truncated at {MAX_LIST_ENTRIES} entries"));
    }
    lines.join("\n")
}
