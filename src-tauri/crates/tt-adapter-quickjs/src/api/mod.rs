//! 脚本 API 注册函数集合。

pub mod fs;
pub mod log;
pub mod variables;
pub mod world_info;

pub(crate) use fs::{register_fs_api, OverlayFs};
pub(crate) use log::register_log_api;
pub(crate) use variables::register_variables_api;
pub(crate) use world_info::register_world_info_api;
