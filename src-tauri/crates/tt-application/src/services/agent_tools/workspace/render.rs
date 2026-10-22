use super::MAX_LIST_ENTRIES;
use tt_ports::workspace_fs::{WorkspaceEntryKind, WorkspaceFileList};

pub(super) fn render_file_list(list: &WorkspaceFileList) -> String {
    if list.entries.is_empty() {
        return "No visible workspace files found.".to_string();
    }

    let mut lines = list
        .entries
        .iter()
        .map(|entry| match entry.kind {
            WorkspaceEntryKind::Directory => format!("{}/", entry.path.as_str()),
            WorkspaceEntryKind::File => entry.path.as_str().to_string(),
        })
        .collect::<Vec<_>>();
    if list.truncated {
        lines.push(format!("... truncated at {MAX_LIST_ENTRIES} entries"));
    }
    lines.join("\n")
}
