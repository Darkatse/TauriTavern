//! 脚本 API 对象构建函数集合（由 runtime/v1 原生模块导出）。

pub mod fs;
pub mod log;
pub mod variables;
pub mod world_info;

pub(crate) use fs::OverlayFs;
