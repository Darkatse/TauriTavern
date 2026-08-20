//! 脚本 API 对象构建函数集合（由 runtime/v1 原生模块导出）。

pub(crate) mod fs;
pub(crate) mod log;

pub(crate) use fs::OverlayFs;
