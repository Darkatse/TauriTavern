mod apply_patch;
mod args;
mod commit;
mod descriptors;
mod list_files;
mod read_file;
mod render;
mod search_files;
mod shell;
mod write_file;

#[cfg(test)]
mod tests;

pub(super) use self::apply_patch::apply_patch;
pub(crate) use self::args::classify_workspace_io_error;
pub(super) use self::commit::commit;
pub(super) use self::descriptors::{
    workspace_apply_patch_descriptor, workspace_commit_descriptor, workspace_list_files_descriptor,
    workspace_read_file_descriptor, workspace_search_files_descriptor, workspace_shell_descriptor,
    workspace_write_file_descriptor,
};
pub(super) use self::list_files::list_files;
pub(crate) use self::list_files::render_workspace_index;
pub(super) use self::read_file::read_file;
pub(super) use self::search_files::search_files;
pub(super) use self::shell::shell;
pub(super) use self::write_file::write_file;

pub(super) const WORKSPACE_LIST_FILES: &str = "workspace.list_files";
pub(super) const WORKSPACE_SEARCH_FILES: &str = "workspace.search_files";
pub(super) const WORKSPACE_READ_FILE: &str = "workspace.read_file";
pub(crate) const WORKSPACE_WRITE_FILE: &str = "workspace.write_file";
pub(crate) const WORKSPACE_APPLY_PATCH: &str = "workspace.apply_patch";
pub(crate) const WORKSPACE_SHELL: &str = "workspace.shell";
pub(super) const WORKSPACE_COMMIT: &str = "workspace.commit";
/// The workspace tools that reach files, i.e. all but commit. Only with one of them does
/// the prompt describe the workspace directories.
pub(crate) const WORKSPACE_FILE_TOOLS: [&str; 6] = [
    WORKSPACE_LIST_FILES,
    WORKSPACE_READ_FILE,
    WORKSPACE_SEARCH_FILES,
    WORKSPACE_WRITE_FILE,
    WORKSPACE_APPLY_PATCH,
    WORKSPACE_SHELL,
];

const DEFAULT_LIST_DEPTH: usize = 2;
const MAX_LIST_DEPTH: usize = 4;
const MAX_LIST_ENTRIES: usize = 200;
/// First-level `persist/` entries named in the workspace index.
const MAX_INDEX_STATE_ENTRIES: usize = 12;
/// Files already in work roots named in the workspace index.
const MAX_INDEX_EXISTING_FILES: usize = 20;
const MAX_READ_BYTES: u64 = 256 * 1024;
const MAX_READ_LINES: usize = 1200;
const MAX_READ_CHARS: usize = 80_000;
const MAX_SEARCH_DEPTH: usize = 8;
const MAX_SEARCH_FILES: usize = 1000;
