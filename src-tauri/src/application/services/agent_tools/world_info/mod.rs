mod read_activated;
mod specs;

pub(super) use read_activated::read_activated;
pub(super) use specs::worldinfo_read_activated_spec;

pub(super) const WORLDINFO_READ_ACTIVATED: &str = "worldinfo.read_activated";

const DEFAULT_WORLDINFO_MAX_CHARS: usize = 20_000;
const MAX_WORLDINFO_CHARS: usize = 50_000;
