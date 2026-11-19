//! PuriCat core.
//!
//! Everything here is independent of the desktop shell so it can be unit-tested
//! without a window and reused later by helper binaries (backup runner, MCP bridge).

pub mod agent;
pub mod agent_tools;
pub mod ai_detect;
pub mod backup;
pub mod backup_schedule;
pub mod db;
pub mod error;
pub mod mcp;
pub mod model;
pub mod restore;
pub mod secrets;
pub mod store;
pub mod table_file;

pub use error::{Error, Result};
