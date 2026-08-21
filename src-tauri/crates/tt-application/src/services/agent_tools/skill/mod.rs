mod descriptors;
mod list;
mod read;
mod script;
mod search;

pub(super) use self::descriptors::{
    skill_list_descriptor, skill_read_descriptor, skill_script_descriptor, skill_search_descriptor,
};
pub(super) use self::list::list;
pub(super) use self::read::read;
pub(super) use self::script::ScriptContext;
pub(super) use self::script::script;
pub(super) use self::search::search;

pub(super) const SKILL_LIST: &str = "skill.list";
pub(super) const SKILL_SEARCH: &str = "skill.search";
pub(super) const SKILL_READ: &str = "skill.read";
pub(super) const SKILL_SCRIPT: &str = "skill.run_script";
