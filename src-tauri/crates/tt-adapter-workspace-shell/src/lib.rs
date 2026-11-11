//! Shell and JavaScript execution over the caller's scoped workspace filesystem.

mod engine;
mod filesystem;
mod javascript;
mod kit;

pub use engine::WorkspaceShellEngine;

pub(crate) static JAVASCRIPT_JOBS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

#[cfg(test)]
mod tests;
