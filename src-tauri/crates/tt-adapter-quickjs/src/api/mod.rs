//! 注入脚本全局对象的 API（`$fs` / `$worldInfo` / `$log` / `$sillytavern`）。

pub mod fs;
pub mod log;
pub mod sillytavern;
pub mod world_info;

pub(crate) use fs::register_fs_api;
pub(crate) use log::register_log_api;
pub(crate) use sillytavern::register_sillytavern_api;
pub(crate) use world_info::register_world_info_api;
