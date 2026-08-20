mod descriptors;
mod read_activated;

pub(super) use descriptors::worldinfo_read_activated_descriptor;
pub(in crate::services::agent_tools) use read_activated::{
    normalize_entry, ActivatedEntry,
};
pub(super) use read_activated::read_activated;

pub(super) const WORLDINFO_READ_ACTIVATED: &str = "worldinfo.read_activated";

const MAX_WORLDINFO_ENTRIES_PER_READ: usize = 20;
const MAX_WORLDINFO_ENTRY_READ_LINES: usize = 1_200;
const MAX_WORLDINFO_ENTRY_READ_CHARS: usize = 8_000;
const MAX_WORLDINFO_TOTAL_READ_CHARS: usize = 20_000;
