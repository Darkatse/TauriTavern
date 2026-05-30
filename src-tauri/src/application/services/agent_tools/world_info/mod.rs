mod read_activated;
mod specs;

pub(super) use read_activated::read_activated;
pub(super) use specs::worldinfo_read_activated_spec;

pub(super) const WORLDINFO_READ_ACTIVATED: &str = "worldinfo.read_activated";

const MAX_WORLDINFO_ENTRIES_PER_READ: usize = 20;
const MAX_WORLDINFO_FULL_ENTRY_CHARS: usize = 8_000;
const MAX_WORLDINFO_ENTRY_RANGE_CHARS: usize = 8_000;
const MAX_WORLDINFO_TOTAL_READ_CHARS: usize = 20_000;
