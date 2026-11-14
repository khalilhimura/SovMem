pub mod cli;
#[cfg(feature = "mcp")]
pub mod mcp;
pub mod profile;

extern crate self as sovmem;

pub const PROFILE_SCHEMA_VERSION: &str = "nous-ssa-context-v0";
