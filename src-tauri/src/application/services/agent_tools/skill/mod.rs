mod list;
mod read;
mod specs;

pub(super) use self::list::list;
pub(super) use self::read::read;
pub(super) use self::specs::{skill_list_spec, skill_read_spec};

pub(super) const SKILL_LIST: &str = "skill.list";
pub(super) const SKILL_READ: &str = "skill.read";
