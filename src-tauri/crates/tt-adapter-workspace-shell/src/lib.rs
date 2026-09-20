//! Shell and JavaScript execution over the caller's scoped workspace filesystem.

mod engine;
mod filesystem;
mod javascript;
mod kit;
// The existing skill.run_script consumer is removed in the Skill input migration.
mod skill;

pub use engine::WorkspaceShellEngine;
pub use skill::QuickJsScriptEngine;

pub(crate) static JAVASCRIPT_JOBS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

#[cfg(test)]
mod tests;
