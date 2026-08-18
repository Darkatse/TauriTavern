//! 注入脚本全局对象的 API（`$fs` / `$worldInfo` / `$log` / `$variables`）。

pub mod fs;
pub mod log;
pub mod variables;
pub mod world_info;

pub(crate) use fs::register_fs_api;
pub(crate) use log::register_log_api;
pub(crate) use variables::register_variables_api;
pub(crate) use world_info::register_world_info_api;
