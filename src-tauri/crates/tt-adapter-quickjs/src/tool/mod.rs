//! Tool integration for script execution
//! 
//! Provides Tool descriptors and executors for skill scripts.

pub mod descriptor;
pub mod executor;

pub use descriptor::ScriptToolDescriptor;
pub use executor::ScriptToolExecutor;
