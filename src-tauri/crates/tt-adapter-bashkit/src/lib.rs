//! Bashkit execution over the caller's existing, scoped workspace filesystem.

mod engine;
mod filesystem;

pub use engine::BashkitWorkspaceShell;

#[cfg(test)]
mod tests;
