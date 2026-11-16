//! PuriCat core.
//!
//! Everything here is independent of the desktop shell so it can be unit-tested
//! without a window and reused later by helper binaries (backup runner, MCP bridge).

pub mod ai_detect;
pub mod db;
pub mod error;
pub mod model;
pub mod secrets;
pub mod store;

pub use error::{Error, Result};
