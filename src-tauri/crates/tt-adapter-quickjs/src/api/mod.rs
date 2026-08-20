//! 脚本 API 对象构建函数集合（由 Runtime 原生模块导出）。

mod fs;
mod log;

pub(crate) use fs::{OverlayFs, build_workspace_object};
pub(crate) use log::build_log_object;
