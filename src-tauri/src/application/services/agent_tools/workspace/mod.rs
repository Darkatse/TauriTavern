mod apply_patch;
mod args;
mod finish;
mod list_files;
mod policy;
mod read_file;
mod render;
mod specs;
mod write_file;

#[cfg(test)]
mod tests;

pub(super) use self::apply_patch::apply_patch;
pub(super) use self::finish::finish;
pub(super) use self::list_files::list_files;
pub(super) use self::read_file::read_file;
pub(super) use self::specs::{
    workspace_apply_patch_spec, workspace_finish_spec, workspace_list_files_spec,
    workspace_read_file_spec, workspace_write_file_spec,
};
pub(super) use self::write_file::write_file;

pub(super) const WORKSPACE_LIST_FILES: &str = "workspace.list_files";
pub(super) const WORKSPACE_READ_FILE: &str = "workspace.read_file";
pub(super) const WORKSPACE_WRITE_FILE: &str = "workspace.write_file";
pub(super) const WORKSPACE_APPLY_PATCH: &str = "workspace.apply_patch";
pub(super) const WORKSPACE_FINISH: &str = "workspace.finish";

const DEFAULT_LIST_DEPTH: usize = 2;
const MAX_LIST_DEPTH: usize = 4;
const MAX_LIST_ENTRIES: usize = 200;
const MAX_READ_BYTES: u64 = 256 * 1024;
const MAX_READ_LINES: usize = 1200;
const MODEL_WORKSPACE_ROOTS_FOR_MODEL: &str = "output/, scratch/, plan/, summaries/, and persist/";
